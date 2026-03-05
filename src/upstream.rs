pub mod cache;
pub mod http;

#[async_trait::async_trait]
pub trait Upstream {
    async fn get(&self, path: &str) -> Result<Option<UpstreamResource>, UpstreamError>;
}

pub struct UpstreamResource {
    pub headers: UpstreamResourceHeaders,
    pub body: axum::body::Body,
}

#[derive(Debug, Clone)]
pub struct UpstreamResourceHeaders {
    pub content_type: Option<axum::http::HeaderValue>,
}

impl axum::response::IntoResponse for UpstreamResource {
    fn into_response(self) -> axum::response::Response {
        let mut response = axum::response::Response::new(self.body);
        if let Some(content_type) = self.headers.content_type {
            response.headers_mut().insert("content-type", content_type);
        }

        response
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
