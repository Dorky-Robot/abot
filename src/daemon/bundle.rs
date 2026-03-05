//! `.abot` bundle save/open — document-style persistence.
//!
//! A `.abot` bundle is a directory containing:
//!   - manifest.json   (name, version, timestamps)
//!   - credentials.json (extracted credential env vars)
//!   - config.json     (shell, resource limits, custom env)
//!   - home/           (bind-mounted as /home/dev in Docker container)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const BUNDLE_VERSION: u32 = 2;
const LEGACY_BUNDLE_VERSION: u32 = 1;
const SESSION_IMAGE: &str = "abot-session";

/// Credential-related env var keys that get stored in credentials.json
const CREDENTIAL_KEYS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "CLAUDE_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
];

/// Result of opening a bundle — enough info to create a session.
#[derive(Debug)]
pub struct OpenedBundle {
    pub name: String,
    pub env: HashMap<String, String>,
    pub path: PathBuf,
}

/// Save a session to a `.abot` bundle directory.
///
/// Writes manifest, credentials, and config metadata files.
/// Preserves `created_at` from an existing manifest; adds/updates `updated_at`.
pub async fn save_bundle(
    bundle_path: &Path,
    name: &str,
    session_env: &HashMap<String, String>,
) -> Result<()> {
    std::fs::create_dir_all(bundle_path)
        .with_context(|| format!("failed to create bundle dir: {}", bundle_path.display()))?;

    // Read existing manifest to preserve created_at
    let manifest_path = bundle_path.join("manifest.json");
    let existing_created_at = if manifest_path.exists() {
        read_json(&manifest_path).ok().and_then(|m| {
            m.get("created_at")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
    } else {
        None
    };

    // 1. Write manifest.json
    let now = chrono::Utc::now().to_rfc3339();
    let manifest = serde_json::json!({
        "version": BUNDLE_VERSION,
        "name": name,
        "created_at": existing_created_at.as_deref().unwrap_or(&now),
        "updated_at": now,
        "image": SESSION_IMAGE,
    });
    write_json(&manifest_path, &manifest)?;

    // 2. Write credentials.json (extract credential keys from session env)
    let mut creds = serde_json::Map::new();
    for key in CREDENTIAL_KEYS {
        if let Some(val) = session_env.get(*key) {
            let json_key = match *key {
                "ANTHROPIC_API_KEY" | "CLAUDE_API_KEY" => "api_key",
                "CLAUDE_CODE_OAUTH_TOKEN" => "claude_token",
                _ => *key,
            };
            creds.insert(json_key.to_string(), serde_json::Value::String(val.clone()));
        }
    }
    write_json(
        &bundle_path.join("credentials.json"),
        &serde_json::Value::Object(creds),
    )?;

    // 3. Write config.json with defaults
    let mut custom_env = serde_json::Map::new();
    for (k, v) in session_env {
        if !CREDENTIAL_KEYS.contains(&k.as_str()) {
            custom_env.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
    }
    let config = serde_json::json!({
        "shell": "/bin/bash",
        "memory_mb": 2048,
        "cpu_percent": 50,
        "env": custom_env,
    });
    write_json(&bundle_path.join("config.json"), &config)?;

    // Set directory permissions to 0o700
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bundle_path, std::fs::Permissions::from_mode(0o700))?;
        let creds_path = bundle_path.join("credentials.json");
        if creds_path.exists() {
            std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600))?;
        }
    }

    Ok(())
}

/// Open a `.abot` bundle directory, returning the session name, env, and path.
pub async fn open_bundle(path: &str) -> Result<OpenedBundle> {
    let bundle_dir = PathBuf::from(path);

    if !bundle_dir.exists() {
        anyhow::bail!("bundle path does not exist: {}", path);
    }

    // 1. Read and validate manifest.json
    let manifest_path = bundle_dir.join("manifest.json");
    let manifest: serde_json::Value =
        read_json(&manifest_path).with_context(|| "failed to read manifest.json")?;

    let version = manifest
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if version != BUNDLE_VERSION && version != LEGACY_BUNDLE_VERSION {
        anyhow::bail!(
            "unsupported bundle version {} (expected {} or {})",
            version,
            LEGACY_BUNDLE_VERSION,
            BUNDLE_VERSION
        );
    }

    // Auto-migrate v1 → v2 (init git repo)
    if version == LEGACY_BUNDLE_VERSION {
        if let Err(e) = git_init_abot(&bundle_dir) {
            tracing::warn!("failed to auto-migrate abot to git: {}", e);
        }
    }

    let name = manifest
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("imported")
        .to_string();

    // 2. Read credentials.json → session env
    let creds_path = bundle_dir.join("credentials.json");
    let mut env = read_credentials(&creds_path);

    // 3. Read config.json → merge custom env
    let config_path = bundle_dir.join("config.json");
    if config_path.exists() {
        let config: serde_json::Value = read_json(&config_path)?;
        if let Some(custom_env) = config.get("env").and_then(|v| v.as_object()) {
            for (k, v) in custom_env {
                if let Some(val) = v.as_str() {
                    env.insert(k.clone(), val.to_string());
                }
            }
        }
    }

    // 4. Ensure home/ directory exists; migrate from legacy filesystem.tar.zst if needed
    let home_dir = bundle_dir.join("home");
    if !home_dir.exists() {
        let snapshot_path = bundle_dir.join("filesystem.tar.zst");
        if snapshot_path.exists() {
            migrate_archive_to_home(&snapshot_path, &home_dir).await?;
        } else {
            std::fs::create_dir_all(&home_dir)
                .with_context(|| "failed to create home/ in bundle")?;
        }
    }

    Ok(OpenedBundle {
        name,
        env,
        path: bundle_dir,
    })
}

