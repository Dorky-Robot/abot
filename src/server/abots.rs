use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::daemon::ipc::DaemonRequest;
use crate::error::AppError;
use crate::server::AppState;

/// GET /abots — list known abots (lightweight)
pub async fn list_abots(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::ListAbots { id: String::new() })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::Internal(error.to_string()))
    } else {
        let abots = resp.get("abots").cloned().unwrap_or(json!([]));
        Ok(Json(json!({ "abots": abots })))
    }
}

/// GET /abots/{name} — abot detail (git info)
pub async fn get_abot(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::GetAbotInfo {
            id: String::new(),
            abot: name,
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        let abot = resp.get("abot").cloned().unwrap_or(json!({}));
        Ok(Json(abot))
    }
}

/// DELETE /abots/{name} — remove from known list
pub async fn remove_abot(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::RemoveKnownAbot {
            id: String::new(),
            abot: name.clone(),
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::Internal(error.to_string()))
    } else {
        Ok(Json(json!({ "removed": name })))
    }
}
