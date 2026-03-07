//! KuboExecBackend — a `SessionBackend` that runs via `docker exec` inside a kubo container.
//!
//! Instead of creating a new container per session, this backend creates an exec
//! instance inside an already-running kubo container. Multiple abots share the
//! same container but get their own PTY sessions with separate working directories.
//!
//! When tmux is available in the container, sessions are wrapped in tmux so the
//! shell survives server restarts and WebSocket disconnects.

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

/// Shell command to ensure Claude CLI settings exist, skipping interactive
/// onboarding and auth prompts (credentials are injected via env vars).
const CLAUDE_SETTINGS_INIT: &str = "mkdir -p ~/.claude && \
     [ -f ~/.claude/settings.json ] || \
     echo '{\"hasCompletedOnboarding\":true,\"hasCompletedAuthFlow\":true}' > ~/.claude/settings.json";

/// Check if an env var key is valid for shell export.
fn is_valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !key.starts_with(|c: char| c.is_ascii_digit())
}

/// Shell-escape a value for single-quoted assignment.
fn shell_escape(val: &str) -> String {
    val.replace('\'', "'\\''")
}

pub struct KuboExecBackend {
    docker: Docker,
    container_id: String,
    exec_id: String,
    /// tmux session name (if tmux-backed). Used for resize commands.
    tmux_name: Option<String>,
    /// Bounded channel for sending stdin data to the writer task.
    /// Errors on send mean the writer task is gone (pipe broken / container dead).
    stdin_chan: mpsc::Sender<Vec<u8>>,
    /// Handle to close stdin on kill.
    stdin_closer: Arc<Mutex<Option<StdinWriter>>>,
    /// Receiver half of stdout/stderr from the exec session
    reader_rx: Option<mpsc::Receiver<String>>,
}

/// Sanitize abot name for use as a tmux session name.
/// tmux disallows `.` and `:` in session names.
pub(crate) fn tmux_session_name(abot_name: &str) -> String {
    abot_name.replace(['.', ':'], "_")
}

/// Strip DA (Device Attributes) response sequences from stdin data.
/// xterm.js responds to DA queries (sent by tmux) with sequences like
/// `ESC[?1;2c` (DA1) and `ESC[>0;276;0c` (DA2). If these reach tmux stdin,
/// the trailing characters can trigger keybindings (e.g. `c` = new-window).
fn strip_da_responses(data: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(data);
    // DA1: ESC[?<digits;>c   DA2: ESC[><digits;>c
    let cleaned: String = {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                // Check for CSI: ESC[
                if chars.peek() == Some(&'[') {
                    chars.next(); // consume '['
                                  // Check for ? or > (DA1 / DA2 prefix)
                    if matches!(chars.peek(), Some('?' | '>')) {
                        let prefix = chars.next().unwrap();
                        // Consume parameter bytes (digits and ;), buffering them
                        // in case this turns out not to be a DA response.
                        let mut params = String::new();
                        let mut is_da = false;
                        loop {
                            match chars.peek() {
                                Some(&c) if c.is_ascii_digit() || c == ';' => {
                                    params.push(c);
                                    chars.next();
                                }
                                Some(&'c') => {
                                    chars.next(); // consume final 'c'
                                    is_da = true;
                                    break;
                                }
                                _ => break,
                            }
                        }
                        if !is_da {
                            // Not a DA response — replay the full sequence
                            result.push('\x1b');
                            result.push('[');
                            result.push(prefix);
                            result.push_str(&params);
                        }
                    } else {
                        // Not a DA response — preserve ESC[
                        result.push('\x1b');
                        result.push('[');
                    }
                } else {
                    result.push(ch);
                }
            } else {
                result.push(ch);
            }
        }
        result
    };
    cleaned.into_bytes()
}

