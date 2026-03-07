//! Kubo — a shared runtime room (one Docker container hosting many abots).
//!
//! Each kubo is a directory under `~/.abot/kubos/{name}.kubo/` and runs as a
//! single long-lived Docker container. A kubo is NOT a git repo — it's
//! infrastructure (container config + credentials + manifest). Abots are
//! added as git worktrees and share the container's tools. Sessions use
//! `docker exec` into the container.

use anyhow::{Context, Result};
use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
use bollard::image::{BuildImageOptions, CreateImageOptions};
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use bollard::Docker;
use futures_util::TryStreamExt;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const DEFAULT_KUBO_IMAGE: &str = "abot-kubo";
const FALLBACK_IMAGE: &str = "alpine:3";
const IDLE_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Runtime state for a kubo container.
pub struct Kubo {
    pub name: String,
    pub path: PathBuf,
    pub container_id: Option<String>,
    pub docker: Docker,
    /// Names of active sessions using this kubo.
    pub active_sessions: HashSet<String>,
    /// Timestamp of last session close (for idle timeout).
    pub last_session_close: Option<std::time::Instant>,
}

/// Kubo manifest stored at `{kubo_path}/manifest.json`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct KuboManifest {
    pub version: u32,
    pub name: String,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Abot names currently employed (as worktrees) in this kubo.
    #[serde(default)]
    pub abots: Vec<String>,
}

/// Typed summary of a kubo, suitable for JSON serialization.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KuboSummary {
    pub name: String,
    pub path: String,
    pub running: bool,
    pub active_sessions: usize,
    pub abots: Vec<String>,
}

/// Validate that a kubo or abot name is safe for filesystem paths and git refs.
/// Rejects path traversal, slashes, null bytes, and characters invalid in git branch names.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("name cannot be empty");
    }
    // Allowlist: alphanumeric, hyphen, underscore, dot.
    // Safe for git refs, shell interpolation, and filesystem paths.
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        anyhow::bail!("name must contain only [a-zA-Z0-9._-]: {}", name);
    }
    if name == "." || name == ".." {
        anyhow::bail!("name cannot be '.' or '..'");
    }
    if name.contains("..") {
        anyhow::bail!("name contains '..' which is invalid in git refs: {}", name);
    }
    if name.ends_with('.') || name.ends_with(".lock") {
        anyhow::bail!(
            "name ends with '.' or '.lock' which is invalid in git refs: {}",
            name
        );
    }
    Ok(())
}

