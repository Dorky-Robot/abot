//! REST endpoints for kubo management.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::error::AppError;
use crate::server::AppState;

#[derive(Deserialize)]
pub(crate) struct CreateKuboBody {
    name: String,
}

#[derive(Deserialize)]
pub(crate) struct OpenKuboBody {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AddAbotBody {
    abot: String,
    #[serde(default)]
    create_session: bool,
    #[serde(default = "default_cols")]
    cols: u16,
    #[serde(default = "default_rows")]
    rows: u16,
    #[serde(default)]
    env: std::collections::HashMap<String, String>,
}

fn default_cols() -> u16 {
    crate::engine::DEFAULT_COLS
}
fn default_rows() -> u16 {
    crate::engine::DEFAULT_ROWS
}

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
    Json(body): Json<CreateKuboBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = app
        .engine
        .create_kubo(&body.name)
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "name": body.name, "path": path })))
}

/// POST /kubos/:name/start — start a kubo container
pub async fn start_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine.start_kubo(&name).await.map_err(AppError::from)?;

    Ok(Json(json!({ "name": name })))
}

/// POST /kubos/:name/stop — stop a kubo container
pub async fn stop_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine.stop_kubo(&name).await.map_err(AppError::from)?;

    Ok(Json(json!({ "name": name })))
}

/// POST /kubos/open — open a kubo from a path on disk
pub async fn open_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<OpenKuboBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name = app
        .engine
        .open_kubo(&body.path)
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "name": name, "path": body.path })))
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
        .map_err(AppError::from)?;

    Ok(Json(json!({ "kubo": kubo_name, "abot": abot_name })))
}

/// POST /kubos/:name/abots — add an abot to a kubo
pub async fn add_abot_to_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(kubo_name): Path<String>,
    Json(body): Json<AddAbotBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let session = app
        .engine
        .add_abot_to_kubo(
            &kubo_name,
            &body.abot,
            body.create_session,
            body.cols,
            body.rows,
            body.env,
        )
        .await
        .map_err(AppError::from)?;

    let mut result = json!({ "kubo": kubo_name, "abot": body.abot });
    if let Some(s) = session {
        result["session"] = json!(s);
    }
    Ok(Json(result))
}