/// Run a command inside a container and return (exit_code, stdout).
async fn exec_cmd(docker: &Docker, container_id: &str, cmd: &[&str]) -> Result<(i64, String)> {
    let exec = docker
        .create_exec(
            container_id,
            CreateExecOptions {
                cmd: Some(cmd.to_vec()),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                user: Some("1000:1000"),
                ..Default::default()
            },
        )
        .await?;

    let attach = docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                ..Default::default()
            }),
        )
        .await?;

    let mut output = String::new();
    if let bollard::exec::StartExecResults::Attached {
        output: mut stream, ..
    } = attach
    {
        while let Some(Ok(chunk)) = stream.next().await {
            output.push_str(&String::from_utf8_lossy(&chunk.into_bytes()));
        }
    }

    let inspect = docker.inspect_exec(&exec.id).await?;
    let exit_code = inspect.exit_code.unwrap_or(-1);
    Ok((exit_code, output))
}

/// Check if tmux is available in the container.
async fn check_tmux(docker: &Docker, container_id: &str) -> bool {
    match exec_cmd(docker, container_id, &["which", "tmux"]).await {
        Ok((code, _)) => code == 0,
        Err(_) => false,
    }
}

/// Check if a tmux session exists.
async fn tmux_has_session(docker: &Docker, container_id: &str, session: &str) -> bool {
    match exec_cmd(
        docker,
        container_id,
        &["tmux", "has-session", "-t", session],
    )
    .await
    {
        Ok((code, _)) => code == 0,
        Err(_) => false,
    }
}

/// Apply standard tmux session options: hide status bar, size window to latest
/// client, enable aggressive resize, and disable pane splits.
///
/// Called for both new sessions and when reattaching to existing ones (to cover
/// sessions created before these options were added).
async fn apply_tmux_session_options(docker: &Docker, container_id: &str, session: &str) {
    // Hide tmux status bar — the browser UI handles session identity.
    let _ = exec_cmd(
        docker,
        container_id,
        &["tmux", "set-option", "-t", session, "status", "off"],
    )
    .await;

    // Size the window to match the most recently active client, not the
    // smallest. Prevents dots when a new client attaches with a different size.
    let _ = exec_cmd(
        docker,
        container_id,
        &["tmux", "set-option", "-t", session, "window-size", "latest"],
    )
    .await;

    // Enable aggressive resize so panes resize immediately when clients change.
    let _ = exec_cmd(
        docker,
        container_id,
        &[
            "tmux",
            "set-option",
            "-t",
            session,
            "aggressive-resize",
            "on",
        ],
    )
    .await;

    // Disable tmux pane splits. Pane management through Docker's PTY
    // indirection causes resize artifacts (dots, garbled content).
    // Splits will be implemented as browser-side layout when needed.
    let _ = exec_cmd(docker, container_id, &["tmux", "unbind-key", "\""]).await;
    let _ = exec_cmd(docker, container_id, &["tmux", "unbind-key", "%"]).await;
}

/// Create a new tmux session (detached).
async fn tmux_new_session(
    docker: &Docker,
    container_id: &str,
    session: &str,
    cols: u16,
    rows: u16,
    env: &[String],
    abot_name: &str,
) -> Result<()> {
    let workdir = super::kubo::Kubo::abot_workdir(abot_name);

    // Build env export preamble for the shell inside tmux
    let mut env_script = String::new();
    for var in env {
        if let Some((k, v)) = var.split_once('=') {
            if !is_valid_env_key(k) {
                continue;
            }
            let escaped = shell_escape(v);
            env_script.push_str(&format!("export {k}='{escaped}'; "));
        }
    }

    let shell_cmd = format!(
        "{}cd {} 2>/dev/null; if command -v bash >/dev/null 2>&1; then exec bash -l; else exec sh -l; fi",
        env_script, workdir
    );

    let x = cols.to_string();
    let y = rows.to_string();

    exec_cmd(
        docker,
        container_id,
        &[
            "tmux",
            "new-session",
            "-d",
            "-s",
            session,
            "-x",
            &x,
            "-y",
            &y,
            "/bin/sh",
            "-c",
            &shell_cmd,
        ],
    )
    .await?;

    // Configure tmux session options
    let _ = exec_cmd(
        docker,
        container_id,
        &[
            "tmux",
            "set-option",
            "-t",
            session,
            "history-limit",
            "50000",
        ],
    )
    .await;

    // Keep default tmux prefix (Ctrl+B) for interactive use.
    // DA response filtering happens client-side (terminal_facet.dart) and
    // server-side (stdin writer strips DA responses before forwarding).

    apply_tmux_session_options(docker, container_id, session).await;

    Ok(())
}

