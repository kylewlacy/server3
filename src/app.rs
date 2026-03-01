use std::sync::Arc;

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
    pub store: Arc<dyn Store + Send + Sync>,
}

async fn get_object(State(state): State<AppState>, uri: axum::http::Uri) -> Result<Body, AppError> {
    let key = uri.path().trim_matches('/');
    let body = state
        .store
        .get_object(key)
        .await?
        .ok_or_else(|| ObjectNotFound {
            key: key.to_string(),
        })?;
    Ok(body)
}

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("error from store")]
    Store(#[from] crate::store::StoreError),

    #[error(transparent)]
    NotFound(#[from] ObjectNotFound),
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
