//! `.abot` bundle export/import.
//!
//! A `.abot` bundle is a directory containing:
//!   - manifest.json   (name, version, timestamps)
//!   - credentials.json (extracted credential env vars)
//!   - config.json     (shell, resource limits, custom env)
//!   - filesystem.tar.zst (compressed snapshot of /home/dev from Docker volume)

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

/// Result of importing a bundle — enough info to create a session.
#[derive(Debug)]
pub struct ImportedBundle {
    pub name: String,
    pub env: HashMap<String, String>,
}

/// Export a session as a `.abot` bundle directory.
///
/// Creates `{path}/{name}.abot/` with manifest, credentials, config,
/// and a filesystem snapshot from the Docker volume.
pub async fn export_bundle(
    name: &str,
    session_env: &HashMap<String, String>,
    path: &str,
) -> Result<String> {
    let bundle_dir = PathBuf::from(path).join(format!("{name}.abot"));
    std::fs::create_dir_all(&bundle_dir)
        .with_context(|| format!("failed to create bundle dir: {}", bundle_dir.display()))?;

    // 1. Write manifest.json
    let manifest = serde_json::json!({
        "version": BUNDLE_VERSION,
        "name": name,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "image": SESSION_IMAGE,
    });
    write_json(&bundle_dir.join("manifest.json"), &manifest)?;

    // 2. Write credentials.json (extract credential keys from session env)
    let mut creds = serde_json::Map::new();
    for key in CREDENTIAL_KEYS {
        if let Some(val) = session_env.get(*key) {
            // Map env var name to a friendlier JSON key
            let json_key = match *key {
                "ANTHROPIC_API_KEY" | "CLAUDE_API_KEY" => "api_key",
                "CLAUDE_CODE_OAUTH_TOKEN" => "claude_token",
                _ => *key,
            };
            creds.insert(json_key.to_string(), serde_json::Value::String(val.clone()));
        }
    }
    write_json(
        &bundle_dir.join("credentials.json"),
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
    write_json(&bundle_dir.join("config.json"), &config)?;

    // 4. Snapshot Docker volume via `docker cp`
    #[cfg(feature = "docker")]
    {
        let snapshot_path = bundle_dir.join("filesystem.tar.zst");
        snapshot_volume(name, &snapshot_path).await?;
    }

    // Set directory permissions to 0o700
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bundle_dir, std::fs::Permissions::from_mode(0o700))?;
        // Protect credentials.json
        let creds_path = bundle_dir.join("credentials.json");
        if creds_path.exists() {
            std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600))?;
        }
    }

    Ok(bundle_dir.to_string_lossy().to_string())
}

/// Import a `.abot` bundle directory, returning the session name and env.
pub async fn import_bundle(path: &str) -> Result<ImportedBundle> {
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
            // Map friendly JSON keys back to env var names
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

    // 4. Restore filesystem snapshot if present
    #[cfg(feature = "docker")]
    {
        let snapshot_path = bundle_dir.join("filesystem.tar.zst");
        if snapshot_path.exists() {
            restore_volume(&name, &snapshot_path).await?;
        }
    }

    Ok(ImportedBundle { name, env })
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn read_json(path: &Path) -> Result<serde_json::Value> {
    let contents = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&contents)?;
    Ok(value)
}

/// Snapshot a Docker volume's /home/dev as a zstd-compressed tar archive.
#[cfg(feature = "docker")]
async fn snapshot_volume(session_name: &str, output_path: &Path) -> Result<()> {
    let container_name = format!("abot-{session_name}");
    let output = tokio::process::Command::new("docker")
        .args(["cp", &format!("{container_name}:/home/dev/."), "-"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .with_context(|| "failed to run docker cp")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker cp failed: {}", stderr);
    }

    // Compress the tar data with zstd
    let compressed =
        zstd::encode_all(output.stdout.as_slice(), 3).with_context(|| "zstd compression failed")?;
    std::fs::write(output_path, compressed)?;
    Ok(())
}

/// Restore a zstd-compressed tar archive into a Docker volume.
#[cfg(feature = "docker")]
async fn restore_volume(session_name: &str, snapshot_path: &Path) -> Result<()> {
    let volume_name = format!("abot-agent-{session_name}");

    // Create the volume first
    let status = tokio::process::Command::new("docker")
        .args(["volume", "create", &volume_name])
        .status()
        .await
        .with_context(|| "failed to create docker volume")?;

    if !status.success() {
        anyhow::bail!("docker volume create failed");
    }

    // Decompress and pipe into a temporary container that writes to the volume
    let compressed = std::fs::read(snapshot_path)?;
    let decompressed =
        zstd::decode_all(compressed.as_slice()).with_context(|| "zstd decompression failed")?;

    // Use a helper container to extract into the volume
    let mut child = tokio::process::Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            "-v",
            &format!("{volume_name}:/home/dev"),
            "alpine:3",
            "tar",
            "xf",
            "-",
            "-C",
            "/home/dev",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| "failed to spawn restore container")?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(&decompressed).await?;
        drop(stdin);
    }

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("volume restore failed: {}", stderr);
    }

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
    async fn test_export_creates_bundle_structure() {
        let dir = std::env::temp_dir().join("abot-export-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut env = HashMap::new();
        env.insert(
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api-test".to_string(),
        );
        env.insert("CUSTOM_VAR".to_string(), "custom-value".to_string());

        let result = export_bundle("test-session", &env, dir.to_str().unwrap()).await;
        assert!(result.is_ok());

        let bundle_dir = dir.join("test-session.abot");
        assert!(bundle_dir.join("manifest.json").exists());
        assert!(bundle_dir.join("credentials.json").exists());
        assert!(bundle_dir.join("config.json").exists());

        // Verify manifest
        let manifest = read_json(&bundle_dir.join("manifest.json")).unwrap();
        assert_eq!(manifest["version"], 1);
        assert_eq!(manifest["name"], "test-session");

        // Verify credentials
        let creds = read_json(&bundle_dir.join("credentials.json")).unwrap();
        assert_eq!(creds["api_key"], "sk-ant-api-test");

        // Verify config
        let config = read_json(&bundle_dir.join("config.json")).unwrap();
        assert_eq!(config["env"]["CUSTOM_VAR"], "custom-value");
        // Credential keys should not be in config env
        assert!(config["env"].get("ANTHROPIC_API_KEY").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_import_reads_bundle() {
        let dir = std::env::temp_dir().join("abot-import-test");
        let _ = std::fs::remove_dir_all(&dir);

        // Create a bundle manually
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

        let result = import_bundle(bundle_dir.to_str().unwrap()).await;
        assert!(result.is_ok());

        let bundle = result.unwrap();
        assert_eq!(bundle.name, "my-project");
        assert_eq!(
            bundle.env.get("CLAUDE_CODE_OAUTH_TOKEN"),
            Some(&"sk-ant-oat01-test-token".to_string())
        );
        assert_eq!(bundle.env.get("MY_VAR"), Some(&"hello".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_import_rejects_bad_version() {
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

        let result = import_bundle(bundle_dir.to_str().unwrap()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unsupported bundle version"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