/// Resize a tmux window (fire-and-forget).
fn tmux_resize(docker: &Docker, container_id: &str, session: &str, cols: u16, rows: u16) {
    let docker = docker.clone();
    let container_id = container_id.to_string();
    let session = session.to_string();
    let x = cols.to_string();
    let y = rows.to_string();
    tokio::spawn(async move {
        let _ = exec_cmd(
            &docker,
            &container_id,
            &["tmux", "resize-window", "-t", &session, "-x", &x, "-y", &y],
        )
        .await;
    });
}

/// Capture tmux scrollback (ANSI-colored).
pub async fn capture_scrollback(
    docker: &Docker,
    container_id: &str,
    session: &str,
) -> Option<String> {
    match exec_cmd(
        docker,
        container_id,
        &[
            "tmux",
            "capture-pane",
            "-t",
            session,
            "-p",
            "-e",
            "-S",
            "-5000",
        ],
    )
    .await
    {
        Ok((0, output)) if !output.trim().is_empty() => Some(output),
        _ => None,
    }
}

/// Kill a tmux session. Used when explicitly removing an abot from a kubo.
pub(crate) async fn tmux_kill_session(docker: &Docker, container_id: &str, session: &str) {
    let _ = exec_cmd(
        docker,
        container_id,
        &["tmux", "kill-session", "-t", session],
    )
    .await;
}

impl KuboExecBackend {
    /// Spawn a new exec session inside a running kubo container.
    ///
    /// If tmux is available, the session is wrapped in tmux for persistence.
    /// Falls back to a raw exec if tmux is unavailable or fails.
    pub async fn spawn(
        container_id: &str,
        abot_name: &str,
        cols: u16,
        rows: u16,
        env: Vec<String>,
    ) -> Result<Self> {
        let docker = Docker::connect_with_socket_defaults()?;
        let tmux_name = tmux_session_name(abot_name);

        let has_tmux = check_tmux(&docker, container_id).await;
        tracing::info!(
            "KuboExecBackend::spawn abot={}, container={}, tmux_available={}",
            abot_name,
            &container_id[..12.min(container_id.len())],
            has_tmux
        );

        if has_tmux {
            match Self::spawn_tmux(
                &docker,
                container_id,
                abot_name,
                &tmux_name,
                cols,
                rows,
                &env,
            )
            .await
            {
                Ok(backend) => {
                    tracing::info!(
                        "tmux session '{}' attached for abot '{}'",
                        tmux_name,
                        abot_name
                    );
                    return Ok(backend);
                }
                Err(e) => {
                    tracing::warn!(
                        "tmux spawn failed for '{}', falling back to raw exec: {}",
                        abot_name,
                        e
                    );
                }
            }
        }

        tracing::info!("using raw exec for abot '{}'", abot_name);
        Self::spawn_raw(&docker, container_id, abot_name, cols, rows, env).await
    }