/// Read a `credentials.json` file and return env vars for container injection.
///
/// Maps `api_key` → `ANTHROPIC_API_KEY` + `CLAUDE_API_KEY` (if `sk-ant-api` prefix)
///       or `CLAUDE_CODE_OAUTH_TOKEN` (otherwise).
/// Maps `claude_token` → `CLAUDE_CODE_OAUTH_TOKEN`.
/// Returns an empty map if the file is missing or invalid.
pub fn read_credentials(path: &Path) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let creds = match read_json(path) {
        Ok(v) => v,
        Err(_) => return env,
    };
    if let Some(obj) = creds.as_object() {
        if let Some(val) = obj.get("api_key").and_then(|v| v.as_str()) {
            if val.starts_with("sk-ant-api") {
                env.insert("ANTHROPIC_API_KEY".to_string(), val.to_string());
                env.insert("CLAUDE_API_KEY".to_string(), val.to_string());
            } else {
                env.insert("CLAUDE_CODE_OAUTH_TOKEN".to_string(), val.to_string());
            }
        }
        if let Some(val) = obj.get("claude_token").and_then(|v| v.as_str()) {
            env.insert("CLAUDE_CODE_OAUTH_TOKEN".to_string(), val.to_string());
        }
    }
    env
}

pub(crate) fn write_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub(crate) fn read_json(path: &Path) -> Result<serde_json::Value> {
    let contents = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&contents)?;
    Ok(value)
}

/// Save terminal scrollback to a bundle directory.
/// Uses atomic write (tmp + rename) to avoid partial reads.
/// No-op if content is empty.
pub fn save_scrollback(bundle_path: &Path, content: &str) {
    if content.is_empty() {
        return;
    }
    let target = bundle_path.join("scrollback");
    let tmp = bundle_path.join("scrollback.tmp");
    if std::fs::write(&tmp, content.as_bytes()).is_ok() {
        let _ = std::fs::rename(&tmp, &target);
    }
}

