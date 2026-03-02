use std::{collections::HashMap, sync::Arc};

use axum::{Json, body::Body, extract::State};
use reqwest::StatusCode;

use crate::store::Store;

pub fn router(state: AppState) -> axum::Router {
    axum::Router::new()
        .fallback(axum::routing::get(get_object))
        .with_state(state)
}

#[derive(Clone)]
pub struct AppState {
    pub store: Option<Arc<dyn Store + Send + Sync>>,
    pub host_stores: Arc<HashMap<Arc<str>, Arc<dyn Store + Send + Sync>>>,
}

impl AppState {
    fn store_for_host(&self, host: Option<&str>) -> Option<&Arc<dyn Store + Send + Sync>> {
        host.and_then(|host| self.host_stores.get(host))
            .or(self.store.as_ref())
    }
}

async fn get_object(
    State(state): State<AppState>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
) -> Result<Body, AppError> {
    let host_value = host(&headers)?;
    let hostname = host_value.map(|host_value| {
        if let Some((host, _port)) = host_value.rsplit_once(':') {
            host
        } else {
            host_value
        }
    });
    let store = state
        .store_for_host(hostname)
        .ok_or_else(|| AppError::HostNotConfigured {
            hostname: hostname.map(ToString::to_string),
        })?;

    let key = uri.path().trim_matches('/');
    let object = store.get_object(key).await?.ok_or_else(|| ObjectNotFound {
        key: key.to_string(),
    })?;
    Ok(object)
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
    #[error("error from store")]
    Store(#[from] crate::store::StoreError),

    #[error(transparent)]
    NotFound(#[from] ObjectNotFound),

    #[error("no upstream store configured for this host")]
    HostNotConfigured { hostname: Option<String> },

    #[error("invalid 'Host' header value")]
    InvalidHostHeader,
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            Self::Store(error) => {
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
            Self::HostNotConfigured { ref hostname } => {
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
#[error("object not found in store: {key}")]
struct ObjectNotFound {
    key: String,
}
