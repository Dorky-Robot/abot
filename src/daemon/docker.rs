//! Docker container backend for sessions.
//!
//! Each session spawns a container from the `abot-session` image (or `alpine:3` fallback).
//! The container runs a shell with TTY attached, stdin/stdout piped via bollard.

use anyhow::Result;
use bollard::container::{
    AttachContainerOptions, Config, CreateContainerOptions, RemoveContainerOptions,
    ResizeContainerTtyOptions,
};
use bollard::models::HostConfig;
use bollard::image::CreateImageOptions;
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

    /// Spawn a new container with TTY and return a DockerBackend
    pub async fn spawn(cols: u16, rows: u16) -> Result<Self> {
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

        // Create container with TTY and resource limits
        let config = Config {
            image: Some(image.to_string()),
            tty: Some(true),
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            open_stdin: Some(true),
            cmd: Some(vec!["/bin/sh".to_string(), "-l".to_string()]),
            env: Some(vec![
                "TERM=xterm-256color".to_string(),
                "COLORTERM=truecolor".to_string(),
                "LANG=en_US.UTF-8".to_string(),
            ]),
            host_config: Some(HostConfig {
                memory: Some(512 * 1024 * 1024),        // 512 MB
                memory_swap: Some(512 * 1024 * 1024),   // No swap
                cpu_period: Some(100_000),               // 100ms period
                cpu_quota: Some(50_000),                 // 50% of one CPU
                pids_limit: Some(256),                   // Max 256 processes
                ..Default::default()
            }),
            ..Default::default()
        };

        let container = docker
            .create_container(
                Some(CreateContainerOptions::<String> {
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