/// Load terminal scrollback from a bundle directory.
/// Returns `None` if the file is missing or unreadable.
pub fn load_scrollback(bundle_path: &Path) -> Option<String> {
    let path = bundle_path.join("scrollback");
    std::fs::read(&path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Ensure a bundle's `home/` directory exists, creating the full bundle structure.
/// Returns the path to the `home/` directory.
/// Uses configured abots dir (v2), falling back to `bundles/` if it exists (v1 compat).
/// Used by CreateSession and tests; will be superseded by worktree model for kubo sessions.
#[allow(dead_code)]
pub fn ensure_bundle_home(data_dir: &Path, name: &str) -> Result<PathBuf> {
    super::kubo::validate_name(name)?;

    // Check v2 path first (respects bundleDir config), then legacy v1 path
    let abots_dir = resolve_abots_dir(data_dir);
    let v2_dir = abots_dir.join(format!("{name}.abot"));
    let v1_dir = data_dir.join("bundles").join(format!("{name}.abot"));

    let bundle_dir = if v1_dir.exists() && !v2_dir.exists() {
        // Legacy location still in use
        v1_dir
    } else {
        v2_dir
    };

    let home_dir = bundle_dir.join("home");
    std::fs::create_dir_all(&home_dir)
        .with_context(|| format!("failed to create bundle home: {}", home_dir.display()))?;

    // Init git if not already a repo or worktree (.git can be a file for worktrees)
    if !bundle_dir.join(".git").exists() {
        let _ = git_init_abot(&bundle_dir);
    }

    Ok(home_dir)
}

/// Recursively copy a directory tree from `src` to `dst`.
/// Symlinks are skipped to prevent following links outside the bundle.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("failed to create dir: {}", dst.display()))?;

    for entry in
        std::fs::read_dir(src).with_context(|| format!("failed to read dir: {}", src.display()))?
    {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // Skip symlinks to prevent following links outside the bundle
        if src_path.symlink_metadata()?.file_type().is_symlink() {
            continue;
        }

        if metadata.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "failed to copy {} → {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}

/// Migrate a legacy `filesystem.tar.zst` archive into a `home/` directory.
async fn migrate_archive_to_home(archive_path: &Path, home_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(home_dir)
        .with_context(|| format!("failed to create home dir: {}", home_dir.display()))?;

    let compressed = std::fs::read(archive_path)?;
    let decompressed =
        zstd::decode_all(compressed.as_slice()).with_context(|| "zstd decompression failed")?;

    // Extract tar into home/
    let mut child = tokio::process::Command::new("tar")
        .args(["xf", "-", "-C"])
        .arg(home_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| "failed to spawn tar for migration")?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(&decompressed).await?;
        drop(stdin);
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("archive migration failed: {}", stderr);
    }

    tracing::info!(
        "migrated legacy filesystem.tar.zst → {}",
        home_dir.display()
    );
    Ok(())
}

// ── Git-based abot repo ──────────────────────────────────────────

/// Default .gitignore for an abot repo.
const ABOT_GITIGNORE: &str = "\
credentials.json
scrollback
scrollback.tmp
home/.cache/
home/.local/share/
home/.claude/
home/.bash_history
home/.zsh_history
home/.node_repl_history
home/.python_history
";

/// Initialize an abot directory as a git repo with .gitignore and initial commit.
/// No-op if `.git` already exists (as directory for regular repos, or file for worktrees).
pub fn git_init_abot(abot_path: &Path) -> Result<()> {
    if abot_path.join(".git").exists() {
        return Ok(());
    }

    // Write .gitignore
    let gitignore_path = abot_path.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, ABOT_GITIGNORE)
            .with_context(|| "failed to write .gitignore")?;
    }

    // Update manifest version to 2
    let manifest_path = abot_path.join("manifest.json");
    if manifest_path.exists() {
        if let Ok(mut manifest) = read_json(&manifest_path) {
            manifest["version"] = serde_json::Value::Number(BUNDLE_VERSION.into());
            let _ = write_json(&manifest_path, &manifest);
        }
    }

    // git init + initial commit
    run_git(abot_path, &["init"])?;
    run_git(abot_path, &["add", "-A"])?;
    // Check if there's anything to commit
    let status = run_git(abot_path, &["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        run_git(abot_path, &["commit", "-m", "Initial abot snapshot"])?;
    }

    tracing::info!("initialized git repo at {}", abot_path.display());
    Ok(())
}

/// Auto-commit changes in an abot git repo (used by autosave loop).
/// Works for both regular repos (.git dir) and worktrees (.git file).
/// Returns Ok(true) if a commit was made, Ok(false) if nothing to commit.
pub fn auto_commit_abot(abot_path: &Path) -> Result<bool> {
    if !abot_path.join(".git").exists() {
        return Ok(false);
    }

    run_git(abot_path, &["add", "-A"])?;
    let status = run_git(abot_path, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        return Ok(false);
    }

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let msg = format!("autosave {}", now);
    run_git(abot_path, &["commit", "-m", &msg])?;
    Ok(true)
}

/// Migrate v1 `bundles/` directory to v2 `abots/` directory.
/// Also creates a default kubo and adds all existing abots as subdirectories.
pub fn migrate_data_dir(data_dir: &Path) -> Result<()> {
    let bundles_dir = data_dir.join("bundles");
    let abots_dir = resolve_abots_dir(data_dir);
    let kubos_dir = resolve_kubos_dir(data_dir);

    if !bundles_dir.exists() {
        // Nothing to migrate — create dirs
        let _ = std::fs::create_dir_all(&abots_dir);
        let _ = std::fs::create_dir_all(&kubos_dir);
        return Ok(());
    }

    // Already migrated?
    if abots_dir.exists() {
        return Ok(());
    }

    tracing::info!("migrating v1 bundles → v2 abots");

    // Rename bundles/ → abots/
    std::fs::rename(&bundles_dir, &abots_dir)
        .with_context(|| "failed to rename bundles/ to abots/")?;

    // Init git in each abot
    if let Ok(entries) = std::fs::read_dir(&abots_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("abot"))
            {
                if let Err(e) = git_init_abot(&path) {
                    tracing::warn!("failed to git-init migrated abot {}: {}", path.display(), e);
                }
            }
        }
    }

    // Create default kubo with all abots as subdirectories
    let _ = std::fs::create_dir_all(&kubos_dir);
    let default_kubo_path = kubos_dir.join("default.kubo");
    if !default_kubo_path.exists() {
        super::kubo::Kubo::ensure_kubo_dir(&kubos_dir, "default")?;

        // Copy abot dirs into kubo as subdirectories (worktrees set up on demand)
        if let Ok(entries) = std::fs::read_dir(&abots_dir) {
            let mut abot_names = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if let Some(base) = name.strip_suffix(".abot") {
                            // Create abot home dir in kubo
                            let kubo_abot_dir = default_kubo_path.join(base);
                            let _ = std::fs::create_dir_all(kubo_abot_dir.join("home"));
                            abot_names.push(base.to_string());
                        }
                    }
                }
            }

            // Update kubo manifest with abot list
            if let Ok(mut manifest) = super::kubo::Kubo::read_manifest(&default_kubo_path) {
                manifest.abots = abot_names;
                let _ = super::kubo::Kubo::write_manifest(&default_kubo_path, &manifest);
            }
        }
    }

    tracing::info!("migration complete: bundles → abots");
    Ok(())
}

/// Run a git command in the given directory and return stdout.
pub(crate) fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git {:?}", args))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {:?} failed: {}", args, stderr);
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ── Git worktree operations ─────────────────────────────────────

/// Read the data-dir config.json, returning {} on any failure.
fn read_data_config(data_dir: &Path) -> serde_json::Value {
    let config_path = data_dir.join("config.json");
    std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}))
}

/// Resolve the abots directory from config.json `bundleDir`, falling back to `{data_dir}/abots/`.
pub fn resolve_abots_dir(data_dir: &Path) -> PathBuf {
    let config = read_data_config(data_dir);
    if let Some(dir) = config.get("bundleDir").and_then(|v| v.as_str()) {
        let p = PathBuf::from(dir);
        if p.is_absolute() {
            return p;
        }
    }
    data_dir.join("abots")
}

