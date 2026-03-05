use std::{collections::HashMap, path::PathBuf, sync::Arc};

use clap::Parser;
use figment::providers::Format as _;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

use server3::upstream::{ArcUpstream, http::HttpUpstream};

#[derive(Debug, Clone, Parser)]
enum Args {
    Serve {
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Args::Serve {
        config: config_path,
    } = Args::parse();

    let mut config = figment::Figment::new();
    if let Some(config_path) = config_path {
        config = config.merge(Styx::file(config_path));
    };
    config = config.merge(figment::providers::Env::prefixed("SERVER3_").split("__"));
    let config: server3::config::Config = config.extract()?;

    const DEFAULT_TRACING_DIRECTIVE: &str = concat!(env!("CARGO_CRATE_NAME"), "=info,warn");
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_TRACING_DIRECTIVE)),
        )
        .init();
    let prometheus = install_prometheus_recorder()?;

    let cache_storage = server3::cache::CacheStorage::new(config.storage)?;
    let cache_storage = Arc::new(cache_storage);

    let upstream = if let Some(upstream) = config.upstream {
        let upstream = build_upstream(upstream)?;
        let upstream =
            server3::cache::Cache::new(cache_storage.clone(), "DEFAULT".into(), upstream);
        Some(upstream)
    } else {
        None
    };
    let host_upstreams = config
        .hosts
        .into_iter()
        .filter_map(|(host, host_config)| {
            let host = Arc::<str>::from(host);
            let upstream = host_config.upstream?;
            let upstream = build_upstream(upstream);
            let upstream = match upstream {
                Ok(upstream) => upstream,
                Err(error) => {
                    return Some(Err(error));
                }
            };
            let upstream =
                server3::cache::Cache::new(cache_storage.clone(), host.clone(), upstream);
            Some(Ok((host, upstream)))
        })
        .collect::<anyhow::Result<HashMap<_, _>>>()?;
    let host_upstreams = Arc::new(host_upstreams);

    anyhow::ensure!(
        upstream.is_some() || !host_upstreams.is_empty(),
        "no upstreams configured",
    );

    let state = server3::app::AppState {
        upstream,
        host_upstreams,
    };

    let app = server3::app::router(state)
        .layer(axum::middleware::from_fn(request_metrics_middleware))
        .layer(
            tower::ServiceBuilder::new().layer(
                tower_http::trace::TraceLayer::new_for_http()
                    .make_span_with(|req: &axum::http::Request<_>| {
                        let path = if let Some(path) =
                            req.extensions().get::<axum::extract::MatchedPath>()
                        {
                            path.as_str()
                        } else {
                            req.uri().path()
                        };
                        let request_id = uuid::Uuid::new_v4();
                        tracing::info_span!("request", path, %request_id)
                    })
                    .on_request(|_req: &axum::http::Request<_>, _span: &tracing::Span| {
                        tracing::debug!("started request");
                    })
                    .on_response(
                        |res: &axum::http::Response<_>,
                         latency: std::time::Duration,
                         _span: &tracing::Span| {
                            tracing::debug!(
                                latency_secs = latency.as_secs_f32(),
                                response_code = res.status().as_u16(),
                                "finished request",
                            );
                        },
                    ),
            ),
        );

    let metrics_app = axum::Router::new().route(
        "/metrics",
        axum::routing::get({
            let prometheus = prometheus.clone();
            async move || prometheus.render()
        }),
    );

    let prometheus_upkeep_fut = async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            prometheus.run_upkeep();
        }
    };
    let app_server_fut = async {
        let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
        let addr = listener.local_addr()?;
        tracing::info!("listening on {addr}");
        axum::serve(listener, app).await?;
        anyhow::Ok(())
    };
    let app_metrics_server_fut = async {
        let listener = tokio::net::TcpListener::bind(&config.bind_metrics_address).await?;
        let addr = listener.local_addr()?;
        tracing::info!("listening for metrics on {addr}");
        axum::serve(listener, metrics_app).await?;
        anyhow::Ok(())
    };

    tokio::select! {
        () = prometheus_upkeep_fut => {},
        result = app_server_fut => {
            result?;
        }
        result = app_metrics_server_fut => {
            result?;
        }
    };

    Ok(())
}

fn install_prometheus_recorder() -> anyhow::Result<metrics_exporter_prometheus::PrometheusHandle> {
    const EXPONENTIAL_SECONDS: &[f64] = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];

    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .set_buckets_for_metric(
            metrics_exporter_prometheus::Matcher::Full(
                "http_requests_duration_seconds".to_string(),
            ),
            EXPONENTIAL_SECONDS,
        )?
        .install_recorder()?;

    Ok(prometheus)
}

async fn request_metrics_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl axum::response::IntoResponse {
    let start = std::time::Instant::now();

    let method = req.method().to_string();
    let path = req
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map_or_else(|| req.uri().path(), |path| path.as_str())
        .to_owned();
    let response = next.run(req).await;

    let status = response.status().as_u16().to_string();
    let labels = [("method", method), ("path", path), ("status", status)];

    let duration = start.elapsed();

    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_requests_duration_seconds", &labels).record(duration.as_secs_f64());

    response
}

fn build_upstream(config: server3::config::UpstreamConfig) -> anyhow::Result<ArcUpstream> {
    let upstream = match config {
        server3::config::UpstreamConfig::Http(config) => Arc::new(HttpUpstream::new(config)?),
    };
    Ok(upstream)
}

enum Styx {}

impl figment::providers::Format for Styx {
    type Error = serde_styx::Error;

    const NAME: &'static str = "Styx";

    fn from_str<T: serde::de::DeserializeOwned>(string: &str) -> Result<T, Self::Error> {
        serde_styx::from_str(string)
    }
}
