use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::auth::middleware;
use crate::error::AppError;
use crate::server::AppState;

/// GET /shortcuts — get user shortcuts
pub async fn get_shortcuts(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;

    let path = app.data_dir.join("shortcuts.json");
    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            let shortcuts: serde_json::Value =
                serde_json::from_str(&contents).unwrap_or(serde_json::json!([]));
            Ok(Json(shortcuts))
        }
        Err(_) => Ok(Json(serde_json::json!([]))),
    }
}

/// PUT /shortcuts — save user shortcuts
pub async fn set_shortcuts(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(shortcuts): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;

    let path = app.data_dir.join("shortcuts.json");
    let json = serde_json::to_string_pretty(&shortcuts)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    std::fs::write(&path, json).map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}
