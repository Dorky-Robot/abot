use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::error::AppError;
use crate::server::AppState;

#[derive(Deserialize)]
pub struct KuboBody {
    pub kubo: String,
}

/// GET /abots — list known abots
pub async fn list_abots(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let abots = app.engine.list_abots().await;
    Ok(Json(json!({ "abots": abots })))
}

/// GET /abots/{name} — abot detail (git info)
pub async fn get_abot(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let abot = app
        .engine
        .get_abot_info(&name)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!(abot)))
}

/// DELETE /abots/{name} — remove from known list
pub async fn remove_abot(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine
        .remove_known_abot(&name)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(json!({ "removed": name })))
}

/// POST /abots/{name}/dismiss — remove worktree but keep branch as past variant
pub async fn dismiss_variant(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<KuboBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine
        .dismiss_variant(&name, &body.kubo)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "dismissed": name, "kubo": body.kubo })))
}

/// POST /abots/{name}/integrate — merge a kubo variant into the default branch
pub async fn integrate_variant(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<KuboBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine
        .integrate_variant(&name, &body.kubo)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "integrated": name, "kubo": body.kubo })))
}

/// POST /abots/{name}/discard — delete a kubo variant branch
pub async fn discard_variant(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<KuboBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine
        .discard_variant(&name, &body.kubo)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "discarded": name, "kubo": body.kubo })))
}