impl Kubo {
    /// Ensure the kubo directory exists with a manifest and git repo.
    pub fn ensure_kubo_dir(kubos_dir: &Path, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        let kubo_path = kubos_dir.join(format!("{name}.kubo"));
        std::fs::create_dir_all(&kubo_path)
            .with_context(|| format!("failed to create kubo dir: {}", kubo_path.display()))?;

        let manifest_path = kubo_path.join("manifest.json");
        if !manifest_path.exists() {
            let now = chrono::Utc::now().to_rfc3339();
            let manifest = KuboManifest {
                version: 1,
                name: name.to_string(),
                created_at: now.clone(),
                updated_at: Some(now),
                abots: vec![],
            };
            let json = serde_json::to_string_pretty(&manifest)?;
            std::fs::write(&manifest_path, json)?;
        }

        // Create empty credentials.json with restricted permissions
        let creds_path = kubo_path.join("credentials.json");
        if !creds_path.exists() {
            std::fs::write(&creds_path, "{}")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600))?;
            }
        }

        Ok(kubo_path)
    }

    /// Read the kubo manifest.
    pub fn read_manifest(kubo_path: &Path) -> Result<KuboManifest> {
        let manifest_path = kubo_path.join("manifest.json");
        let contents = std::fs::read_to_string(&manifest_path).with_context(|| {
            format!("failed to read kubo manifest: {}", manifest_path.display())
        })?;
        let manifest: KuboManifest = serde_json::from_str(&contents)?;
        Ok(manifest)
    }

    /// Write the kubo manifest.
    pub fn write_manifest(kubo_path: &Path, manifest: &KuboManifest) -> Result<()> {
        let manifest_path = kubo_path.join("manifest.json");
        let json = serde_json::to_string_pretty(manifest)?;
        std::fs::write(&manifest_path, json)?;
        Ok(())
    }

    /// Start the kubo container. Idempotent — reattaches if already running.
    pub async fn start(&mut self) -> Result<()> {
        if self.container_id.is_some() {
            // Check if container is still running
            if self.is_running().await {
                return Ok(());
            }
            // Container died, clear the ID
            self.container_id = None;
        }

        let container_name = format!("abot-kubo-{}", self.name);

        // Check if a container with this name already exists and is running
        if let Ok(info) = self.docker.inspect_container(&container_name, None).await {
            if info.state.as_ref().and_then(|s| s.running).unwrap_or(false) {
                self.container_id = info.id;
                tracing::info!("reattached to existing kubo container '{}'", container_name);
                return Ok(());
            }
            // Container exists but isn't running — remove and recreate
            let _ = self
                .docker
                .remove_container(
                    &container_name,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
        }

        // Determine which image to use
        let image = self.resolve_image().await?;

        let env_vars = vec![
            "TERM=xterm-256color".to_string(),
            "COLORTERM=truecolor".to_string(),
            "LANG=en_US.UTF-8".to_string(),
            "PATH=/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string(),
        ];

        let config = Config {
            image: Some(image.clone()),
            tty: Some(false),
            open_stdin: Some(false),
            cmd: Some(vec!["sleep".to_string(), "infinity".to_string()]),
            env: Some(env_vars),
            user: Some("1000:1000".to_string()),
            host_config: Some(HostConfig {
                memory: Some(2048 * 1024 * 1024),      // 2 GB base
                memory_swap: Some(2048 * 1024 * 1024), // No swap
                cpu_period: Some(100_000),
                cpu_quota: Some(100_000), // 100% of one CPU (shared)
                pids_limit: Some(512),    // More PIDs for multi-abot
                cap_drop: Some(vec!["ALL".to_string()]),
                security_opt: Some(vec!["no-new-privileges".to_string()]),
                readonly_rootfs: Some(false),
                mounts: Some(vec![Mount {
                    target: Some("/home/abots".to_string()),
                    source: Some(self.path.to_string_lossy().to_string()),
                    typ: Some(MountTypeEnum::BIND),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let container = self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.clone(),
                    ..Default::default()
                }),
                config,
            )
            .await
            .with_context(|| format!("failed to create kubo container '{}'", container_name))?;

        self.docker
            .start_container::<String>(&container.id, None)
            .await
            .with_context(|| format!("failed to start kubo container '{}'", container_name))?;

        tracing::info!(
            "started kubo container '{}' (id: {})",
            container_name,
            container.id.get(..12).unwrap_or(&container.id)
        );
        self.container_id = Some(container.id);
        Ok(())
    }

    /// Stop the kubo container.
    /// Tries by container ID first, then by container name (handles server restart).
    pub async fn stop(&mut self) -> Result<()> {
        let force_remove = || {
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            })
        };

        if let Some(ref id) = self.container_id {
            let _ = self.docker.remove_container(id, force_remove()).await;
            tracing::info!("stopped kubo container '{}'", self.name);
            self.container_id = None;
            return Ok(());
        }

        // Fallback: try by container name
        let container_name = format!("abot-kubo-{}", self.name);
        if self
            .docker
            .inspect_container(&container_name, None)
            .await
            .is_ok()
        {
            let _ = self
                .docker
                .remove_container(&container_name, force_remove())
                .await;
            tracing::info!("stopped kubo container '{}' (by name)", self.name);
        }
        Ok(())
    }

    /// Check if the container is still running via Docker API.
    /// Checks both by container ID (if known) and by container name (for server restarts).
    /// Check if a container is running by ID, without needing a Kubo reference.
    /// Used by health checks to avoid holding the kubos mutex during Docker calls.
    pub async fn check_container_running(container_id: &str) -> bool {
        if let Ok(docker) = bollard::Docker::connect_with_socket_defaults() {
            if let Ok(info) = docker.inspect_container(container_id, None).await {
                return info.state.as_ref().and_then(|s| s.running).unwrap_or(false);
            }
        }
        false
    }

    pub async fn is_running(&self) -> bool {
        // First try by container ID (fast path)
        if let Some(ref id) = self.container_id {
            if let Ok(info) = self.docker.inspect_container(id, None).await {
                return info.state.as_ref().and_then(|s| s.running).unwrap_or(false);
            }
        }
        // Fallback: check by container name (handles server restart with live container)
        let container_name = format!("abot-kubo-{}", self.name);
        if let Ok(info) = self.docker.inspect_container(&container_name, None).await {
            return info.state.as_ref().and_then(|s| s.running).unwrap_or(false);
        }
        false
    }

    /// Check if this kubo should be stopped due to idle timeout.
    pub fn should_idle_stop(&self) -> bool {
        if !self.active_sessions.is_empty() {
            return false;
        }
        if let Some(last_close) = self.last_session_close {
            last_close.elapsed().as_secs() >= IDLE_TIMEOUT_SECS
        } else {
            false
        }
    }

    /// Record that a session was opened in this kubo.
    pub fn session_opened(&mut self, session_name: &str) {
        self.active_sessions.insert(session_name.to_string());
        self.last_session_close = None;
    }

    /// Record that a session was closed in this kubo.
    pub fn session_closed(&mut self, session_name: &str) {
        self.active_sessions.remove(session_name);
        if self.active_sessions.is_empty() {
            self.last_session_close = Some(std::time::Instant::now());
        }
    }

    /// Resolve which Docker image to use for this kubo.
    /// Custom Dockerfile in kubo dir → build custom image.
    /// Otherwise → use default abot-kubo image, falling back to alpine.
    async fn resolve_image(&self) -> Result<String> {
        let dockerfile = self.path.join("Dockerfile");
        if dockerfile.exists() {
            let custom_image = format!("abot-kubo-{}", self.name);
            self.build_custom_image(&custom_image, &dockerfile).await?;
            return Ok(custom_image);
        }

        // Try default kubo image
        if image_exists(&self.docker, DEFAULT_KUBO_IMAGE).await {
            return Ok(DEFAULT_KUBO_IMAGE.to_string());
        }

        // Fall back to alpine
        if !image_exists(&self.docker, FALLBACK_IMAGE).await {
            self.docker
                .create_image(
                    Some(CreateImageOptions {
                        from_image: FALLBACK_IMAGE,
                        ..Default::default()
                    }),
                    None,
                    None,
                )
                .try_collect::<Vec<_>>()
                .await?;
        }
        Ok(FALLBACK_IMAGE.to_string())
    }

    /// Build a custom Docker image from a Dockerfile in the kubo dir.
    async fn build_custom_image(&self, image_name: &str, dockerfile: &Path) -> Result<()> {
        // Read Dockerfile content and create a tar archive for the build context
        let dockerfile_content = std::fs::read(dockerfile)?;

        let mut header = tar::Header::new_gnu();
        header.set_path("Dockerfile")?;
        header.set_size(dockerfile_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        let mut tar_buf = Vec::new();
        {
            let mut tar_builder = tar::Builder::new(&mut tar_buf);
            tar_builder.append(&header, &*dockerfile_content)?;
            tar_builder.finish()?;
        }

        self.docker
            .build_image(
                BuildImageOptions {
                    t: image_name,
                    rm: true,
                    ..Default::default()
                },
                None,
                Some(tar_buf.into()),
            )
            .try_collect::<Vec<_>>()
            .await
            .with_context(|| format!("failed to build custom kubo image '{}'", image_name))?;

        tracing::info!("built custom kubo image '{}'", image_name);
        Ok(())
    }

    /// Ensure an abot's home directory exists inside this kubo.
    /// If the abot dir already has a `.git` file (worktree) or `.git` dir,
    /// skip git init — the worktree setup handles git state.
    pub fn ensure_abot_home(&self, abot_name: &str) -> Result<PathBuf> {
        validate_name(abot_name)?;
        let abot_dir = self.path.join(abot_name);
        let home_dir = abot_dir.join("home");
        std::fs::create_dir_all(&home_dir).with_context(|| {
            format!("failed to create abot home in kubo: {}", home_dir.display())
        })?;
        // Only init git if .git doesn't exist at all. A worktree has a .git
        // *file* (not dir), which also satisfies .exists(), so we skip init
        // for both regular repos and worktrees.
        if !abot_dir.join(".git").exists() {
            if let Err(e) = super::bundle::git_init_abot(&abot_dir) {
                tracing::warn!("failed to git-init abot {}: {}", abot_name, e);
            }
        }
        Ok(abot_dir)
    }

    /// Get the container-internal working directory for an abot.
    pub fn abot_workdir(abot_name: &str) -> String {
        format!("/home/abots/{}/home", abot_name)
    }

    /// Serialize to JSON for IPC responses.
    /// Queries Docker for live container status instead of relying on in-memory flag.
    pub async fn summary(&self) -> KuboSummary {
        let abots = Self::read_manifest(&self.path)
            .map(|m| m.abots)
            .unwrap_or_default();
        let running = self.is_running().await;
        KuboSummary {
            name: self.name.clone(),
            path: self.path.to_string_lossy().to_string(),
            running,
            active_sessions: self.active_sessions.len(),
            abots,
        }
    }
}

/// Create a new Kubo instance (does not start the container).
pub fn new_kubo(name: String, path: PathBuf) -> Result<Kubo> {
    let docker = Docker::connect_with_socket_defaults()?;
    Ok(Kubo {
        name,
        path,
        container_id: None,
        docker,
        active_sessions: HashSet::new(),
        last_session_close: None,
    })
}

/// List all kubo directories under the kubos dir.
pub fn list_kubo_dirs(kubos_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut kubos = Vec::new();
    if let Ok(entries) = std::fs::read_dir(kubos_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if let Some(base) = name.strip_suffix(".kubo") {
                        kubos.push((base.to_string(), path));
                    }
                }
            }
        }
    }
    kubos
}