/// Resolve the kubos directory from config.json `kubosDir`, falling back to `{data_dir}/kubos/`.
pub fn resolve_kubos_dir(data_dir: &Path) -> PathBuf {
    let config = read_data_config(data_dir);
    if let Some(dir) = config.get("kubosDir").and_then(|v| v.as_str()) {
        let p = PathBuf::from(dir);
        if p.is_absolute() {
            return p;
        }
    }
    data_dir.join("kubos")
}

/// Create a canonical `.abot` bundle in the abots directory.
/// Returns the path to the created bundle (e.g. `{abots_dir}/{name}.abot/`).
/// Skips if the bundle already exists. Validates name for path and git-ref safety.
pub fn create_canonical_abot(abots_dir: &Path, name: &str) -> Result<PathBuf> {
    super::kubo::validate_name(name)?;

    let bundle_path = abots_dir.join(format!("{name}.abot"));
    if bundle_path.exists() {
        return Ok(bundle_path);
    }

    let home_dir = bundle_path.join("home");
    std::fs::create_dir_all(&home_dir)
        .with_context(|| format!("failed to create canonical abot: {}", bundle_path.display()))?;

    // Write a minimal manifest so the canonical abot is a valid bundle
    let now = chrono::Utc::now().to_rfc3339();
    let manifest = serde_json::json!({
        "version": BUNDLE_VERSION,
        "name": name,
        "created_at": now,
        "updated_at": now,
    });
    write_json(&bundle_path.join("manifest.json"), &manifest)?;

    git_init_abot(&bundle_path)?;

    tracing::info!("created canonical abot at {}", bundle_path.display());
    Ok(bundle_path)
}

/// Add an abot as a git worktree in a kubo directory.
/// Creates a `kubo/<kubo_name>` branch in the canonical abot repo, then
/// runs `git worktree add` to place it at `{kubo_path}/{abot_name}`.
/// Validates names for path and git-ref safety.
pub fn worktree_add_abot(
    canonical_path: &Path,
    kubo_path: &Path,
    abot_name: &str,
    kubo_name: &str,
) -> Result<()> {
    super::kubo::validate_name(abot_name)?;
    super::kubo::validate_name(kubo_name)?;

    let worktree_path = kubo_path.join(abot_name);
    if worktree_path.exists() {
        let git_file = worktree_path.join(".git");
        if git_file.exists() && !git_file.is_dir() {
            // .git file exists — verify the gitdir target is still valid
            if let Ok(content) = std::fs::read_to_string(&git_file) {
                let gitdir = content.trim().strip_prefix("gitdir: ").unwrap_or("");
                if !gitdir.is_empty() && std::path::Path::new(gitdir).exists() {
                    return Ok(()); // Valid worktree, reuse it
                }
            }
            // Stale .git file — remove and recreate
            tracing::warn!(
                "removing stale worktree at {} (gitdir target missing)",
                worktree_path.display()
            );
        } else {
            // Directory exists but isn't a worktree
            tracing::warn!(
                "removing non-worktree directory at {} to set up worktree",
                worktree_path.display()
            );
        }
        std::fs::remove_dir_all(&worktree_path)?;
    }

    if !canonical_path.join(".git").exists() {
        anyhow::bail!(
            "canonical abot is not a git repo: {}",
            canonical_path.display()
        );
    }

    let branch = format!("kubo/{kubo_name}");
    let worktree_str = worktree_path.to_string_lossy().to_string();

    // Create the branch if it doesn't exist (based on current HEAD)
    let branches = run_git(canonical_path, &["branch", "--list", &branch])?;
    if branches.trim().is_empty() {
        run_git(canonical_path, &["branch", &branch])?;
    }

    // Create the worktree
    run_git(canonical_path, &["worktree", "add", &worktree_str, &branch])?;

    tracing::info!(
        "added worktree for '{}' at {} on branch '{}'",
        abot_name,
        worktree_path.display(),
        branch,
    );
    Ok(())
}

/// Remove an abot's worktree from a kubo directory.
/// Currently unused — remove-abot keeps worktrees for resume semantics.
#[allow(dead_code)]
pub fn worktree_remove_abot(
    canonical_path: &Path,
    kubo_path: &Path,
    abot_name: &str,
) -> Result<()> {
    super::kubo::validate_name(abot_name)?;
    let worktree_path = kubo_path.join(abot_name);
    if !worktree_path.exists() {
        return Ok(());
    }

    let worktree_str = worktree_path.to_string_lossy().to_string();
    run_git(
        canonical_path,
        &["worktree", "remove", &worktree_str, "--force"],
    )?;

    // Prune stale worktree metadata so the branch can be re-used
    let _ = run_git(canonical_path, &["worktree", "prune"]);

    tracing::info!(
        "removed worktree for '{}' from {}",
        abot_name,
        kubo_path.display(),
    );
    Ok(())
}

// ── Variant lifecycle ────────────────────────────────────────────

/// Find the filesystem path of a worktree checked out on `kubo_branch`, if any.
fn find_worktree_path(canonical_path: &Path, kubo_branch: &str) -> Option<String> {
    let output = run_git(canonical_path, &["worktree", "list", "--porcelain"]).ok()?;
    let needle = format!("branch refs/heads/{}", kubo_branch);
    for block in output.split("\n\n") {
        if block.lines().any(|l| l == needle) {
            return block
                .lines()
                .find(|l| l.starts_with("worktree "))
                .and_then(|l| l.strip_prefix("worktree "))
                .map(String::from);
        }
    }
    None
}

