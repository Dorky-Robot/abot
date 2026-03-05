use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::daemon::ipc::DaemonRequest;
use crate::error::AppError;
use crate::server::AppState;

#[derive(Deserialize)]
pub struct KuboBody {
    pub kubo: String,
}

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

/// POST /abots/{name}/dismiss — remove worktree but keep branch as past variant
pub async fn dismiss_variant(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<KuboBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::DismissVariant {
            id: String::new(),
            abot: name.clone(),
            kubo: body.kubo.clone(),
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        Ok(Json(json!({ "dismissed": name, "kubo": body.kubo })))
    }
}

/// POST /abots/{name}/integrate — merge a kubo variant into the default branch
pub async fn integrate_variant(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<KuboBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::IntegrateVariant {
            id: String::new(),
            abot: name.clone(),
            kubo: body.kubo.clone(),
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        Ok(Json(json!({ "integrated": name, "kubo": body.kubo })))
    }
}

/// POST /abots/{name}/discard — delete a kubo variant branch
pub async fn discard_variant(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<KuboBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::DiscardVariant {
            id: String::new(),
            abot: name.clone(),
            kubo: body.kubo.clone(),
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        Ok(Json(json!({ "discarded": name, "kubo": body.kubo })))
    }
}
