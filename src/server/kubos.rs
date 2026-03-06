//! REST endpoints for kubo management.

use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::error::AppError;
use crate::server::AppState;

/// GET /kubos — list all kubos
pub async fn list_kubos(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let kubos = app.engine.list_kubos().await;
    Ok(Json(json!(kubos)))
}

/// POST /kubos — create a new kubo
pub async fn create_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing name".into()))?
        .to_string();

    let path = app
        .engine
        .create_kubo(&name)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "name": name, "path": path })))
}

/// POST /kubos/:name/start — start a kubo container
pub async fn start_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine
        .start_kubo(&name)
        .await
        .map_err(|e| AppError::BadRequest(format!("failed to start kubo: {}", e)))?;

    Ok(Json(json!({ "name": name })))
}

/// POST /kubos/:name/stop — stop a kubo container
pub async fn stop_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine
        .stop_kubo(&name)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "name": name })))
}

/// POST /kubos/open — open a kubo from a path on disk
pub async fn open_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = body
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing path".into()))?
        .to_string();

    let name = app
        .engine
        .open_kubo(&path)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "name": name, "path": path })))
}

/// DELETE /kubos/:name/abots/:abot — remove an abot from a kubo
pub async fn remove_abot_from_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path((kubo_name, abot_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine
        .remove_abot_from_kubo(&kubo_name, &abot_name)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "kubo": kubo_name, "abot": abot_name })))
}

/// POST /kubos/:name/abots — add an abot to a kubo
pub async fn add_abot_to_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(kubo_name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let abot_name = body
        .get("abot")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing abot".into()))?
        .to_string();

    let create_session = body
        .get("createSession")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let cols = body.get("cols").and_then(|v| v.as_u64()).unwrap_or(120) as u16;
    let rows = body.get("rows").and_then(|v| v.as_u64()).unwrap_or(40) as u16;

    let env = body
        .get("env")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect::<std::collections::HashMap<String, String>>()
        })
        .unwrap_or_default();

    let session = app
        .engine
        .add_abot_to_kubo(&kubo_name, &abot_name, create_session, cols, rows, env)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let mut result = json!({ "kubo": kubo_name, "abot": abot_name });
    if let Some(s) = session {
        result["session"] = json!(s);
    }
    Ok(Json(result))
}
