use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Context as _;
use axum::extract::State;
use clap::Parser;
use figment::providers::Format as _;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

use server3::upstream::{ArcUpstream, http::HttpUpstream, s3::S3Upstream};

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

    let routes = server3::cache::CacheRoutes::from_config(&config.cache, &config.routes);
    let routes = Arc::new(routes);

    let upstream = if let Some(upstream) = config.upstream {
        let upstream = build_upstream(upstream).await?;
        let upstream = server3::cache::Cache::new(
            cache_storage.clone(),
            "**".into(),
            routes.clone(),
            upstream,
        );
        Some(upstream)
    } else {
        None
    };

    let mut host_upstreams = HashMap::new();
    for (host, host_config) in config.hosts {
        let host = Arc::<str>::from(host);
        let host_cache = host_config.cache.as_ref().unwrap_or(&config.cache);
        let host_routes = host_config.routes.as_ref().unwrap_or(&config.routes);
        let routes = server3::cache::CacheRoutes::from_config(host_cache, host_routes);

        let Some(upstream) = host_config.upstream else {
            continue;
        };
        let upstream = build_upstream(upstream).await?;
        let upstream = server3::cache::Cache::new(
            cache_storage.clone(),
            host.clone(),
            Arc::new(routes),
            upstream,
        );
        host_upstreams.insert(host, upstream);
    }
    let host_upstreams = Arc::new(host_upstreams);

    anyhow::ensure!(
        upstream.is_some() || !host_upstreams.is_empty(),
        "no upstreams configured",
    );

    let state = server3::app::AppState {
        upstream,
        host_upstreams,
        metadata: Arc::new(config.metadata),
    };

    let app = server3::app::router(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            request_metrics_middleware,
        ))
        .layer(
            tower::ServiceBuilder::new().layer(
                tower_http::trace::TraceLayer::new_for_http()
                    .make_span_with(|_req: &axum::http::Request<_>| {
                        let request_id = uuid::Uuid::new_v4();
                        tracing::info_span!("request", %request_id)
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
    let shutdown_signal = shutdown_signal();

    tokio::select! {
        () = prometheus_upkeep_fut => {},
        result = app_server_fut => {
            result?;
        }
        result = app_metrics_server_fut => {
            result?;
        }
        result = shutdown_signal => {
            result??;
            tracing::info!("shutting down");
        }
    };

    Ok(())
}

fn shutdown_signal() -> tokio::sync::oneshot::Receiver<anyhow::Result<()>> {
    let (tx, rx) = tokio::sync::oneshot::channel::<anyhow::Result<()>>();

    tokio::task::spawn(async move {
        let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());

        let sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        let mut sigterm = match sigterm {
            Ok(sigterm) => sigterm,
            Err(error) => {
                let _ = tx.send(Err(error).context("failed to install SIGTERM handler"));
                return;
            }
        };

        tokio::select! {
            result = &mut ctrl_c => {
                let _ = tx.send(result.context("Ctrl-C handler failed"));
            }
            Some(()) = sigterm.recv() => {
                let _ = tx.send(Ok(()));
            }
        }
    });

    rx
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

#[axum::debug_middleware]
async fn request_metrics_middleware(
    State(state): State<server3::app::AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl axum::response::IntoResponse {
    let start = std::time::Instant::now();

    let host = server3::app::host(req.headers()).ok().flatten();
    let (host, path) = state.filtered_host_and_path(host, req.uri().path());
    let host = host.map(|host| &**host).unwrap_or("**");
    let path = path.unwrap_or("**");
    let method = req.method().to_string();
    let response = next.run(req).await;

    let status = response.status().as_u16().to_string();
    let labels = [
        ("host", host.to_string()),
        ("path", path.to_string()),
        ("method", method),
        ("status", status),
    ];

    let duration = start.elapsed();

    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_requests_duration_seconds", &labels).record(duration.as_secs_f64());

    response
}

async fn build_upstream(config: server3::config::UpstreamConfig) -> anyhow::Result<ArcUpstream> {
    let upstream: ArcUpstream = match config {
        server3::config::UpstreamConfig::Http(config) => Arc::new(HttpUpstream::new(config)?),
        server3::config::UpstreamConfig::S3(config) => Arc::new(S3Upstream::new(config).await?),
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
