use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::engine::EngineError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("locked out: try again in {0} seconds")]
    LockedOut(u64),

    #[error("internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),

    #[error(transparent)]
    Db(#[from] rusqlite::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<EngineError> for AppError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::NotFound(msg) => AppError::NotFound(msg),
            EngineError::AlreadyExists(msg) => AppError::Conflict(msg),
            EngineError::InvalidInput(msg) => AppError::BadRequest(msg),
            EngineError::Internal(msg) => AppError::Internal(msg),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Conflict(_) => (StatusCode::CONFLICT, self.to_string()),
            AppError::LockedOut(_) => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            AppError::Internal(_) => {
                tracing::error!("internal error: {}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
            AppError::Anyhow(e) => {
                tracing::error!("internal error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
            AppError::Db(e) => {
                tracing::error!("database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
            AppError::Json(e) => {
                tracing::error!("json error: {}", e);
                (StatusCode::BAD_REQUEST, format!("invalid json: {}", e))
            }
        };

        let body = serde_json::json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}