/// Auto-commit any outstanding changes in a worktree, then remove it.
/// Ensures no work is lost when the worktree is cleaned up.
fn commit_and_remove_worktree(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    if let Some(wt_path) = find_worktree_path(canonical_path, kubo_branch) {
        let wt = std::path::Path::new(&wt_path);
        if let Ok(committed) = auto_commit_abot(wt) {
            if committed {
                tracing::info!("auto-committed changes in worktree '{}'", wt_path);
            }
        }
        let _ = run_git(canonical_path, &["worktree", "remove", &wt_path, "--force"]);
        let _ = run_git(canonical_path, &["worktree", "prune"]);
    }
    Ok(())
}

/// Force-remove a worktree without committing (for discard).
fn force_remove_worktree(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    if let Some(wt_path) = find_worktree_path(canonical_path, kubo_branch) {
        let _ = run_git(canonical_path, &["worktree", "remove", &wt_path, "--force"]);
        let _ = run_git(canonical_path, &["worktree", "prune"]);
    }
    Ok(())
}

/// Integrate a kubo variant into the abot's default branch, then delete the branch.
/// Commits any outstanding worktree changes before merging.
/// Returns Err if the merge has conflicts (merge is aborted in that case).
pub fn integrate_variant(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    commit_and_remove_worktree(canonical_path, kubo_branch)?;

    // Merge the kubo branch into the current (default) branch
    let merge_output = run_git(canonical_path, &["merge", kubo_branch, "--no-edit"]);
    if merge_output.is_err() {
        let _ = run_git(canonical_path, &["merge", "--abort"]);
        anyhow::bail!("merge conflict integrating variant '{}'", kubo_branch);
    }

    // Delete the branch (it's now merged)
    run_git(canonical_path, &["branch", "-d", kubo_branch])?;

    tracing::info!(
        "integrated variant '{}' into {}",
        kubo_branch,
        canonical_path.display()
    );
    Ok(())
}

/// Dismiss a kubo variant — commit outstanding changes, remove worktree, keep branch.
pub fn dismiss_variant(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    commit_and_remove_worktree(canonical_path, kubo_branch)?;

    tracing::info!(
        "dismissed variant '{}' from {}",
        kubo_branch,
        canonical_path.display()
    );
    Ok(())
}

/// Discard a kubo variant — delete the branch and worktree without saving.
pub fn discard_variant(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    force_remove_worktree(canonical_path, kubo_branch)?;

    // Force-delete the branch
    run_git(canonical_path, &["branch", "-D", kubo_branch])?;

    tracing::info!(
        "discarded variant '{}' from {}",
        kubo_branch,
        canonical_path.display()
    );
    Ok(())
}

// ── Known abots registry ────────────────────────────────────────

/// A known abot entry in `abots.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownAbot {
    pub name: String,
    pub added_at: String,
}

/// Detail info for a single abot (git state + kubo employment).
#[derive(Debug, Clone, Serialize)]
pub struct AbotDetail {
    pub name: String,
    pub path: String,
    pub created_at: Option<String>,
    pub default_branch: String,
    pub kubo_branches: Vec<KuboBranch>,
    pub git_status: String,
}

/// A kubo branch in an abot's git repo.
#[derive(Debug, Clone, Serialize)]
pub struct KuboBranch {
    pub kubo_name: String,
    pub branch: String,
    pub has_worktree: bool,
}

