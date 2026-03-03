use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::auth::middleware::Authenticated;
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
