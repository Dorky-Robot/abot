//! `.abot` bundle save/open — document-style persistence.
//!
//! A `.abot` bundle is a directory containing:
//!   - manifest.json   (name, version, timestamps)
//!   - credentials.json (extracted credential env vars)
//!   - config.json     (shell, resource limits, custom env)
//!   - home/           (bind-mounted as /home/dev in Docker container)

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(crate) const BUNDLE_VERSION: u32 = 2;
const LEGACY_BUNDLE_VERSION: u32 = 1;

// Re-export from submodules so callers can continue using `bundle::`.
pub use super::git_ops::{
    auto_commit_abot, create_canonical_abot, discard_variant, dismiss_variant, git_init_abot,
    integrate_variant, worktree_add_abot,
};
pub use super::known_abots::{
    add_known_abot, get_abot_detail, read_known_abots, remove_known_abot, sync_known_abots,
    AbotDetail,
};

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
    env: &HashMap<String, String>,
) -> Result<()> {
    // Ensure directory exists
    std::fs::create_dir_all(bundle_path)
        .with_context(|| format!("failed to create bundle dir: {}", bundle_path.display()))?;

    // Read existing manifest to preserve created_at
    let existing = read_json(&bundle_path.join("manifest.json")).ok();
    let created_at = existing
        .as_ref()
        .and_then(|m| m.get("created_at"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let manifest = serde_json::json!({
        "version": BUNDLE_VERSION,
        "name": name,
        "created_at": created_at,
        "updated_at": chrono::Utc::now().to_rfc3339(),
    });
    write_json(&bundle_path.join("manifest.json"), &manifest)?;

    // credentials.json — extract known credential env vars
    let mut creds = serde_json::Map::new();
    if let Some(key) = env.get("ANTHROPIC_API_KEY") {
        creds.insert(
            "api_key".to_string(),
            serde_json::Value::String(key.clone()),
        );
    }
    if let Some(token) = env.get("CLAUDE_CODE_OAUTH_TOKEN") {
        creds.insert(
            "claude_token".to_string(),
            serde_json::Value::String(token.clone()),
        );
    }
    let creds_path = bundle_path.join("credentials.json");
    write_json(&creds_path, &serde_json::Value::Object(creds))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600));
    }

    // config.json — env vars (minus credentials), shell settings
    let config_env: HashMap<&str, &str> = env
        .iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "ANTHROPIC_API_KEY" | "CLAUDE_API_KEY" | "CLAUDE_CODE_OAUTH_TOKEN"
            )
        })
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let config = serde_json::json!({
        "shell": "/bin/bash",
        "env": config_env,
    });
    write_json(&bundle_path.join("config.json"), &config)?;

    Ok(())
}

/// Open a `.abot` bundle directory, returning the session name, env, and path.
pub async fn open_bundle(path: &str) -> Result<OpenedBundle> {
    let bundle_path = PathBuf::from(path);
    if !bundle_path.exists() {
        anyhow::bail!("bundle does not exist: {}", path);
    }

    // Read manifest
    let manifest_path = bundle_path.join("manifest.json");
    let manifest = if manifest_path.exists() {
        read_json(&manifest_path)?
    } else {
        // Minimal manifest from directory name
        let name = bundle_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed");
        serde_json::json!({"version": 1, "name": name})
    };

    let version = manifest
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);
    if version != BUNDLE_VERSION as u64 && version != LEGACY_BUNDLE_VERSION as u64 {
        anyhow::bail!("unsupported bundle version: {}", version);
    }

    let name = manifest
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unnamed")
        .to_string();

    // Read credentials
    let creds_path = bundle_path.join("credentials.json");
    let env_from_creds = read_credentials(&creds_path);

    // Read config
    let config_path = bundle_path.join("config.json");
    let config = read_json(&config_path).unwrap_or(serde_json::json!({}));

    let mut env = HashMap::new();
    env.extend(env_from_creds);

    // Config env vars
    if let Some(env_obj) = config.get("env").and_then(|v| v.as_object()) {
        for (k, v) in env_obj {
            if let Some(val) = v.as_str() {
                env.insert(k.clone(), val.to_string());
            }
        }
    }

    // Migrate archive to home/ if needed
    let archive_path = bundle_path.join("filesystem.tar.zst");
    let home_dir = bundle_path.join("home");
    if archive_path.exists() && !home_dir.exists() {
        if let Err(e) = migrate_archive_to_home(&archive_path, &home_dir).await {
            tracing::warn!("failed to migrate archive: {}", e);
        }
    }

    // Ensure home/ exists
    let _ = std::fs::create_dir_all(bundle_path.join("home"));

    Ok(OpenedBundle {
        name,
        env,
        path: bundle_path,
    })
}

