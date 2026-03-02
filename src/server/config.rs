use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::json;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use crate::auth::middleware;
use crate::error::AppError;
use crate::server::AppState;

/// Read config.json from the data directory, returning {} if missing/invalid
fn read_config(data_dir: &Path) -> serde_json::Value {
    let path = data_dir.join("config.json");
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or(json!({})),
        Err(_) => json!({}),
    }
}

/// Write config.json to the data directory
fn write_config(data_dir: &Path, config: &serde_json::Value) -> Result<(), AppError> {
    let path = data_dir.join("config.json");
    let json =
        serde_json::to_string_pretty(config).map_err(|e| AppError::Internal(e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(())
}

/// GET /api/config — get instance configuration
pub async fn get_config(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    Ok(Json(json!({ "config": read_config(&app.data_dir) })))
}

/// PUT /api/config/instance-name — set the instance name
pub async fn set_instance_name(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    middleware::require_csrf(&app, &addr, &headers)?;

    let name = body
        .get("instanceName")
        .and_then(|v| v.as_str())
        .unwrap_or("abot");

    let mut config = read_config(&app.data_dir);
    config["instanceName"] = json!(name);
    write_config(&app.data_dir, &config)?;

    Ok(Json(json!({ "instanceName": name })))
}
