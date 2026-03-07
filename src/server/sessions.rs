use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::engine::{DEFAULT_COLS, DEFAULT_ROWS};
use crate::error::AppError;
use crate::server::anthropic_oauth;
use crate::server::AppState;

#[derive(Deserialize)]
pub(crate) struct CreateSessionBody {
    name: String,
    kubo: String,
}

#[derive(Deserialize)]
pub(crate) struct RenameBody {
    name: String,
}

#[derive(Deserialize)]
pub(crate) struct ApiKeyBody {
    api_key: String,
}

#[derive(Deserialize)]
pub(crate) struct OpenBundleBody {
    path: String,
    kubo: String,
}

#[derive(Deserialize)]
pub(crate) struct SaveAsBody {
    path: String,
}

#[derive(Deserialize)]
pub(crate) struct CloseBody {
    #[serde(default)]
    save: bool,
}

/// GET /sessions — list all sessions
pub async fn list_sessions(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let sessions = app.engine.list_sessions().await;
    Ok(Json(json!(sessions)))
}

/// POST /sessions — create a new session
pub async fn create_session(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<CreateSessionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let session_name = app
        .engine
        .create_session(
            body.name,
            DEFAULT_COLS,
            DEFAULT_ROWS,
            HashMap::new(),
            body.kubo,
        )
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "name": session_name })))
}

/// GET /sessions/:name — get session info
pub async fn get_session(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let session = app
        .engine
        .get_session(&name)
        .await
        .map_err(AppError::from)?;
    Ok(Json(json!(session)))
}

/// PUT /sessions/:name — rename a session
pub async fn rename_session(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(old_name): Path<String>,
    Json(body): Json<RenameBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let new_name = &body.name;

    app.engine
        .rename_session(&old_name, new_name)
        .await
        .map_err(AppError::from)?;

    // Update ClientTracker so WebSocket relay continues routing output
    app.stream_clients
        .rename_attached_session(&old_name, new_name)
        .await;

    Ok(Json(json!({ "oldName": old_name, "newName": new_name })))
}

/// DELETE /sessions/:name — delete a session
pub async fn delete_session(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    app.engine
        .delete_session(&name)
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "name": name })))
}

/// POST /sessions/:name/credentials — save credentials for one session
pub async fn set_session_credentials(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<ApiKeyBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let key = body.api_key.trim().to_string();

    if key.is_empty() {
        return Err(AppError::BadRequest("api_key cannot be empty".into()));
    }

    let env = anthropic_oauth::build_env_map(Some(&key));

    // Convert Option<String> values to the format update_session_env expects
    let session_env: HashMap<String, Option<String>> = env.into_iter().collect();

    app.engine
        .update_session_env(&name, session_env)
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "session": name, "status": "connected" })))
}

/// GET /sessions/:name/credentials/status — check if session has credentials
pub async fn session_credentials_status(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let session = app
        .engine
        .get_session(&name)
        .await
        .map_err(AppError::from)?;

    let status = if session.env_keys > 0 {
        "connected"
    } else {
        "disconnected"
    };

    Ok(Json(json!({ "session": name, "status": status })))
}

/// DELETE /sessions/:name/credentials — clear session credentials
pub async fn delete_session_credentials(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let env = anthropic_oauth::build_env_map(None);

    let session_env: HashMap<String, Option<String>> = env.into_iter().collect();

    app.engine
        .update_session_env(&name, session_env)
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "session": name, "status": "disconnected" })))
}

/// POST /sessions/open — open a .abot bundle as a new session
pub async fn open_bundle(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<OpenBundleBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (name, bundle_path) = app
        .engine
        .open_bundle(&body.path, DEFAULT_COLS, DEFAULT_ROWS, &body.kubo)
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "name": name, "path": bundle_path })))
}

/// POST /sessions/:name/save — save session to its tracked bundle path
pub async fn save_session(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = app
        .engine
        .save_session(&name)
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "session": name, "path": path })))
}

/// POST /sessions/:name/save-as — save session to a new bundle path
pub async fn save_session_as(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<SaveAsBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let saved_path = app
        .engine
        .save_session_as(&name, &body.path)
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "session": name, "path": saved_path })))
}

/// POST /sessions/:name/close — close session (optionally save first)
pub async fn close_session(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<CloseBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let save = body.save;

    app.engine
        .close_session(&name, save)
        .await
        .map_err(AppError::from)?;

    Ok(Json(json!({ "session": name })))
}