/// Read a `credentials.json` file and return env vars for container injection.
pub fn read_credentials(path: &Path) -> HashMap<String, String> {
    super::credentials::read_credentials_file(path)
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
    let scrollback_path = bundle_path.join("scrollback");
    let tmp_path = bundle_path.join("scrollback.tmp");
    if std::fs::write(&tmp_path, content).is_ok() {
        let _ = std::fs::rename(&tmp_path, &scrollback_path);
    }
}

/// Load terminal scrollback from a bundle directory.
/// Returns `None` if the file is missing or unreadable.
pub fn load_scrollback(bundle_path: &Path) -> Option<String> {
    let scrollback_path = bundle_path.join("scrollback");
    std::fs::read_to_string(scrollback_path).ok()
}

/// Ensure a bundle's `home/` directory exists, creating the full bundle structure.
/// Returns the path to the `home/` directory.
/// Uses configured abots dir (v2), falling back to `bundles/` if it exists (v1 compat).
/// Used by CreateSession and tests; will be superseded by worktree model for kubo sessions.
#[allow(dead_code)]
pub fn ensure_bundle_home(data_dir: &Path, name: &str) -> Result<PathBuf> {
    let abots_dir = resolve_abots_dir(data_dir);
    let bundle_path = abots_dir.join(format!("{name}.abot"));
    let home_dir = bundle_path.join("home");

    std::fs::create_dir_all(&home_dir)
        .with_context(|| format!("failed to create bundle home: {}", home_dir.display()))?;

    // Write minimal manifest if missing
    let manifest_path = bundle_path.join("manifest.json");
    if !manifest_path.exists() {
        let now = chrono::Utc::now().to_rfc3339();
        let manifest = serde_json::json!({
            "version": BUNDLE_VERSION,
            "name": name,
            "created_at": now,
            "updated_at": now,
        });
        write_json(&manifest_path, &manifest)?;
    }

    Ok(home_dir)
}

/// Recursively copy a directory tree from `src` to `dst`.
/// Symlinks are skipped to prevent following links outside the bundle.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.is_dir() {
        anyhow::bail!("source is not a directory: {}", src.display());
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());

        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue; // skip symlinks for safety
        }
        if ft.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

/// Migrate a legacy `filesystem.tar.zst` archive into a `home/` directory.
async fn migrate_archive_to_home(archive_path: &Path, home_dir: &Path) -> Result<()> {
    let data = std::fs::read(archive_path)?;
    let decoder = zstd::Decoder::new(std::io::Cursor::new(data))?;
    let mut archive = tar::Archive::new(decoder);

    std::fs::create_dir_all(home_dir)?;
    archive.unpack(home_dir)?;

    // Remove the archive after successful migration
    let _ = std::fs::remove_file(archive_path);
    tracing::info!("migrated archive to home/: {}", archive_path.display());
    Ok(())
}

// ── Data directory & path resolution ────────────────────────────

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

        // 3. Server restarts, ensure_bundle_home again (idempotent)
        let home2 = ensure_bundle_home(&dir, "main").unwrap();
        let bundle_path2 = home2.parent().unwrap();

        // 4. Load scrollback — must survive the restart
        let loaded = load_scrollback(bundle_path2);
        assert_eq!(loaded, Some(terminal_output.to_string()));

        // 5. Pre-populate ring buffer (what CreateSession should do)
        let mut buf = crate::engine::ring_buffer::RingBuffer::new(5000, 5 * 1024 * 1024);
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
}
