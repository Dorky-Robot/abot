//! REST endpoints for kubo management.

use axum::extract::{Path, State};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::daemon::ipc::DaemonRequest;
use crate::error::AppError;
use crate::server::AppState;

/// GET /kubos — list all kubos
pub async fn list_kubos(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::ListKubos { id: String::new() })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let kubos = resp.get("kubos").cloned().unwrap_or(json!([]));
    Ok(Json(kubos))
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

    let resp = app
        .daemon_client
        .rpc(DaemonRequest::CreateKubo {
            id: String::new(),
            name: name.clone(),
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        let path = resp
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(Json(json!({ "name": name, "path": path })))
    }
}

/// POST /kubos/:name/stop — stop a kubo container
pub async fn stop_kubo(
    _csrf: CsrfVerified,
    State(app): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let resp = app
        .daemon_client
        .rpc(DaemonRequest::StopKubo {
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

/// POST /kubos/:name/abots — add an abot to a kubo
///
/// Body: `{ "abot": "name", "createSession": true, "cols": 120, "rows": 40, "env": {} }`
/// When `createSession` is true, also creates a terminal session and returns its name.
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

    let resp = app
        .daemon_client
        .rpc(DaemonRequest::AddAbotToKubo {
            id: String::new(),
            kubo: kubo_name.clone(),
            abot: abot_name.clone(),
            create_session,
            cols,
            rows,
            env,
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(error) = resp.get("error").and_then(|v| v.as_str()) {
        Err(AppError::BadRequest(error.to_string()))
    } else {
        let session = resp
            .get("session")
            .and_then(|v| v.as_str())
            .map(String::from);
        let mut result = json!({ "kubo": kubo_name, "abot": abot_name });
        if let Some(s) = session {
            result["session"] = json!(s);
        }
        Ok(Json(result))
    }
}
