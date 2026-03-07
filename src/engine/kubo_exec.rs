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
    /// True when connected via tmux control mode (`-C`). In control mode,
    /// stdin carries tmux commands (send-keys, refresh-client) instead of raw bytes,
    /// and stdout carries `%output` protocol lines instead of terminal data.
    control_mode: bool,
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
    // Run all tmux option commands in parallel — each is independent.
    let cmd1 = ["tmux", "set-option", "-t", session, "status", "off"];
    let cmd2 = ["tmux", "set-option", "-t", session, "window-size", "latest"];
    let cmd3 = [
        "tmux",
        "set-option",
        "-t",
        session,
        "aggressive-resize",
        "on",
    ];
    let cmd4 = ["tmux", "unbind-key", "\""];
    let cmd5 = ["tmux", "unbind-key", "%"];
    let _ = tokio::join!(
        exec_cmd(docker, container_id, &cmd1),
        exec_cmd(docker, container_id, &cmd2),
        exec_cmd(docker, container_id, &cmd3),
        exec_cmd(docker, container_id, &cmd4),
        exec_cmd(docker, container_id, &cmd5),
    );
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

/// Unescape tmux control mode octal encoding.
/// tmux replaces chars < ASCII 32 and `\` with octal: `\015` for CR, `\012` for LF, `\134` for `\`.
fn unescape_tmux_output(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            let d0 = bytes[i + 1];
            let d1 = bytes[i + 2];
            let d2 = bytes[i + 3];
            if (b'0'..=b'7').contains(&d0)
                && (b'0'..=b'7').contains(&d1)
                && (b'0'..=b'7').contains(&d2)
            {
                let val = (d0 - b'0') as u16 * 64 + (d1 - b'0') as u16 * 8 + (d2 - b'0') as u16;
                if val <= 255 {
                    result.push(val as u8);
                    i += 4;
                    continue;
                }
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).to_string()
}

/// Encode bytes as hex pairs for `tmux send-keys -H`.
fn encode_hex_keys(data: &[u8]) -> String {
    let mut hex = String::with_capacity(data.len() * 3);
    for (i, byte) in data.iter().enumerate() {
        if i > 0 {
            hex.push(' ');
        }
        hex.push_str(&format!("{:02x}", byte));
    }
    hex
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

/// Spawn the persistent stdin writer task. Returns the sender half of the channel.
/// The task drains the channel and writes to Docker stdin. Errors close the writer,
/// causing future sends to fail with `TrySendError::Closed`.
fn spawn_stdin_writer(stdin: Arc<Mutex<Option<StdinWriter>>>) -> mpsc::Sender<Vec<u8>> {
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(64);
    tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            let mut guard = stdin.lock().await;
            if let Some(ref mut writer) = *guard {
                if writer.write_all(&data).await.is_err() || writer.flush().await.is_err() {
                    *guard = None;
                    break;
                }
            } else {
                break;
            }
        }
    });
    tx
}

