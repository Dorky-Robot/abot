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
        return Err(AppError::BadRequest("path is not a directory".into()));
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
pub async fn pick_directory(_csrf: CsrfVerified) -> Result<Json<serde_json::Value>, AppError> {
    picker_response(native_pick_directory().await)
}

/// POST /api/pick-file — open the native OS file picker (for .abot bundles).
pub async fn pick_file(_csrf: CsrfVerified) -> Result<Json<serde_json::Value>, AppError> {
    picker_response(native_pick_file().await)
}

/// POST /api/pick-save-location — open the native OS save dialog.
pub async fn pick_save_location(
    _csrf: CsrfVerified,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let default_name = body
        .get("defaultName")
        .and_then(|v| v.as_str())
        .unwrap_or("session.abot");
    // Sanitize filename: only allow safe characters
    let safe_name: String = default_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_' || *c == ' ')
        .collect();
    let safe_name = if safe_name.is_empty() {
        "session.abot"
    } else {
        &safe_name
    };
    picker_response(native_pick_save(safe_name).await)
}

fn picker_response(
    result: anyhow::Result<Option<String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    match result {
        Ok(Some(path)) => Ok(Json(json!({ "path": path }))),
        Ok(None) => Ok(Json(json!({ "path": null }))),
        Err(e) => Err(AppError::Internal(format!("picker failed: {e}"))),
    }
}

// --- Platform-specific native pickers ---

#[cfg(target_os = "macos")]
async fn run_osascript(script: &str) -> anyhow::Result<Option<String>> {
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

#[cfg(target_os = "macos")]
async fn native_pick_directory() -> anyhow::Result<Option<String>> {
    run_osascript("POSIX path of (choose folder)").await
}

#[cfg(target_os = "macos")]
async fn native_pick_file() -> anyhow::Result<Option<String>> {
    run_osascript("POSIX path of (choose file of type {\"com.dorkyrobot.abot.bundle\"})").await
}

#[cfg(target_os = "macos")]
async fn native_pick_save(default_name: &str) -> anyhow::Result<Option<String>> {
    let script = format!(
        "POSIX path of (choose file name default name \"{}\")",
        default_name.replace(['\\', '"'], "")
    );
    run_osascript(&script).await
}

#[cfg(target_os = "linux")]
async fn run_zenity_or_kdialog(
    zenity_args: &[&str],
    kdialog_args: &[&str],
) -> anyhow::Result<Option<String>> {
    let output = tokio::process::Command::new("zenity")
        .args(zenity_args)
        .output()
        .await;
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => {
            tokio::process::Command::new("kdialog")
                .args(kdialog_args)
                .output()
                .await?
        }
    };
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

#[cfg(target_os = "linux")]
async fn native_pick_directory() -> anyhow::Result<Option<String>> {
    run_zenity_or_kdialog(
        &["--file-selection", "--directory"],
        &["--getexistingdirectory", "."],
    )
}

#[cfg(target_os = "linux")]
async fn native_pick_file() -> anyhow::Result<Option<String>> {
    run_zenity_or_kdialog(
        &["--file-selection", "--file-filter=*.abot"],
        &["--getopenfilename", ".", "*.abot"],
    )
}

#[cfg(target_os = "linux")]
async fn native_pick_save(default_name: &str) -> anyhow::Result<Option<String>> {
    let zenity_args = vec!["--file-selection", "--save", "--filename", default_name];
    let zenity_refs: Vec<&str> = zenity_args.iter().map(|s| s.as_ref()).collect();
    run_zenity_or_kdialog(
        &zenity_refs,
        &["--getsavefilename", ".", &format!("*.abot|{default_name}")],
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
async fn native_pick_directory() -> anyhow::Result<Option<String>> {
    anyhow::bail!("native picker not supported on this platform")
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
async fn native_pick_file() -> anyhow::Result<Option<String>> {
    anyhow::bail!("native picker not supported on this platform")
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
async fn native_pick_save(_name: &str) -> anyhow::Result<Option<String>> {
    anyhow::bail!("native picker not supported on this platform")
}
