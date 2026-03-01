use server3::{
    config::{CacheConfig, UpstreamConfig},
    models::HashId,
    store::http::HttpStore,
};

pub fn fake_hash_id(n: u64) -> HashId {
    let bytes = n.to_be_bytes();
    let mut hash_bytes = [0; HashId::LEN];
    hash_bytes[(HashId::LEN - bytes.len())..].copy_from_slice(&bytes);
    HashId::from_bytes(hash_bytes)
}

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
    TestContext {
        cache_dir: std::sync::OnceLock::new(),
    }
}

pub fn mockito_http_store(mockito: &mockito::Server) -> HttpStore {
    let config = UpstreamConfig {
        url: mockito.url().parse().unwrap(),
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