/// Read the known abots list from `{data_dir}/abots.json`.
pub fn read_known_abots(data_dir: &Path) -> Vec<KnownAbot> {
    let path = data_dir.join("abots.json");
    match read_json(&path) {
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
    write_json(&data_dir.join("abots.json"), &val)
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
    let kubos_dir = resolve_kubos_dir(data_dir);
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
    let abots_dir = resolve_abots_dir(data_dir);
    let abot_path = abots_dir.join(format!("{name}.abot"));

    if !abot_path.exists() {
        anyhow::bail!("abot '{}' not found", name);
    }

    // Read manifest for created_at
    let created_at = read_json(&abot_path.join("manifest.json"))
        .ok()
        .and_then(|m| {
            m.get("created_at")
                .and_then(|v| v.as_str())
                .map(String::from)
        });

    // Git info
    let default_branch = if abot_path.join(".git").exists() {
        run_git(&abot_path, &["rev-parse", "--abbrev-ref", "HEAD"])
            .unwrap_or_else(|_| "main".to_string())
            .trim()
            .to_string()
    } else {
        "main".to_string()
    };

    let git_status = if abot_path.join(".git").exists() {
        run_git(&abot_path, &["status", "--short"])
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        String::new()
    };

    // Kubo branches
    let kubo_branches = if abot_path.join(".git").exists() {
        let branches_output =
            run_git(&abot_path, &["branch", "--list", "kubo/*"]).unwrap_or_default();
        let worktrees_output =
            run_git(&abot_path, &["worktree", "list", "--porcelain"]).unwrap_or_default();

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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_read_json() {
        let dir = std::env::temp_dir().join("abot-bundle-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.json");

        let value = serde_json::json!({"hello": "world"});
        write_json(&path, &value).unwrap();

        let read_back = read_json(&path).unwrap();
        assert_eq!(read_back, value);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_save_creates_bundle_structure() {
        let dir = std::env::temp_dir().join("abot-save-test");
        let _ = std::fs::remove_dir_all(&dir);

        let bundle_dir = dir.join("test-session.abot");

        let mut env = HashMap::new();
        env.insert(
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api-test".to_string(),
        );
        env.insert("CUSTOM_VAR".to_string(), "custom-value".to_string());

        let result = save_bundle(&bundle_dir, "test-session", &env).await;
        assert!(result.is_ok());

        assert!(bundle_dir.join("manifest.json").exists());
        assert!(bundle_dir.join("credentials.json").exists());
        assert!(bundle_dir.join("config.json").exists());

        // Verify manifest
        let manifest = read_json(&bundle_dir.join("manifest.json")).unwrap();
        assert_eq!(manifest["version"], BUNDLE_VERSION);
        assert_eq!(manifest["name"], "test-session");
        assert!(manifest.get("updated_at").is_some());

        // Verify credentials
        let creds = read_json(&bundle_dir.join("credentials.json")).unwrap();
        assert_eq!(creds["api_key"], "sk-ant-api-test");

        // Verify config
        let config = read_json(&bundle_dir.join("config.json")).unwrap();
        assert_eq!(config["env"]["CUSTOM_VAR"], "custom-value");
        assert!(config["env"].get("ANTHROPIC_API_KEY").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_save_preserves_created_at() {
        let dir = std::env::temp_dir().join("abot-save-preserve-test");
        let _ = std::fs::remove_dir_all(&dir);

        let bundle_dir = dir.join("test.abot");
        std::fs::create_dir_all(&bundle_dir).unwrap();

        // Write an initial manifest with a known created_at
        write_json(
            &bundle_dir.join("manifest.json"),
            &serde_json::json!({
                "version": 1,
                "name": "test",
                "created_at": "2025-01-01T00:00:00Z",
                "image": "abot-session",
            }),
        )
        .unwrap();

        // Save over it
        let env = HashMap::new();
        save_bundle(&bundle_dir, "test", &env).await.unwrap();

        let manifest = read_json(&bundle_dir.join("manifest.json")).unwrap();
        assert_eq!(manifest["created_at"], "2025-01-01T00:00:00Z");
        assert!(manifest.get("updated_at").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_open_reads_bundle() {
        let dir = std::env::temp_dir().join("abot-open-test");
        let _ = std::fs::remove_dir_all(&dir);

        let bundle_dir = dir.join("my-project.abot");
        std::fs::create_dir_all(&bundle_dir).unwrap();

        write_json(
            &bundle_dir.join("manifest.json"),
            &serde_json::json!({
                "version": 1,
                "name": "my-project",
                "created_at": "2026-03-02T10:30:00Z",
                "image": "abot-session",
            }),
        )
        .unwrap();

        write_json(
            &bundle_dir.join("credentials.json"),
            &serde_json::json!({
                "claude_token": "sk-ant-oat01-test-token"
            }),
        )
        .unwrap();

        write_json(
            &bundle_dir.join("config.json"),
            &serde_json::json!({
                "shell": "/bin/bash",
                "memory_mb": 2048,
                "cpu_percent": 50,
                "env": {
                    "MY_VAR": "hello"
                }
            }),
        )
        .unwrap();

        let result = open_bundle(bundle_dir.to_str().unwrap()).await;
        assert!(result.is_ok());

        let bundle = result.unwrap();
        assert_eq!(bundle.name, "my-project");
        assert_eq!(bundle.path, bundle_dir);
        assert_eq!(
            bundle.env.get("CLAUDE_CODE_OAUTH_TOKEN"),
            Some(&"sk-ant-oat01-test-token".to_string())
        );
        assert_eq!(bundle.env.get("MY_VAR"), Some(&"hello".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_save_then_open_roundtrip() {
        let dir = std::env::temp_dir().join("abot-roundtrip-test");
        let _ = std::fs::remove_dir_all(&dir);

        let bundle_dir = dir.join("roundtrip.abot");

        let mut env = HashMap::new();
        env.insert(
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api-roundtrip".to_string(),
        );
        env.insert("CLAUDE_CODE_OAUTH_TOKEN".to_string(), "tok-abc".to_string());
        env.insert("MY_CUSTOM".to_string(), "value123".to_string());

        // Save
        save_bundle(&bundle_dir, "my-session", &env).await.unwrap();

        // Open
        let opened = open_bundle(bundle_dir.to_str().unwrap()).await.unwrap();
        assert_eq!(opened.name, "my-session");
        assert_eq!(opened.path, bundle_dir);

        // Credential keys should round-trip through the friendly JSON names
        assert_eq!(
            opened.env.get("ANTHROPIC_API_KEY"),
            Some(&"sk-ant-api-roundtrip".to_string())
        );
        assert_eq!(
            opened.env.get("CLAUDE_CODE_OAUTH_TOKEN"),
            Some(&"tok-abc".to_string())
        );
        // Custom env should be preserved
        assert_eq!(opened.env.get("MY_CUSTOM"), Some(&"value123".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_open_nonexistent_path_fails() {
        let result = open_bundle("/tmp/abot-does-not-exist-xyz.abot").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn test_save_creates_dir_recursively() {
        let dir = std::env::temp_dir().join("abot-nested-test/deep/path/test.abot");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("abot-nested-test"));

        let env = HashMap::new();
        save_bundle(&dir, "nested", &env).await.unwrap();
        assert!(dir.join("manifest.json").exists());

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("abot-nested-test"));
    }

    #[test]
    fn test_ensure_bundle_home_creates_dirs() {
        let dir = std::env::temp_dir().join("abot-ensure-home-test");
        let _ = std::fs::remove_dir_all(&dir);

        let home = ensure_bundle_home(&dir, "test-session").unwrap();
        assert!(home.exists());
        assert!(home.is_dir());
        // v2 uses abots/ dir
        assert_eq!(home, dir.join("abots/test-session.abot/home"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_copy_dir_recursive() {
        let base = std::env::temp_dir().join("abot-copydir-test");
        let _ = std::fs::remove_dir_all(&base);

        let src = base.join("src");
        let dst = base.join("dst");

        // Create source tree
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "hello").unwrap();
        std::fs::write(src.join("sub/b.txt"), "world").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert_eq!(
            std::fs::read_to_string(dst.join("sub/b.txt")).unwrap(),
            "world"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn test_open_creates_home_if_missing() {
        let dir = std::env::temp_dir().join("abot-open-home-test");
        let _ = std::fs::remove_dir_all(&dir);

        let bundle_dir = dir.join("test.abot");
        std::fs::create_dir_all(&bundle_dir).unwrap();

        write_json(
            &bundle_dir.join("manifest.json"),
            &serde_json::json!({
                "version": 1,
                "name": "test",
                "created_at": "2026-03-02T10:30:00Z",
                "image": "abot-session",
            }),
        )
        .unwrap();

        let result = open_bundle(bundle_dir.to_str().unwrap()).await;
        assert!(result.is_ok());
        assert!(bundle_dir.join("home").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_save_and_load_scrollback() {
        let dir = std::env::temp_dir().join("abot-scrollback-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        save_scrollback(&dir, "hello\x1b[32mworld\x1b[0m");
        let loaded = load_scrollback(&dir);
        assert_eq!(loaded, Some("hello\x1b[32mworld\x1b[0m".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_save_scrollback_empty_is_noop() {
        let dir = std::env::temp_dir().join("abot-scrollback-empty-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        save_scrollback(&dir, "");
        assert!(!dir.join("scrollback").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_scrollback_missing_returns_none() {
        let dir = std::env::temp_dir().join("abot-scrollback-missing-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(load_scrollback(&dir), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Simulates the CreateSession → close → CreateSession round-trip:
    /// scrollback saved to bundle on close should be loadable on next create.
    #[test]
    fn test_scrollback_survives_create_session_roundtrip() {
        let dir = std::env::temp_dir().join("abot-scrollback-roundtrip-test");
        let _ = std::fs::remove_dir_all(&dir);

        // 1. First session: ensure_bundle_home creates the bundle dir
        let home = ensure_bundle_home(&dir, "main").unwrap();
        let bundle_path = home.parent().unwrap();

        // 2. Session produces terminal output, close saves scrollback
        let terminal_output = "user@host:~$ ls\nfile1.txt  file2.txt\nuser@host:~$ ";
        save_scrollback(bundle_path, terminal_output);
        assert!(bundle_path.join("scrollback").exists());

        // 3. Daemon restarts, ensure_bundle_home again (idempotent)
        let home2 = ensure_bundle_home(&dir, "main").unwrap();
        let bundle_path2 = home2.parent().unwrap();

        // 4. Load scrollback — must survive the restart
        let loaded = load_scrollback(bundle_path2);
        assert_eq!(loaded, Some(terminal_output.to_string()));

        // 5. Pre-populate ring buffer (what CreateSession should do)
        let mut buf = crate::daemon::ring_buffer::RingBuffer::new(5000, 5 * 1024 * 1024);
        if let Some(scrollback) = loaded {
            buf.pre_populate(scrollback);
        }
        assert_eq!(buf.to_string(), terminal_output);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_open_rejects_bad_version() {
        let dir = std::env::temp_dir().join("abot-badver-test");
        let _ = std::fs::remove_dir_all(&dir);
        let bundle_dir = dir.join("bad.abot");
        std::fs::create_dir_all(&bundle_dir).unwrap();

        write_json(
            &bundle_dir.join("manifest.json"),
            &serde_json::json!({
                "version": 99,
                "name": "bad"
            }),
        )
        .unwrap();

        let result = open_bundle(bundle_dir.to_str().unwrap()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unsupported bundle version"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_credentials_api_key() {
        let dir = std::env::temp_dir().join("abot-creds-api-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("credentials.json");
        write_json(&path, &serde_json::json!({"api_key": "sk-ant-api-test123"})).unwrap();

        let env = read_credentials(&path);
        assert_eq!(env.get("ANTHROPIC_API_KEY").unwrap(), "sk-ant-api-test123");
        assert_eq!(env.get("CLAUDE_API_KEY").unwrap(), "sk-ant-api-test123");
        assert!(!env.contains_key("CLAUDE_CODE_OAUTH_TOKEN"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_credentials_non_api_key_becomes_oauth() {
        let dir = std::env::temp_dir().join("abot-creds-oauth-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("credentials.json");
        write_json(&path, &serde_json::json!({"api_key": "some-other-token"})).unwrap();

        let env = read_credentials(&path);
        assert_eq!(
            env.get("CLAUDE_CODE_OAUTH_TOKEN").unwrap(),
            "some-other-token"
        );
        assert!(!env.contains_key("ANTHROPIC_API_KEY"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_credentials_claude_token() {
        let dir = std::env::temp_dir().join("abot-creds-token-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("credentials.json");
        write_json(&path, &serde_json::json!({"claude_token": "tok-abc"})).unwrap();

        let env = read_credentials(&path);
        assert_eq!(env.get("CLAUDE_CODE_OAUTH_TOKEN").unwrap(), "tok-abc");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_credentials_missing_file() {
        let env = read_credentials(Path::new("/tmp/abot-does-not-exist/credentials.json"));
        assert!(env.is_empty());
    }

    #[test]
    fn test_read_credentials_empty_json() {
        let dir = std::env::temp_dir().join("abot-creds-empty-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("credentials.json");
        std::fs::write(&path, "{}").unwrap();

        let env = read_credentials(&path);
        assert!(env.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

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
    fn test_get_abot_detail_with_git() {
        let dir = std::env::temp_dir().join("abot-detail-git-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Create a canonical abot
        let abots_dir = dir.join("abots");
        let abot_path = create_canonical_abot(&abots_dir, "testbot").unwrap();
        assert!(abot_path.join(".git").exists());

        let detail = get_abot_detail(&dir, "testbot").unwrap();
        assert_eq!(detail.name, "testbot");
        assert!(detail.created_at.is_some());
        assert!(!detail.default_branch.is_empty());
        assert!(detail.kubo_branches.is_empty()); // no kubo branches yet

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_integrate_variant_commits_worktree_changes() {
        let dir = std::env::temp_dir().join("abot-integrate-commit-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Create canonical abot + kubo worktree
        let abots_dir = dir.join("abots");
        let abot_path = create_canonical_abot(&abots_dir, "alice").unwrap();
        let kubos_dir = dir.join("kubos");
        crate::daemon::kubo::Kubo::ensure_kubo_dir(&kubos_dir, "lab").unwrap();
        let kubo_path = kubos_dir.join("lab.kubo");
        worktree_add_abot(&abot_path, &kubo_path, "alice", "lab").unwrap();

        // Create a file in the worktree (simulating terminal work)
        let wt_path = kubo_path.join("alice");
        let test_file = wt_path.join("home").join("work.txt");
        std::fs::create_dir_all(test_file.parent().unwrap()).unwrap();
        std::fs::write(&test_file, "hello from lab").unwrap();

        // Verify file is NOT on default branch yet
        let default_file = abot_path.join("home").join("work.txt");
        assert!(!default_file.exists());

        // Integrate — should auto-commit worktree changes, merge into default
        integrate_variant(&abot_path, "kubo/lab").unwrap();

        // File should now be on the default branch
        assert!(
            default_file.exists(),
            "work.txt should exist on default branch after integrate"
        );
        assert_eq!(
            std::fs::read_to_string(&default_file).unwrap(),
            "hello from lab"
        );

        // Kubo branch should be deleted
        let branches = run_git(&abot_path, &["branch", "--list", "kubo/lab"]).unwrap();
        assert!(branches.trim().is_empty(), "kubo/lab branch should be gone");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discard_variant_does_not_commit_changes() {
        let dir = std::env::temp_dir().join("abot-discard-nocommit-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let abots_dir = dir.join("abots");
        let abot_path = create_canonical_abot(&abots_dir, "alice").unwrap();
        let kubos_dir = dir.join("kubos");
        crate::daemon::kubo::Kubo::ensure_kubo_dir(&kubos_dir, "lab").unwrap();
        let kubo_path = kubos_dir.join("lab.kubo");
        worktree_add_abot(&abot_path, &kubo_path, "alice", "lab").unwrap();

        // Create a file in the worktree
        let wt_path = kubo_path.join("alice");
        let test_file = wt_path.join("home").join("secret.txt");
        std::fs::create_dir_all(test_file.parent().unwrap()).unwrap();
        std::fs::write(&test_file, "discard me").unwrap();

        // Discard — should NOT commit changes
        discard_variant(&abot_path, "kubo/lab").unwrap();

        // File should NOT be on the default branch
        let default_file = abot_path.join("home").join("secret.txt");
        assert!(
            !default_file.exists(),
            "secret.txt should NOT exist after discard"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_sync_known_abots_with_kubos() {
        let dir = std::env::temp_dir().join("abot-sync-known-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Create a kubo with some abots in its manifest
        let kubos_dir = dir.join("kubos");
        crate::daemon::kubo::Kubo::ensure_kubo_dir(&kubos_dir, "test-kubo").unwrap();
        let kubo_path = kubos_dir.join("test-kubo.kubo");
        let mut manifest = crate::daemon::kubo::Kubo::read_manifest(&kubo_path).unwrap();
        manifest.abots = vec!["alice".to_string(), "bob".to_string()];
        crate::daemon::kubo::Kubo::write_manifest(&kubo_path, &manifest).unwrap();

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
