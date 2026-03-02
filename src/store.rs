pub mod cache;
pub mod http;

#[async_trait::async_trait]
pub trait Store {
    async fn get_object(&self, key: &str) -> Result<Option<StoreObject>, StoreError>;
}

pub struct StoreObject {
    pub headers: StoreObjectHeaders,
    pub body: axum::body::Body,
}

#[derive(Debug, Clone)]
pub struct StoreObjectHeaders {
    pub content_type: Option<axum::http::HeaderValue>,
}

impl axum::response::IntoResponse for StoreObject {
    fn into_response(self) -> axum::response::Response {
        let mut response = axum::response::Response::new(self.body);
        if let Some(content_type) = self.headers.content_type {
            response.headers_mut().insert("content-type", content_type);
        }

        response
    }
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
