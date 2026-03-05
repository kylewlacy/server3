use std::{collections::HashMap, sync::Arc};

use axum::{Json, extract::State};
use reqwest::StatusCode;

use crate::upstream::{Upstream, UpstreamResource};

pub fn router(state: AppState) -> axum::Router {
    axum::Router::new()
        .fallback(axum::routing::get(get_resource))
        .with_state(state)
}

#[derive(Clone)]
pub struct AppState {
    pub upstream: Option<Arc<dyn Upstream + Send + Sync>>,
    pub host_upstreams: Arc<HashMap<Arc<str>, Arc<dyn Upstream + Send + Sync>>>,
}

impl AppState {
    fn upstream_for_host(&self, host: Option<&str>) -> Option<&Arc<dyn Upstream + Send + Sync>> {
        host.and_then(|host| self.host_upstreams.get(host))
            .or(self.upstream.as_ref())
    }
}

async fn get_resource(
    State(state): State<AppState>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
) -> Result<UpstreamResource, AppError> {
    let host_value = host(&headers)?;
    let hostname = host_value.map(|host_value| {
        if let Some((host, _port)) = host_value.rsplit_once(':') {
            host
        } else {
            host_value
        }
    });
    let upstream = state
        .upstream_for_host(hostname)
        .ok_or_else(|| AppError::NoUpstreamServer {
            hostname: hostname.map(ToString::to_string),
        })?;

    let path = uri.path().trim_matches('/');
    let resource = upstream.get(path).await?.ok_or_else(|| ResourceNotFound {
        path: path.to_string(),
    })?;
    Ok(resource)
}

fn host(headers: &axum::http::HeaderMap) -> Result<Option<&str>, AppError> {
    let mut host_values = headers.get_all("host").iter();
    let Some(host_value) = host_values.next() else {
        return Ok(None);
    };

    if host_values.next().is_some() {
        // A duplicate 'Host' header is not allowed
        return Err(AppError::InvalidHostHeader);
    }

    let host_value = host_value
        .to_str()
        .map_err(|_| AppError::InvalidHostHeader)?;
    Ok(Some(host_value))
}

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("upstream error: {0}")]
    Upstream(#[from] crate::upstream::UpstreamError),

    #[error(transparent)]
    NotFound(#[from] ResourceNotFound),

    #[error("no upstream server configured for this host")]
    NoUpstreamServer { hostname: Option<String> },

    #[error("invalid 'Host' header value")]
    InvalidHostHeader,
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            Self::Upstream(error) => {
                tracing::warn!("store error: {error:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("store error: {error}"),
                )
            }
            Self::NotFound(not_found) => {
                tracing::info!("{not_found}");
                (StatusCode::NOT_FOUND, not_found.to_string())
            }
            Self::NoUpstreamServer { ref hostname } => {
                tracing::info!(hostname, "host not configured");
                (StatusCode::NOT_FOUND, self.to_string())
            }
            Self::InvalidHostHeader => (StatusCode::BAD_REQUEST, self.to_string()),
        };

        (status, Json(JsonError { message })).into_response()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct JsonError {
    message: String,
}

#[derive(Debug, thiserror::Error)]
#[error("resource not found: {path}")]
struct ResourceNotFound {
    path: String,
}