    /// Attach to a docker exec, spawn the output relay, resize, and return Self.
    async fn attach_exec(
        docker: &Docker,
        container_id: &str,
        exec_id: &str,
        cols: u16,
        rows: u16,
        tmux_name: Option<String>,
    ) -> Result<Self> {
        let attach = docker
            .start_exec(
                exec_id,
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
                let stdin_closer = Arc::new(Mutex::new(Some(input)));

                // Persistent writer task: drains the channel and writes to Docker stdin.
                // Errors close the channel, causing future write() calls to fail.
                let (stdin_chan_tx, mut stdin_chan_rx) = mpsc::channel::<Vec<u8>>(64);
                {
                    let stdin_ref = stdin_closer.clone();
                    tokio::spawn(async move {
                        while let Some(data) = stdin_chan_rx.recv().await {
                            let mut guard = stdin_ref.lock().await;
                            if let Some(ref mut writer) = *guard {
                                if writer.write_all(&data).await.is_err()
                                    || writer.flush().await.is_err()
                                {
                                    *guard = None;
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    });
                }

                tokio::spawn(async move {
                    while let Some(Ok(chunk)) = output.next().await {
                        let data = String::from_utf8_lossy(&chunk.into_bytes()).to_string();
                        if tx.send(data).await.is_err() {
                            break;
                        }
                    }
                });

                let _ = docker
                    .resize_exec(
                        exec_id,
                        ResizeExecOptions {
                            width: cols,
                            height: rows,
                        },
                    )
                    .await;

                Ok(Self {
                    docker: docker.clone(),
                    container_id: container_id.to_string(),
                    exec_id: exec_id.to_string(),
                    tmux_name,
                    stdin_chan: stdin_chan_tx,
                    stdin_closer,
                    reader_rx: Some(rx),
                })
            }
            bollard::exec::StartExecResults::Detached => {
                anyhow::bail!("exec started in detached mode unexpectedly")
            }
        }
    }

    /// Spawn via tmux: create or reuse session, then attach.
    async fn spawn_tmux(
        docker: &Docker,
        container_id: &str,
        abot_name: &str,
        tmux_name: &str,
        cols: u16,
        rows: u16,
        env: &[String],
    ) -> Result<Self> {
        let has = tmux_has_session(docker, container_id, tmux_name).await;

        if !has {
            tmux_new_session(docker, container_id, tmux_name, cols, rows, env, abot_name).await?;
        } else {
            // Existing session — resize to match current dimensions
            tmux_resize(docker, container_id, tmux_name, cols, rows);
        }

        // Ensure options are set (covers sessions created before this change)
        apply_tmux_session_options(docker, container_id, tmux_name).await;

        let exec = docker
            .create_exec(
                container_id,
                CreateExecOptions {
                    cmd: Some(vec!["tmux", "attach-session", "-d", "-t", tmux_name]),
                    tty: Some(true),
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    user: Some("1000:1000"),
                    ..Default::default()
                },
            )
            .await?;

        Self::attach_exec(
            docker,
            container_id,
            &exec.id,
            cols,
            rows,
            Some(tmux_name.to_string()),
        )
        .await
    }

    /// Spawn a raw exec session (no tmux). Original behavior.
    async fn spawn_raw(
        docker: &Docker,
        container_id: &str,
        abot_name: &str,
        cols: u16,
        rows: u16,
        env: Vec<String>,
    ) -> Result<Self> {
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
                        "if command -v bash >/dev/null 2>&1; then exec bash -l; else exec sh -l; fi",
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

        Self::attach_exec(docker, container_id, &exec.id, cols, rows, None).await
    }
}

impl SessionBackend for KuboExecBackend {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        // Strip DA (Device Attributes) responses that xterm.js echoes back.
        // These look like ESC[?1;2c or ESC[>0;276;0c and can trigger tmux
        // keybindings if they reach the terminal stdin.
        let filtered = strip_da_responses(data);
        if filtered.is_empty() {
            return Ok(());
        }
        self.stdin_chan.try_send(filtered).map_err(|e| match e {
            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                anyhow::anyhow!("stdin buffer full (input dropped)")
            }
            tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                anyhow::anyhow!("stdin channel closed (container may be dead)")
            }
        })
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        let docker = self.docker.clone();
        let exec_id = self.exec_id.clone();
        let container_id = self.container_id.clone();
        let tmux_name = self.tmux_name.clone();
        tokio::spawn(async move {
            // Resize the Docker exec PTY
            let _ = docker
                .resize_exec(
                    &exec_id,
                    ResizeExecOptions {
                        width: cols,
                        height: rows,
                    },
                )
                .await;
            // Also resize the tmux window to match. Without this, tmux may
            // not detect the PTY size change through Docker's indirection,
            // leaving dots in the unused space.
            if let Some(ref session) = tmux_name {
                tmux_resize(&docker, &container_id, session, cols, rows);
            }
        });

