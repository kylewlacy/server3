use std::{collections::HashMap, sync::Arc};

use axum::{Json, extract::State, http::HeaderValue, response::IntoResponse as _};
use reqwest::StatusCode;

use crate::{
    cache::{Cache, CachedResourceResponse},
    upstream::{ArcUpstream, Upstream},
};

pub fn router(state: AppState) -> axum::Router {
    axum::Router::new()
        .fallback(axum::routing::get(get_resource))
        .with_state(state)
}

#[derive(Clone)]
pub struct AppState {
    pub upstream: Option<Cache<ArcUpstream>>,
    pub host_upstreams: Arc<HashMap<Arc<str>, Cache<ArcUpstream>>>,
    pub metadata: Arc<crate::config::MetadataConfig>,
}

impl AppState {
    pub fn filtered_host_and_path(
        &self,
        host: Option<&str>,
        path: &str,
    ) -> (Option<&Arc<str>>, Option<&str>) {
        if let Some(host) = host
            && let Some((host, upstream)) = self.host_upstreams.get_key_value(host)
        {
            let path = upstream.get_path_pattern(path);
            (Some(host), path)
        } else {
            let path = self
                .upstream
                .as_ref()
                .and_then(|upstream| upstream.get_path_pattern(path));
            (None, path)
        }
    }

    fn cache_for_host(
        &self,
        host: Option<&str>,
    ) -> Option<&Cache<Arc<dyn Upstream + Send + Sync>>> {
        host.and_then(|host| self.host_upstreams.get(host))
            .or(self.upstream.as_ref())
    }
}

async fn get_resource(
    State(state): State<AppState>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, AppError> {
    let host_value = host(&headers)?;
    let hostname = host_value.map(|host_value| {
        if let Some((host, _port)) = host_value.rsplit_once(':') {
            host
        } else {
            host_value
        }
    });
    let cache = state
        .cache_for_host(hostname)
        .ok_or_else(|| AppError::NoUpstreamServer {
            hostname: hostname.map(ToString::to_string),
        })?;

    let resource = cache
        .get(uri.path(), std::time::Instant::now())
        .await?
        .ok_or_else(|| ResourceNotFound {
            path: uri.path().to_string(),
        })?;
    let CachedResourceResponse {
        resource,
        outcome,
        expires_at,
    } = resource;
    let mut response = resource.into_response();

    let headers = response.headers_mut();
    if state.metadata.cache_outcome {
        headers.insert(
            "server3-cache-outcome",
            axum::http::HeaderValue::from_static(outcome.as_str()),
        );
    }
    if state.metadata.expires_at {
        let expires_at =
            expires_at.and_then(|expires_at| HeaderValue::from_str(&expires_at.to_string()).ok());
        if let Some(expires_at) = expires_at {
            headers.insert("server3-expires-at", expires_at);
        }
    }
    if let Some(node_name) = &state.metadata.node_name {
        let node_name = HeaderValue::from_str(node_name);
        if let Ok(node_name) = node_name {
            headers.insert("server3-node-name", node_name);
        }
    }

    Ok(response)
}

pub fn host(headers: &axum::http::HeaderMap) -> Result<Option<&str>, InvalidHostHeader> {
    let mut host_values = headers.get_all("host").iter();
    let Some(host_value) = host_values.next() else {
        return Ok(None);
    };

    if host_values.next().is_some() {
        // A duplicate 'Host' header is not allowed
        return Err(InvalidHostHeader);
    }

    let host_value = host_value.to_str().map_err(|_| InvalidHostHeader)?;
    Ok(Some(host_value))
}

#[derive(Debug, thiserror::Error)]
#[error("invalid 'Host' header value")]
pub struct InvalidHostHeader;

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("upstream error: {0}")]
    Upstream(#[from] crate::upstream::UpstreamError),

    #[error(transparent)]
    NotFound(#[from] ResourceNotFound),

    #[error("no upstream server configured for this host")]
    NoUpstreamServer { hostname: Option<String> },

    #[error(transparent)]
    InvalidHostHeader(#[from] InvalidHostHeader),
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            Self::Upstream(error) => {
                tracing::warn!("upstream error: {error:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("upstream error: {error}"),
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
            Self::InvalidHostHeader(InvalidHostHeader) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
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
