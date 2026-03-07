//! Known abots registry — tracks which abots the user has created/employed.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A known abot entry in `abots.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownAbot {
    pub name: String,
    pub added_at: String,
}

/// Detail info for a single abot (git state + kubo employment).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbotDetail {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub default_branch: String,
    pub kubo_branches: Vec<KuboBranch>,
    pub git_status: String,
    /// When this abot was added to the known list (set by engine, not by get_abot_detail).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
}

/// A kubo branch in an abot's git repo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KuboBranch {
    pub kubo_name: String,
    pub branch: String,
    pub has_worktree: bool,
    /// Whether there's a live session for this abot@kubo (set by engine).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_session: Option<bool>,
}

/// Read the known abots list from `{data_dir}/abots.json`.
pub fn read_known_abots(data_dir: &Path) -> Vec<KnownAbot> {
    let path = data_dir.join("abots.json");
    match super::bundle::read_json(&path) {
        Ok(val) => {
            if let Some(arr) = val.get("abots").and_then(|v| v.as_array()) {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<KnownAbot>(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        }
        Err(_) => Vec::new(),
    }
}

/// Write the known abots list to `{data_dir}/abots.json`.
fn write_known_abots(data_dir: &Path, abots: &[KnownAbot]) -> Result<()> {
    let val = serde_json::json!({ "abots": abots });
    super::bundle::write_json(&data_dir.join("abots.json"), &val)
}

/// Add an abot to the known list (no-op if already present).
pub fn add_known_abot(data_dir: &Path, name: &str) {
    let mut abots = read_known_abots(data_dir);
    if abots.iter().any(|a| a.name == name) {
        return;
    }
    abots.push(KnownAbot {
        name: name.to_string(),
        added_at: chrono::Utc::now().to_rfc3339(),
    });
    let _ = write_known_abots(data_dir, &abots);
}

/// Remove an abot from the known list.
pub fn remove_known_abot(data_dir: &Path, name: &str) {
    let mut abots = read_known_abots(data_dir);
    abots.retain(|a| a.name != name);
    let _ = write_known_abots(data_dir, &abots);
}

/// Sync known abots with all kubo manifests (union). Called on startup.
pub fn sync_known_abots(data_dir: &Path) {
    let kubos_dir = super::bundle::resolve_kubos_dir(data_dir);
    let mut known = read_known_abots(data_dir);
    let known_names: std::collections::HashSet<String> =
        known.iter().map(|a| a.name.clone()).collect();

    if let Ok(entries) = std::fs::read_dir(&kubos_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("kubo"))
            {
                if let Ok(manifest) = super::kubo::Kubo::read_manifest(&path) {
                    for abot_name in &manifest.abots {
                        if !known_names.contains(abot_name) {
                            known.push(KnownAbot {
                                name: abot_name.clone(),
                                added_at: chrono::Utc::now().to_rfc3339(),
                            });
                        }
                    }
                }
            }
        }
    }

    let _ = write_known_abots(data_dir, &known);
}

