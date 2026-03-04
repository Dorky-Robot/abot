//! KuboExecBackend — a `SessionBackend` that runs via `docker exec` inside a kubo container.
//!
//! Instead of creating a new container per session, this backend creates an exec
//! instance inside an already-running kubo container. Multiple abots share the
//! same container but get their own PTY sessions with separate working directories.

use anyhow::Result;
use bollard::exec::{CreateExecOptions, ResizeExecOptions, StartExecOptions};
use bollard::Docker;
use futures_util::StreamExt;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};

use super::backend::SessionBackend;

type StdinWriter = Pin<Box<dyn AsyncWrite + Send>>;

pub struct KuboExecBackend {
    docker: Docker,
    container_id: String,
    exec_id: String,
    /// Sender half of stdin pipe to the exec session
    stdin_tx: Arc<Mutex<Option<StdinWriter>>>,
    /// Receiver half of stdout/stderr from the exec session
    reader_rx: Option<mpsc::Receiver<String>>,
}

impl KuboExecBackend {
    /// Spawn a new exec session inside a running kubo container.
    ///
    /// `container_id` — the kubo container to exec into
    /// `abot_name` — used to set the working directory to `/home/abots/{abot_name}/home`
    /// `cols`, `rows` — initial terminal dimensions
    /// `env` — environment variables to set
    pub async fn spawn(
        container_id: &str,
        abot_name: &str,
        cols: u16,
        rows: u16,
        env: Vec<String>,
    ) -> Result<Self> {
        let docker = Docker::connect_with_socket_defaults()?;
        let workdir = super::kubo::Kubo::abot_workdir(abot_name);

        let mut exec_env = vec![
            "TERM=xterm-256color".to_string(),
            "COLORTERM=truecolor".to_string(),
            "LANG=en_US.UTF-8".to_string(),
            format!("HOME=/home/abots/{}/home", abot_name),
            format!(
                "PATH=/home/abots/{}/home/.local/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                abot_name
            ),
        ];
        exec_env.extend(env);

        let exec = docker
            .create_exec(
                container_id,
                CreateExecOptions {
                    cmd: Some(vec![
                        "/bin/sh",
                        "-lc",
                        "exec bash -l 2>/dev/null || exec sh -l",
                    ]),
                    tty: Some(true),
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    env: Some(exec_env.iter().map(|s| s.as_str()).collect()),
                    working_dir: Some(&workdir),
                    user: Some("1000:1000"),
                    ..Default::default()
                },
            )
            .await?;

        let exec_id = exec.id.clone();

        let attach = docker
            .start_exec(
                &exec_id,
                Some(StartExecOptions {
                    detach: false,
                    tty: true,
                    ..Default::default()
                }),
            )
            .await?;

        let (tx, rx) = mpsc::channel::<String>(256);

        match attach {
            bollard::exec::StartExecResults::Attached { mut output, input } => {
                let stdin_tx = Arc::new(Mutex::new(Some(input)));

                // Relay output
                tokio::spawn(async move {
                    while let Some(Ok(chunk)) = output.next().await {
                        let data = String::from_utf8_lossy(&chunk.into_bytes()).to_string();
                        if tx.send(data).await.is_err() {
                            break;
                        }
                    }
                });

                // Resize to initial dimensions
                let _ = docker
                    .resize_exec(
                        &exec_id,
                        ResizeExecOptions {
                            width: cols,
                            height: rows,
                        },
                    )
                    .await;

                Ok(Self {
                    docker,
                    container_id: container_id.to_string(),
                    exec_id,
                    stdin_tx,
                    reader_rx: Some(rx),
                })
            }
            bollard::exec::StartExecResults::Detached => {
                anyhow::bail!("exec started in detached mode unexpectedly")
            }
        }
    }
}

impl SessionBackend for KuboExecBackend {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        let stdin_tx = self.stdin_tx.clone();
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
        let exec_id = self.exec_id.clone();
        tokio::spawn(async move {
            let _ = docker
                .resize_exec(
                    &exec_id,
                    ResizeExecOptions {
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
        // Close stdin to signal the exec to terminate.
        // We do NOT remove the kubo container — only the exec session ends.
        let stdin_tx = self.stdin_tx.clone();
        tokio::spawn(async move {
            let mut guard = stdin_tx.lock().await;
            *guard = None; // Drop the stdin writer
        });
    }

    fn is_alive(&mut self) -> bool {
        // Cannot easily check synchronously. Assume alive until reader closes.
        true
    }

    fn try_exit_code(&mut self) -> Option<u32> {
        None
    }

    fn inject_env(&self, env: &std::collections::HashMap<String, String>) {
        if env.is_empty() {
            return;
        }
        let mut script = String::new();
        for (k, v) in env {
            if !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                || k.starts_with(|c: char| c.is_ascii_digit())
                || k.is_empty()
            {
                continue;
            }
            let escaped = v.replace('\'', "'\\''");
            script.push_str(&format!("export {k}='{escaped}'\n"));
        }
        let docker = self.docker.clone();
        let container_id = self.container_id.clone();
        tokio::spawn(async move {
            use bollard::exec::{CreateExecOptions, StartExecOptions};
            let cmd = format!(
                "mkdir -p ~/.claude && \
                 [ -f ~/.claude/settings.json ] || echo '{{\"hasCompletedOnboarding\":true,\"hasCompletedAuthFlow\":true}}' > ~/.claude/settings.json && \
                 printf '%s' '{}' > ~/.abot_env && \
                 grep -q 'source.*abot_env' ~/.bashrc 2>/dev/null || \
                 echo '[ -f ~/.abot_env ] && source ~/.abot_env' >> ~/.bashrc",
                script.replace('\'', "'\\''")
            );
            let write_exec = docker
                .create_exec(
                    &container_id,
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
