pub mod cache;
pub mod http;

#[async_trait::async_trait]
pub trait Store {
    async fn get_object(&self, key: &str) -> Result<Option<axum::body::Body>, StoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