/// Check if a Docker image exists locally.
async fn image_exists(docker: &Docker, image: &str) -> bool {
    docker.inspect_image(image).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_kubo_dir_creates_structure() {
        let dir = std::env::temp_dir().join("abot-kubo-test");
        let _ = std::fs::remove_dir_all(&dir);

        let path = Kubo::ensure_kubo_dir(&dir, "test").unwrap();
        assert!(path.exists());
        assert!(path.join("manifest.json").exists());

        let manifest = Kubo::read_manifest(&path).unwrap();
        assert_eq!(manifest.name, "test");
        assert_eq!(manifest.version, 1);

        // credentials.json should be created with empty object
        let creds_path = path.join("credentials.json");
        assert!(creds_path.exists());
        let contents = std::fs::read_to_string(&creds_path).unwrap();
        assert_eq!(contents, "{}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&creds_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_kubo_manifest_roundtrip() {
        let dir = std::env::temp_dir().join("abot-kubo-manifest-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let manifest = KuboManifest {
            version: 1,
            name: "test-kubo".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: Some("2026-01-01T00:00:00Z".to_string()),
            abots: vec!["alice".to_string(), "bob".to_string()],
        };
        Kubo::write_manifest(&dir, &manifest).unwrap();

        let read_back = Kubo::read_manifest(&dir).unwrap();
        assert_eq!(read_back.name, "test-kubo");
        assert_eq!(read_back.abots, vec!["alice", "bob"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_kubo_dirs() {
        let dir = std::env::temp_dir().join("abot-list-kubos-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work.kubo")).unwrap();
        std::fs::create_dir_all(dir.join("ml.kubo")).unwrap();
        std::fs::create_dir_all(dir.join("not-a-kubo")).unwrap();

        let kubos = list_kubo_dirs(&dir);
        assert_eq!(kubos.len(), 2);
        let names: Vec<&str> = kubos.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"work"));
        assert!(names.contains(&"ml"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_abot_workdir() {
        assert_eq!(Kubo::abot_workdir("alice"), "/home/abots/alice/home");
    }

    #[test]
    fn test_idle_timeout() {
        let docker = Docker::connect_with_socket_defaults().unwrap();
        let mut kubo = Kubo {
            name: "test".to_string(),
            path: PathBuf::from("/tmp"),
            container_id: None,
            docker,
            active_sessions: HashSet::new(),
            last_session_close: None,
        };

        // No sessions, no last close → don't stop
        assert!(!kubo.should_idle_stop());

        // Active session → don't stop
        kubo.session_opened("s1");
        assert!(!kubo.should_idle_stop());

        // Closed recently → don't stop
        kubo.session_closed("s1");
        assert!(!kubo.should_idle_stop());

        // Closed long ago → stop
        kubo.last_session_close =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(600));
        assert!(kubo.should_idle_stop());
    }
}