/// Get detailed info for a single abot (git branches, worktrees, kubo employment).
pub fn get_abot_detail(data_dir: &Path, name: &str) -> Result<AbotDetail> {
    let abots_dir = super::bundle::resolve_abots_dir(data_dir);
    let abot_path = abots_dir.join(format!("{name}.abot"));

    if !abot_path.exists() {
        anyhow::bail!("abot '{}' not found", name);
    }

    // Read manifest for created_at
    let created_at = super::bundle::read_json(&abot_path.join("manifest.json"))
        .ok()
        .and_then(|m| {
            m.get("created_at")
                .and_then(|v| v.as_str())
                .map(String::from)
        });

    // Git info
    let default_branch = if abot_path.join(".git").exists() {
        super::git_ops::run_git(&abot_path, &["rev-parse", "--abbrev-ref", "HEAD"])
            .unwrap_or_else(|_| "main".to_string())
            .trim()
            .to_string()
    } else {
        "main".to_string()
    };

    let git_status = if abot_path.join(".git").exists() {
        super::git_ops::run_git(&abot_path, &["status", "--short"])
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        String::new()
    };

    // Kubo branches
    let kubo_branches = if abot_path.join(".git").exists() {
        let branches_output = super::git_ops::run_git(&abot_path, &["branch", "--list", "kubo/*"])
            .unwrap_or_default();
        let worktrees_output =
            super::git_ops::run_git(&abot_path, &["worktree", "list", "--porcelain"])
                .unwrap_or_default();

        // Parse worktree paths to know which branches have active worktrees
        let worktree_branches: std::collections::HashSet<String> = worktrees_output
            .split("\n\n")
            .filter_map(|block| {
                block.lines().find(|l| l.starts_with("branch ")).map(|l| {
                    l.strip_prefix("branch refs/heads/")
                        .unwrap_or("")
                        .to_string()
                })
            })
            .collect();

        branches_output
            .lines()
            .map(|l| {
                l.trim()
                    .trim_start_matches("* ")
                    .trim_start_matches("+ ")
                    .to_string()
            })
            .filter(|b| b.starts_with("kubo/"))
            .map(|branch| {
                let kubo_name = branch.strip_prefix("kubo/").unwrap_or(&branch).to_string();
                KuboBranch {
                    kubo_name,
                    has_worktree: worktree_branches.contains(&branch),
                    branch,
                    has_session: None,
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(AbotDetail {
        name: name.to_string(),
        path: abot_path.to_string_lossy().to_string(),
        created_at,
        default_branch,
        kubo_branches,
        git_status,
        added_at: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_abots_crud() {
        let dir = std::env::temp_dir().join("abot-known-abots-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Initially empty
        let abots = read_known_abots(&dir);
        assert!(abots.is_empty());

        // Add one
        add_known_abot(&dir, "alice");
        let abots = read_known_abots(&dir);
        assert_eq!(abots.len(), 1);
        assert_eq!(abots[0].name, "alice");

        // Duplicate is a no-op
        add_known_abot(&dir, "alice");
        assert_eq!(read_known_abots(&dir).len(), 1);

        // Add another
        add_known_abot(&dir, "bob");
        assert_eq!(read_known_abots(&dir).len(), 2);

        // Remove
        remove_known_abot(&dir, "alice");
        let abots = read_known_abots(&dir);
        assert_eq!(abots.len(), 1);
        assert_eq!(abots[0].name, "bob");

        // Remove non-existent is a no-op
        remove_known_abot(&dir, "charlie");
        assert_eq!(read_known_abots(&dir).len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_get_abot_detail_not_found() {
        let dir = std::env::temp_dir().join("abot-detail-notfound-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let result = get_abot_detail(&dir, "nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_sync_known_abots_with_kubos() {
        let dir = std::env::temp_dir().join("abot-sync-known-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Create a kubo with some abots in its manifest
        let kubos_dir = dir.join("kubos");
        crate::engine::kubo::Kubo::ensure_kubo_dir(&kubos_dir, "test-kubo").unwrap();
        let kubo_path = kubos_dir.join("test-kubo.kubo");
        let mut manifest = crate::engine::kubo::Kubo::read_manifest(&kubo_path).unwrap();
        manifest.abots = vec!["alice".to_string(), "bob".to_string()];
        crate::engine::kubo::Kubo::write_manifest(&kubo_path, &manifest).unwrap();

        // Pre-add alice to known
        add_known_abot(&dir, "alice");
        assert_eq!(read_known_abots(&dir).len(), 1);

        // Sync should add bob
        sync_known_abots(&dir);
        let known = read_known_abots(&dir);
        assert_eq!(known.len(), 2);
        assert!(known.iter().any(|a| a.name == "alice"));
        assert!(known.iter().any(|a| a.name == "bob"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
