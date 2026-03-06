use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::error::AppError;
use crate::server::anthropic_oauth;
use crate::server::AppState;

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
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing name".into()))?
        .to_string();

    let kubo = body
        .get("kubo")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing kubo".into()))?
        .to_string();

    let session_name = app
        .engine
        .create_session(name, 120, 40, HashMap::new(), kubo)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "name": session_name })))
}

/// GET /sessions/:name — get session info
pub async fn get_session(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    match app.engine.get_session(&name).await {
        Ok(session) => Ok(Json(session)),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") {
                Err(AppError::NotFound)
            } else {
                Err(AppError::BadRequest(msg))
            }
        }
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

    app.engine
        .rename_session(&old_name, new_name)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

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
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "name": name })))
}

/// POST /sessions/:name/credentials — save credentials for one session
pub async fn set_session_credentials(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let key = body
        .get("api_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing api_key".into()))?
        .trim()
        .to_string();

    if key.is_empty() {
        return Err(AppError::BadRequest("api_key cannot be empty".into()));
    }

    let env = anthropic_oauth::build_env_map(Some(&key));

    // Convert Option<String> values to the format update_session_env expects
    let session_env: HashMap<String, Option<String>> = env.into_iter().collect();

    app.engine
        .update_session_env(&name, session_env)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "session": name, "status": "connected" })))
}

/// GET /sessions/:name/credentials/status — check if session has credentials
pub async fn session_credentials_status(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let session = app.engine.get_session(&name).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") {
            AppError::NotFound
        } else {
            AppError::BadRequest(msg)
        }
    })?;

    let env_keys = session.get("envKeys").and_then(|v| v.as_u64()).unwrap_or(0);

    let status = if env_keys > 0 {
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
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "session": name, "status": "disconnected" })))
}

/// POST /sessions/open — open a .abot bundle as a new session
pub async fn open_bundle(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = body
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing path".into()))?
        .to_string();

    let kubo = body
        .get("kubo")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing kubo".into()))?
        .to_string();

    let (name, bundle_path) = app
        .engine
        .open_bundle(&path, 120, 40, &kubo)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

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
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "session": name, "path": path })))
}

/// POST /sessions/:name/save-as — save session to a new bundle path
pub async fn save_session_as(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = body
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing path".into()))?
        .to_string();

    let saved_path = app
        .engine
        .save_session_as(&name, &path)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "session": name, "path": saved_path })))
}

/// POST /sessions/:name/close — close session (optionally save first)
pub async fn close_session(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let save = body.get("save").and_then(|v| v.as_bool()).unwrap_or(false);

    app.engine
        .close_session(&name, save)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    Ok(Json(json!({ "session": name })))
}
