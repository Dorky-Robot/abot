use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::{Authenticated, CsrfVerified};
use crate::error::AppError;
use crate::server::AppState;

#[derive(Deserialize)]
pub struct BrowseQuery {
    path: Option<String>,
    show_hidden: Option<bool>,
}

/// GET /api/browse — list directory contents for the file browser.
pub async fn browse_dir(
    _auth: Authenticated,
    State(app): State<Arc<AppState>>,
    Query(params): Query<BrowseQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let show_hidden = params.show_hidden.unwrap_or(false);

    // Default to ~/.abot/bundles
    let raw_path = params
        .path
        .unwrap_or_else(|| app.data_dir.join("bundles").to_string_lossy().to_string());

    // Expand ~ to home directory
    let expanded = if let Some(stripped) = raw_path.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            home.join(stripped.strip_prefix('/').unwrap_or(stripped))
        } else {
            return Err(AppError::Internal("cannot resolve home directory".into()));
        }
    } else {
        std::path::PathBuf::from(&raw_path)
    };

    // Canonicalize to prevent traversal
    let canonical = expanded.canonicalize().map_err(|_| AppError::NotFound)?;

    if !canonical.is_dir() {
        return Err(AppError::BadRequest(format!(
            "not a directory: {}",
            canonical.display()
        )));
    }

    let parent = canonical.parent().map(|p| p.to_string_lossy().to_string());

    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(&canonical)
        .map_err(|e| AppError::Internal(format!("read_dir failed: {e}")))?;

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        // Filter hidden files unless show_hidden
        if !show_hidden && name.starts_with('.') {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let is_dir = metadata.is_dir();
        let size = metadata.len();
        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        entries.push(json!({
            "name": name,
            "isDir": is_dir,
            "size": size,
            "modified": modified,
        }));
    }

    // Sort: directories first, then alphabetical by name
    entries.sort_by(|a, b| {
        let a_dir = a["isDir"].as_bool().unwrap_or(false);
        let b_dir = b["isDir"].as_bool().unwrap_or(false);
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                let a_name = a["name"].as_str().unwrap_or("");
                let b_name = b["name"].as_str().unwrap_or("");
                a_name.to_lowercase().cmp(&b_name.to_lowercase())
            }
        }
    });

    Ok(Json(json!({
        "path": canonical.to_string_lossy(),
        "parent": parent,
        "entries": entries,
    })))
}

/// POST /api/pick-directory — open the native OS directory picker.
///
/// Uses `osascript` on macOS, `zenity`/`kdialog` on Linux.
/// Returns `{"path": "/selected/dir"}` or `{"path": null}` if cancelled.
pub async fn pick_directory(_csrf: CsrfVerified) -> Result<Json<serde_json::Value>, AppError> {
    let result = native_pick_directory().await;
    match result {
        Ok(Some(path)) => Ok(Json(json!({ "path": path }))),
        Ok(None) => Ok(Json(json!({ "path": null }))),
        Err(e) => Err(AppError::Internal(format!("directory picker failed: {e}"))),
    }
}

/// POST /api/pick-file — open the native OS file picker (for opening .abot bundles).
///
/// Returns `{"path": "/selected/file.abot"}` or `{"path": null}` if cancelled.
pub async fn pick_file(_csrf: CsrfVerified) -> Result<Json<serde_json::Value>, AppError> {
    let result = run_osascript("POSIX path of (choose file of type {\"abot\"})").await;
    match result {
        Ok(Some(path)) => Ok(Json(json!({ "path": path }))),
        Ok(None) => Ok(Json(json!({ "path": null }))),
        Err(e) => Err(AppError::Internal(format!("file picker failed: {e}"))),
    }
}

/// POST /api/pick-save-location — open the native OS save dialog.
///
/// Accepts optional `{"defaultName": "filename.abot"}`.
/// Returns `{"path": "/chosen/path.abot"}` or `{"path": null}` if cancelled.
pub async fn pick_save_location(
    _csrf: CsrfVerified,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let default_name = body
        .get("defaultName")
        .and_then(|v| v.as_str())
        .unwrap_or("session.abot");

    let script = format!(
        "POSIX path of (choose file name default name \"{}\")",
        default_name.replace('"', "\\\"")
    );
    let result = run_osascript(&script).await;
    match result {
        Ok(Some(path)) => Ok(Json(json!({ "path": path }))),
        Ok(None) => Ok(Json(json!({ "path": null }))),
        Err(e) => Err(AppError::Internal(format!("save picker failed: {e}"))),
    }
}

/// Run an osascript command (macOS) or equivalent (Linux) and return the trimmed output.
async fn run_osascript(script: &str) -> anyhow::Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .await?;

        if !output.status.success() {
            return Ok(None);
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let path = path.strip_suffix('/').unwrap_or(&path).to_string();
        Ok(Some(path))
    }

    #[cfg(target_os = "linux")]
    {
        let _ = script;
        // Try zenity first, fall back to kdialog
        let output = tokio::process::Command::new("zenity")
            .args(["--file-selection"])
            .output()
            .await;

        let output = match output {
            Ok(o) => o,
            Err(_) => {
                tokio::process::Command::new("kdialog")
                    .args(["--getopenfilename", "."])
                    .output()
                    .await?
            }
        };

        if !output.status.success() {
            return Ok(None);
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Some(path))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = script;
        anyhow::bail!("native file picker not supported on this platform")
    }
}

async fn native_pick_directory() -> anyhow::Result<Option<String>> {
    run_osascript("POSIX path of (choose folder)").await
}
