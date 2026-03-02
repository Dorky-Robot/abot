use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::daemon::ipc::DaemonRequest;
use crate::error::AppError;
use crate::server::AppState;

/// GET /sessions — list all sessions
pub async fn list_sessions(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::ListSessions { id: String::new() })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let sessions = resp.get("sessions").cloned().unwrap_or(json!([]));

    Ok(Json(sessions))
}

/// POST /sessions — create a new session
pub async fn create_session(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let resp = app
        .daemon_client
        .rpc(DaemonRequest::CreateSession {
            id: String::new(),
            name: name.clone(),
            cols: 120,
            rows: 40,
        })
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
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::GetSession {
            id: String::new(),
            name: name.clone(),
        })
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
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(old_name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let new_name = body
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing name".into()))?;

    let resp = app
        .daemon_client
        .rpc(DaemonRequest::RenameSession {
            id: String::new(),
            old_name: old_name.clone(),
            new_name: new_name.to_string(),
        })
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
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::DeleteSession {
            id: String::new(),
            name: name.clone(),
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        Ok(Json(json!({ "name": name })))
    }
}
