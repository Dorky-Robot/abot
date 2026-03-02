use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::daemon::ipc::DaemonRequest;
use crate::error::AppError;
use crate::server::anthropic_oauth;
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
            env: HashMap::new(),
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

    // Build env map using the same logic as global credentials
    let env_map = anthropic_oauth::build_env_map(Some(&key));
    // Convert Option<String> map to HashMap<String, Option<String>> for IPC
    let env: HashMap<String, Option<String>> = env_map;

    let resp = app
        .daemon_client
        .rpc(DaemonRequest::UpdateSessionEnv {
            id: String::new(),
            session: name.clone(),
            env,
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        Ok(Json(json!({ "session": name, "status": "connected" })))
    }
}

/// GET /sessions/:name/credentials/status — check if session has credentials
pub async fn session_credentials_status(
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
            return Err(AppError::NotFound);
        }
        return Err(AppError::BadRequest(error.to_string()));
    }

    let env_keys = resp
        .get("session")
        .and_then(|s| s.get("envKeys"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

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
    // Clear all credential env vars
    let env = anthropic_oauth::build_env_map(None);

    let resp = app
        .daemon_client
        .rpc(DaemonRequest::UpdateSessionEnv {
            id: String::new(),
            session: name.clone(),
            env,
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        Ok(Json(json!({ "session": name, "status": "disconnected" })))
    }
}

/// POST /sessions/:name/export — export session as .abot bundle
pub async fn export_session(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Get export path from body, or use default bundle dir
    let path = body
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            format!("{home}/.abot/bundles")
        });

    let resp = app
        .daemon_client
        .rpc(DaemonRequest::ExportSession {
            id: String::new(),
            session: name.clone(),
            path,
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        let bundle_path = resp
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(Json(json!({ "session": name, "path": bundle_path })))
    }
}

/// POST /sessions/import — import a .abot bundle
pub async fn import_session(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = body
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::BadRequest("missing path".into()))?
        .to_string();

    let resp = app
        .daemon_client
        .rpc(DaemonRequest::ImportSession {
            id: String::new(),
            path,
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        let name = resp
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(Json(json!({ "name": name })))
    }
}
