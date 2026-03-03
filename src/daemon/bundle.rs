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

const BUNDLE_VERSION: u32 = 1;
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
        "memory_mb": 512,
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
        .unwrap_or(0);
    if version != BUNDLE_VERSION as u64 {
        anyhow::bail!(
            "unsupported bundle version {} (expected {})",
            version,
            BUNDLE_VERSION
        );
    }

    let name = manifest
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("imported")
        .to_string();

    // 2. Read credentials.json → session env
    let mut env = HashMap::new();
    let creds_path = bundle_dir.join("credentials.json");
    if creds_path.exists() {
        let creds: serde_json::Value = read_json(&creds_path)?;
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
    }

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
        #[cfg(feature = "docker")]
        {
            let snapshot_path = bundle_dir.join("filesystem.tar.zst");
            if snapshot_path.exists() {
                migrate_archive_to_home(&snapshot_path, &home_dir).await?;
            } else {
                std::fs::create_dir_all(&home_dir)
                    .with_context(|| "failed to create home/ in bundle")?;
            }
        }
        #[cfg(not(feature = "docker"))]
        {
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

/// Ensure a bundle's `home/` directory exists, creating the full bundle structure.
/// Returns the path to the `home/` directory.
pub fn ensure_bundle_home(data_dir: &Path, name: &str) -> Result<PathBuf> {
    validate_session_name(name)?;
    let bundle_dir = data_dir.join("bundles").join(format!("{name}.abot"));
    let home_dir = bundle_dir.join("home");
    std::fs::create_dir_all(&home_dir)
        .with_context(|| format!("failed to create bundle home: {}", home_dir.display()))?;
    Ok(home_dir)
}

/// Validate that a session name is safe for use in filesystem paths.
/// Rejects path traversal components, slashes, and other dangerous characters.
fn validate_session_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("session name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        anyhow::bail!("session name contains invalid characters: {}", name);
    }
    if name == "." || name == ".." || name.starts_with("../") || name.contains("/../") {
        anyhow::bail!("session name contains path traversal: {}", name);
    }
    Ok(())
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
#[cfg(feature = "docker")]
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
        assert_eq!(manifest["version"], 1);
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
                "memory_mb": 512,
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
        assert_eq!(home, dir.join("bundles/test-session.abot/home"));

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
}
