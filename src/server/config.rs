use axum::extract::State;
use axum::Json;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
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
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(json!({ "config": read_config(&app.data_dir) })))
}

/// PUT /api/config/instance-name — set the instance name
pub async fn set_instance_name(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name = body
        .get("instanceName")
        .and_then(|v| v.as_str())
        .unwrap_or("abot");

    let mut config = read_config(&app.data_dir);
    config["instanceName"] = json!(name);
    write_config(&app.data_dir, &config)?;

    Ok(Json(json!({ "instanceName": name })))
}

/// PUT /api/config/bundle-dir — set the bundle directory path
pub async fn set_bundle_dir(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let dir = body
        .get("bundleDir")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing bundleDir".into()))?;

    let mut config = read_config(&app.data_dir);
    config["bundleDir"] = json!(dir);
    write_config(&app.data_dir, &config)?;

    Ok(Json(json!({ "bundleDir": dir })))
}