        Ok(())
    }

    fn take_reader(&mut self) -> Option<mpsc::Receiver<String>> {
        self.reader_rx.take()
    }

    fn kill(&mut self) {
        // Close stdin to detach the docker exec from the tmux session.
        // The tmux session itself is left alive — that's the whole point
        // of tmux persistence. It gets cleaned up when the container stops.
        let stdin_closer = self.stdin_closer.clone();
        tokio::spawn(async move {
            let mut guard = stdin_closer.lock().await;
            *guard = None;
        });
    }

    fn is_alive(&mut self) -> bool {
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
            if !is_valid_env_key(k) {
                continue;
            }
            let escaped = shell_escape(v);
            script.push_str(&format!("export {k}='{escaped}'\n"));
        }
        let docker = self.docker.clone();
        let container_id = self.container_id.clone();
        tokio::spawn(async move {
            use bollard::exec::{CreateExecOptions, StartExecOptions};

            // Write env file via stdin + ensure Claude CLI is pre-configured
            // (skip onboarding/auth prompts since credentials are injected externally)
            let write_cmd = format!(
                "{} && cat > ~/.abot_env && \
                 grep -q 'source.*abot_env' ~/.bashrc 2>/dev/null || \
                 echo '[ -f ~/.abot_env ] && source ~/.abot_env' >> ~/.bashrc",
                CLAUDE_SETTINGS_INIT
            );
            let write_exec = docker
                .create_exec(
                    &container_id,
                    CreateExecOptions {
                        cmd: Some(vec!["/bin/sh", "-c", &write_cmd]),
                        attach_stdin: Some(true),
                        user: Some("1000:1000"),
                        ..Default::default()
                    },
                )
                .await;
            if let Ok(exec) = write_exec {
                let start_result = docker
                    .start_exec(
                        &exec.id,
                        Some(StartExecOptions {
                            detach: false,
                            ..Default::default()
                        }),
                    )
                    .await;
                if let Ok(bollard::exec::StartExecResults::Attached { input, .. }) = start_result {
                    let mut input = input;
                    let _ = input.write_all(script.as_bytes()).await;
                    let _ = input.shutdown().await;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_da1_response() {
        let input = b"\x1b[?1;2c";
        assert_eq!(strip_da_responses(input), b"");
    }

    #[test]
    fn strip_da2_response() {
        let input = b"\x1b[>0;276;0c";
        assert_eq!(strip_da_responses(input), b"");
    }

    #[test]
    fn strip_da_mixed_with_text() {
        let input = b"hello\x1b[?1;2cworld";
        assert_eq!(strip_da_responses(input), b"helloworld");
    }

    #[test]
    fn preserve_normal_input() {
        let input = b"ls -la\r\n";
        assert_eq!(strip_da_responses(input), input.to_vec());
    }

    #[test]
    fn preserve_ctrl_b() {
        let input = b"\x02";
        assert_eq!(strip_da_responses(input), input.to_vec());
    }

    #[test]
    fn preserve_other_csi_sequences() {
        let input = b"\x1b[1;3H";
        assert_eq!(strip_da_responses(input), input.to_vec());
    }

    #[test]
    fn strip_extended_da_response() {
        let input = b"\x1b[?64;1;2;6;9;15;16;17;18;21;22c";
        assert_eq!(strip_da_responses(input), b"");
    }

    #[test]
    fn preserve_csi_with_question_prefix() {
        // ESC[?25h (show cursor) — NOT a DA response, must be preserved intact
        let input = b"\x1b[?25h";
        assert_eq!(strip_da_responses(input), input.to_vec());
    }

    #[test]
    fn preserve_alt_screen_sequence() {
        // ESC[?1049h (alt screen) — NOT a DA response
        let input = b"\x1b[?1049h";
        assert_eq!(strip_da_responses(input), input.to_vec());
    }
}
