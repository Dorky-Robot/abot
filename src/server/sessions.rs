use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::auth::middleware;
use crate::error::AppError;
use crate::server::AppState;

/// GET /sessions — list all sessions
pub async fn list_sessions(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;

    let resp = app
        .daemon_client
        .rpc(json!({ "type": "list-sessions" }))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let sessions = resp.get("sessions").cloned().unwrap_or(json!([]));

    Ok(Json(sessions))
}

/// POST /sessions — create a new session
pub async fn create_session(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    middleware::require_csrf(&app, &addr, &headers)?;

    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let resp = app
        .daemon_client
        .rpc(json!({
            "type": "create-session",
            "name": name,
            "cols": 120,
            "rows": 40,
        }))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        let session_name = resp.get("name").and_then(|v| v.as_str()).unwrap_or(&name);
        Ok(Json(json!({ "name": session_name })))
    }
}

/// GET /sessions/:name — get session info
pub async fn get_session(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;

    let resp = app
        .daemon_client
        .rpc(json!({ "type": "get-session", "name": name }))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        if error.contains("not found") {
            Err(AppError::NotFound)
        } else {
            Err(AppError::BadRequest(error.to_string()))
        }
    } else if let Some(session) = resp.get("session") {
        Ok(Json(session.clone()))
    } else {
        Err(AppError::NotFound)
    }
}

/// PUT /sessions/:name — rename a session
pub async fn rename_session(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(old_name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    middleware::require_csrf(&app, &addr, &headers)?;

    let new_name = body
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing name".into()))?;

    let resp = app
        .daemon_client
        .rpc(json!({
            "type": "rename-session",
            "oldName": old_name,
            "newName": new_name,
        }))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        Ok(Json(json!({ "oldName": old_name, "newName": new_name })))
    }
}

/// DELETE /sessions/:name — delete a session
pub async fn delete_session(
    State(app): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    middleware::require_auth(&app, &addr, &headers)?;
    middleware::require_csrf(&app, &addr, &headers)?;

    let resp = app
        .daemon_client
        .rpc(json!({
            "type": "delete-session",
            "name": name,
        }))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        Ok(Json(json!({ "name": name })))
    }
}
