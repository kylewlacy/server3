use server3::{
    config::{CacheConfig, UpstreamConfig},
    store::http::HttpStore,
};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

pub async fn body_to_bytes(body: axum::body::Body) -> Vec<u8> {
    let bytes = axum::body::to_bytes(body, 10_000_000).await.unwrap();
    bytes.to_vec()
}

pub async fn body_to_string(body: axum::body::Body) -> String {
    let bytes = body_to_bytes(body).await;
    String::from_utf8(bytes).unwrap()
}

pub struct TestContext {
    cache_dir: std::sync::OnceLock<tempfile::TempDir>,
}

pub fn test_context() -> TestContext {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .compact()
                .with_target(false)
                .without_time(),
        )
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("server3=info,warn")),
        )
        .init();
    TestContext {
        cache_dir: std::sync::OnceLock::new(),
    }
}

pub fn mockito_http_store(mockito: &mockito::Server) -> HttpStore {
    mockito_http_store_with_prefix(mockito, "")
}

pub fn mockito_http_store_with_prefix(mockito: &mockito::Server, prefix: &str) -> HttpStore {
    let base_url: url::Url = mockito.url().parse().unwrap();
    let url = if prefix.is_empty() {
        base_url
    } else {
        let relative_path = format!("{}/", prefix.trim_matches('/'));
        base_url.join(&relative_path).unwrap()
    };
    let config = UpstreamConfig {
        url,
        http_timeout: None,
        http_read_timeout: None,
        http_connect_timeout: None,
    };
    HttpStore::new(config).unwrap()
}

pub fn cache_config(ctx: &TestContext) -> CacheConfig {
    let cache_dir = ctx
        .cache_dir
        .get_or_init(|| tempfile::TempDir::new().expect("failed to create temp dir"));

    CacheConfig {
        dir: cache_dir.path().to_path_buf(),
        ..Default::default()
    }
}
