//! Docker container backend for sessions.
//!
//! Each session spawns a container from the `abot-session` image (or `alpine:3` fallback).
//! The container runs a shell with TTY attached, stdin/stdout piped via bollard.

use anyhow::Result;
use bollard::container::{
    AttachContainerOptions, Config, CreateContainerOptions, RemoveContainerOptions,
    ResizeContainerTtyOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use bollard::Docker;
use futures_util::{StreamExt, TryStreamExt};
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};

use super::backend::SessionBackend;

const SESSION_IMAGE: &str = "abot-session";
const FALLBACK_IMAGE: &str = "alpine:3";

pub struct DockerBackend {
    docker: Docker,
    container_id: String,
    /// Sender half of stdin pipe to the container
    stdin_tx: Arc<Mutex<Option<Pin<Box<dyn AsyncWrite + Send>>>>>,
    /// Receiver half of stdout/stderr from the container
    reader_rx: Option<mpsc::Receiver<String>>,
}

impl DockerBackend {
    /// Check if Docker daemon is reachable
    pub async fn is_available() -> bool {
        match Docker::connect_with_socket_defaults() {
            Ok(docker) => docker.ping().await.is_ok(),
            Err(_) => false,
        }
    }

    /// Spawn a new container with TTY and return a DockerBackend.
    /// `home_bind` is the host path to bind-mount as `/home/dev` in the container.
    pub async fn spawn(
        name: &str,
        cols: u16,
        rows: u16,
        env: Vec<String>,
        home_bind: &std::path::Path,
    ) -> Result<Self> {
        let docker = Docker::connect_with_socket_defaults()?;

        // Try abot-session image first, fall back to alpine
        let image = if image_exists(&docker, SESSION_IMAGE).await {
            SESSION_IMAGE
        } else {
            // Pull fallback image if needed
            if !image_exists(&docker, FALLBACK_IMAGE).await {
                docker
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
            FALLBACK_IMAGE
        };

        // Merge default env vars with caller-supplied vars
        let mut env_vars = vec![
            "TERM=xterm-256color".to_string(),
            "COLORTERM=truecolor".to_string(),
            "LANG=en_US.UTF-8".to_string(),
        ];
        env_vars.extend(env);

        // Use bash for abot-session (which has it), /bin/sh for fallback Alpine
        let shell = if image == SESSION_IMAGE {
            "/bin/bash"
        } else {
            "/bin/sh"
        };

        // Create container with TTY and resource limits
        let config = Config {
            image: Some(image.to_string()),
            tty: Some(true),
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            open_stdin: Some(true),
            cmd: Some(vec![shell.to_string(), "-l".to_string()]),
            env: Some(env_vars),
            user: Some("1000:1000".to_string()), // Run as non-root
            host_config: Some(HostConfig {
                memory: Some(512 * 1024 * 1024),         // 512 MB
                memory_swap: Some(512 * 1024 * 1024),    // No swap
                cpu_period: Some(100_000),               // 100ms period
                cpu_quota: Some(50_000),                 // 50% of one CPU
                pids_limit: Some(256),                   // Max 256 processes
                cap_drop: Some(vec!["ALL".to_string()]), // Drop all capabilities
                security_opt: Some(vec!["no-new-privileges".to_string()]),
                readonly_rootfs: Some(false), // Writable for shell use
                mounts: Some(vec![Mount {
                    target: Some("/home/dev".to_string()),
                    source: Some(home_bind.to_string_lossy().to_string()),
                    typ: Some(MountTypeEnum::BIND),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let container = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: format!("abot-{name}"),
                    ..Default::default()
                }),
                config,
            )
            .await?;

        let container_id = container.id;

        // Start container
        docker
            .start_container::<String>(&container_id, None)
            .await?;

        // Resize TTY to requested dimensions
        docker
            .resize_container_tty(
                &container_id,
                ResizeContainerTtyOptions {
                    width: cols,
                    height: rows,
                },
            )
            .await?;

        // Attach to container stdin/stdout
        let attach_results = docker
            .attach_container(
                &container_id,
                Some(AttachContainerOptions::<String> {
                    stdin: Some(true),
                    stdout: Some(true),
                    stderr: Some(true),
                    stream: Some(true),
                    ..Default::default()
                }),
            )
            .await?;

        let mut output = attach_results.output;
        let input = attach_results.input;
        let stdin_tx = Arc::new(Mutex::new(Some(input)));

        // Read container output in a task, send via mpsc channel
        let (tx, rx) = mpsc::channel::<String>(256);
        tokio::spawn(async move {
            while let Some(Ok(chunk)) = output.next().await {
                let data = String::from_utf8_lossy(&chunk.into_bytes()).to_string();
                if tx.send(data).await.is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            docker,
            container_id,
            stdin_tx,
            reader_rx: Some(rx),
        })
    }
}

impl SessionBackend for DockerBackend {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        let stdin_tx = self.stdin_tx.clone();
        // Spawn a task to write asynchronously since the trait method is sync
        tokio::spawn(async move {
            let mut guard = stdin_tx.lock().await;
            if let Some(ref mut input) = *guard {
                let _ = input.write_all(&data).await;
                let _ = input.flush().await;
            }
        });
        Ok(())
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        let docker = self.docker.clone();
        let id = self.container_id.clone();
        tokio::spawn(async move {
            let _ = docker
                .resize_container_tty(
                    &id,
                    ResizeContainerTtyOptions {
                        width: cols,
                        height: rows,
                    },
                )
                .await;
        });
        Ok(())
    }

    fn take_reader(&mut self) -> Option<mpsc::Receiver<String>> {
        self.reader_rx.take()
    }

    fn kill(&mut self) {
        let docker = self.docker.clone();
        let id = self.container_id.clone();
        tokio::spawn(async move {
            let _ = docker
                .remove_container(
                    &id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
        });
    }

    fn is_alive(&mut self) -> bool {
        // Can't easily check synchronously. Assume alive until reader closes.
        true
    }

    fn try_exit_code(&mut self) -> Option<u32> {
        // Docker backend cannot easily get exit code synchronously.
        None
    }

    fn inject_env(&self, env: &std::collections::HashMap<String, String>) {
        if env.is_empty() {
            return;
        }
        // Write export statements to /home/dev/.abot_env, then source it in .bashrc.
        // Also ensure Claude Code's config directory + settings exist so the
        // interactive wizard is skipped.
        let mut script = String::new();
        for (k, v) in env {
            let escaped = v.replace('\'', "'\\''");
            script.push_str(&format!("export {k}='{escaped}'\n"));
        }
        let docker = self.docker.clone();
        let id = self.container_id.clone();
        tokio::spawn(async move {
            use bollard::exec::{CreateExecOptions, StartExecOptions};
            let cmd = format!(
                "mkdir -p ~/.claude && \
                 [ -f ~/.claude/settings.json ] || echo '{{\"hasCompletedOnboarding\":true,\"hasCompletedAuthFlow\":true}}' > ~/.claude/settings.json && \
                 printf '%s' '{}' > /home/dev/.abot_env && \
                 grep -q 'source.*abot_env' /home/dev/.bashrc 2>/dev/null || \
                 echo '[ -f ~/.abot_env ] && source ~/.abot_env' >> /home/dev/.bashrc",
                script.replace('\'', "'\\''")
            );
            let write_exec = docker
                .create_exec(
                    &id,
                    CreateExecOptions {
                        cmd: Some(vec!["/bin/sh", "-c", &cmd]),
                        user: Some("1000:1000"),
                        ..Default::default()
                    },
                )
                .await;
            if let Ok(exec) = write_exec {
                let _ = docker
                    .start_exec(
                        &exec.id,
                        Some(StartExecOptions {
                            detach: true,
                            ..Default::default()
                        }),
                    )
                    .await;
            }
        });
    }
}

impl Drop for DockerBackend {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Check if a Docker image exists locally
async fn image_exists(docker: &Docker, image: &str) -> bool {
    docker.inspect_image(image).await.is_ok()
}
