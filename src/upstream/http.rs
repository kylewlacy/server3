use futures::StreamExt as _;

use crate::{
    config::UpstreamHttpConfig,
    upstream::{Upstream, UpstreamError, UpstreamResource, UpstreamResourceHeaders},
};

pub struct HttpUpstream {
    reqwest: reqwest::Client,
    url: url::Url,
}

impl HttpUpstream {
    pub fn new(config: UpstreamHttpConfig) -> anyhow::Result<Self> {
        let mut reqwest = reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .pool_max_idle_per_host(10);

        if let Some(timeout) = config.http_timeout {
            reqwest = reqwest.timeout(timeout);
        }

        if let Some(timeout) = config.http_read_timeout {
            reqwest = reqwest.read_timeout(timeout);
        }

        if let Some(timeout) = config.http_connect_timeout {
            reqwest = reqwest.connect_timeout(timeout);
        }

        let reqwest = reqwest.build()?;

        Ok(Self {
            reqwest,
            url: config.url,
        })
    }
}

#[async_trait::async_trait]
impl Upstream for HttpUpstream {
    async fn get(&self, path: &str) -> Result<Option<UpstreamResource>, UpstreamError> {
        let url = self.url.join(path)?;

        let response = self.reqwest.get(url).send().await?;
        if matches!(response.status(), reqwest::StatusCode::NOT_FOUND) {
            return Ok(None);
        }

        let response = response.error_for_status()?;
        let content_type = response.headers().get("content-type").cloned();
        let body = axum_body_from_reqwest_response(response);
        let object = UpstreamResource {
            body,
            headers: UpstreamResourceHeaders { content_type },
        };
        Ok(Some(object))
    }
}

fn axum_body_from_reqwest_response(response: reqwest::Response) -> axum::body::Body {
    let response_size: Option<u64> = response
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|content_length| content_length.to_str().ok()?.parse().ok());
    let response_stream = response
        .bytes_stream()
        .map(|bytes| bytes.map_err(UpstreamError::from));

    let size_hint = response_size.map(http_body::SizeHint::with_exact);
    let body =
        crate::response::BodyWithSize::new(crate::response::StreamBody::new(response_stream))
            .with_size_hint(size_hint);
    axum::body::Body::new(body)
}
