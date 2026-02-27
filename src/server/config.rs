use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::auth::middleware;
use crate::error::AppError;
use crate::server::AppState;

/// GET /api/config — get instance configuration
pub async fn get_config(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;

    let path = app.data_dir.join("config.json");
    let config = match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or(json!({})),
        Err(_) => json!({}),
    };

    Ok(Json(config))
}

/// PUT /api/config/instance-name — set the instance name
pub async fn set_instance_name(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;

    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("abot");

    let path = app.data_dir.join("config.json");
    let mut config: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or(json!({})),
        Err(_) => json!({}),
    };

    config["instanceName"] = json!(name);

    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    std::fs::write(&path, json).map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}
