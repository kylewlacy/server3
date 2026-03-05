use server3::{
    config::{StorageConfig, UpstreamHttpConfig},
    upstream::{UpstreamResource, http::HttpUpstream},
};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

pub async fn object_to_bytes(object: UpstreamResource) -> bstr::BString {
    let bytes = axum::body::to_bytes(object.body, 10_000_000).await.unwrap();
    bytes.to_vec().into()
}

pub async fn object_to_string(object: UpstreamResource) -> String {
    let bytes = object_to_bytes(object).await;
    String::from_utf8(bytes.into()).unwrap()
}

pub fn object_content_type(object: &UpstreamResource) -> Option<&bstr::BStr> {
    object
        .headers
        .content_type
        .as_ref()
        .map(|content_type| bstr::BStr::new(content_type.as_bytes()))
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

pub fn mockito_http_store(mockito: &mockito::Server) -> HttpUpstream {
    mockito_http_store_with_prefix(mockito, "")
}

pub fn mockito_http_store_with_prefix(mockito: &mockito::Server, prefix: &str) -> HttpUpstream {
    let base_url: url::Url = mockito.url().parse().unwrap();
    let url = if prefix.is_empty() {
        base_url
    } else {
        let relative_path = format!("{}/", prefix.trim_matches('/'));
        base_url.join(&relative_path).unwrap()
    };
    HttpUpstream::new(UpstreamHttpConfig {
        url,
        http_timeout: None,
        http_read_timeout: None,
        http_connect_timeout: None,
    })
    .unwrap()
}

pub fn cache_config(ctx: &TestContext) -> StorageConfig {
    let cache_dir = ctx
        .cache_dir
        .get_or_init(|| tempfile::TempDir::new().expect("failed to create temp dir"));

    StorageConfig {
        dir: cache_dir.path().to_path_buf(),
        ..Default::default()
    }
}