/// Send data through the stdin channel, mapping errors to anyhow.
fn try_send_stdin(chan: &mpsc::Sender<Vec<u8>>, data: Vec<u8>) -> Result<()> {
    chan.try_send(data).map_err(|e| match e {
        mpsc::error::TrySendError::Full(_) => anyhow::anyhow!("stdin buffer full (input dropped)"),
        mpsc::error::TrySendError::Closed(_) => {
            anyhow::anyhow!("stdin channel closed (container may be dead)")
        }
    })
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
                let stdin_chan_tx = spawn_stdin_writer(stdin_closer.clone());

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
                    control_mode: false,
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

    /// Attach to a docker exec running `tmux -C attach`, parsing the control
    /// mode protocol. No TTY is allocated — all I/O is text-based commands.
    async fn attach_control_mode(
        docker: &Docker,
        container_id: &str,
        exec_id: &str,
        tmux_session: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Self> {
        let attach = docker
            .start_exec(
                exec_id,
                Some(StartExecOptions {
                    detach: false,
                    ..Default::default()
                }),
            )
            .await?;

        let (tx, rx) = mpsc::channel::<String>(256);

        match attach {
            bollard::exec::StartExecResults::Attached {
                mut output,
                mut input,
            } => {
                // Set control mode client size to match the terminal dimensions.
                // Scrollback restoration is handled by the engine's capture-pane
                // mechanism (not by control mode's %output stream).
                let init_cmds = format!("refresh-client -C {}x{}\n", cols, rows);
                if let Err(e) = input.write_all(init_cmds.as_bytes()).await {
                    tracing::warn!("control mode init write failed: {}", e);
                }
                if let Err(e) = input.flush().await {
                    tracing::warn!("control mode init flush failed: {}", e);
                }

                let stdin_closer: Arc<Mutex<Option<StdinWriter>>> =
                    Arc::new(Mutex::new(Some(input)));
                let stdin_chan_tx = spawn_stdin_writer(stdin_closer.clone());

                // Reader task: parse tmux control mode protocol lines.
                // Only `%output %pane_id data` lines carry terminal output.
                tokio::spawn(async move {
                    let mut line_buf = String::new();
                    while let Some(Ok(chunk)) = output.next().await {
                        let bytes = chunk.into_bytes();
                        let data = String::from_utf8_lossy(&bytes);
                        line_buf.push_str(&data);

                        while let Some(newline_pos) = line_buf.find('\n') {
                            let line = &line_buf[..newline_pos];

                            if let Some(rest) = line.strip_prefix("%output ") {
                                // Format: %output %pane_id octal_escaped_data
                                if let Some(space_pos) = rest.find(' ') {
                                    let escaped = &rest[space_pos + 1..];
                                    let unescaped = unescape_tmux_output(escaped);
                                    if tx.send(unescaped).await.is_err() {
                                        return;
                                    }
                                }
                            }
                            // Ignore %begin, %end, %error, %session-changed, etc.

                            // Drain the processed line from the buffer
                            line_buf = line_buf[newline_pos + 1..].to_string();
                        }
                    }
                });

                Ok(Self {
                    docker: docker.clone(),
                    container_id: container_id.to_string(),
                    exec_id: exec_id.to_string(),
                    tmux_name: Some(tmux_session.to_string()),
                    control_mode: true,
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
            tmux_resize(docker, container_id, tmux_name, cols, rows);
        }

        apply_tmux_session_options(docker, container_id, tmux_name).await;

        let exec = docker
            .create_exec(
                container_id,
                CreateExecOptions {
                    cmd: Some(vec![
                        "tmux",
                        "-u",
                        "-C",
                        "attach-session",
                        "-d",
                        "-t",
                        tmux_name,
                    ]),
                    tty: Some(false),
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    user: Some("1000:1000"),
                    ..Default::default()
                },
            )
            .await?;

        Self::attach_control_mode(docker, container_id, &exec.id, tmux_name, cols, rows).await
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
        let payload = if self.control_mode {
            // In control mode, wrap user input as a tmux send-keys command
            // with hex-encoded bytes to avoid any interpretation issues.
            if data.is_empty() {
                return Ok(());
            }
            let hex = encode_hex_keys(data);
            format!("send-keys -H {}\n", hex).into_bytes()
        } else {
            // Strip DA (Device Attributes) responses that xterm.js echoes back.
            // These look like ESC[?1;2c or ESC[>0;276;0c and can trigger tmux
            // keybindings if they reach the terminal stdin.
            let filtered = strip_da_responses(data);
            if filtered.is_empty() {
                return Ok(());
            }
            filtered
        };
        try_send_stdin(&self.stdin_chan, payload)
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        if self.control_mode {
            // In control mode, resize by telling tmux the client's terminal size.
            // tmux uses this (with window-size=latest) to resize the pane.
            let cmd = format!("refresh-client -C {}x{}\n", cols, rows);
            return try_send_stdin(&self.stdin_chan, cmd.into_bytes());
        }

        // TTY mode: resize both the Docker exec PTY and the tmux window.
        let docker = self.docker.clone();
        let exec_id = self.exec_id.clone();
        let container_id = self.container_id.clone();
        let tmux_name = self.tmux_name.clone();
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

    fn try_exit_code(&mut self) -> Option<u32> {
        None
    }

    fn restores_own_scrollback(&self) -> bool {
        // Control mode does NOT restore its own scrollback — refresh-client -S
        // only syncs the protocol stream, it doesn't resend existing screen
        // content for idle sessions. Let the engine's capture-pane mechanism
        // handle scrollback restoration.
        false
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

    // --- tmux control mode helpers ---

    #[test]
    fn unescape_cr_lf() {
        assert_eq!(unescape_tmux_output("hello\\015\\012"), "hello\r\n");
    }

    #[test]
    fn unescape_backslash() {
        assert_eq!(unescape_tmux_output("a\\134b"), "a\\b");
    }

    #[test]
    fn unescape_plain_text() {
        assert_eq!(unescape_tmux_output("hello world"), "hello world");
    }

    #[test]
    fn unescape_mixed() {
        // "$ ls\r\n" in octal: "$ ls\015\012"
        assert_eq!(unescape_tmux_output("$ ls\\015\\012"), "$ ls\r\n");
    }

    #[test]
    fn unescape_partial_octal_preserved() {
        // Not enough digits after backslash — preserved as-is
        assert_eq!(unescape_tmux_output("a\\01z"), "a\\01z");
    }

    #[test]
    fn unescape_rejects_non_octal_digits() {
        // 8 and 9 are not valid octal digits — preserved as-is
        assert_eq!(unescape_tmux_output("a\\189"), "a\\189");
    }

    #[test]
    fn encode_hex_simple() {
        assert_eq!(encode_hex_keys(b"hi"), "68 69");
    }

    #[test]
    fn encode_hex_control_chars() {
        assert_eq!(encode_hex_keys(b"\r"), "0d");
        assert_eq!(encode_hex_keys(b"\x1b[A"), "1b 5b 41");
    }

    #[test]
    fn encode_hex_empty() {
        assert_eq!(encode_hex_keys(b""), "");
    }
}
