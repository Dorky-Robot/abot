use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use tokio::sync::mpsc;

use super::backend::SessionBackend;

/// Spawn a PTY process and return handles for I/O.
pub struct PtyHandle {
    pub writer: Box<dyn std::io::Write + Send>,
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    reader_rx: Option<mpsc::Receiver<String>>,
    reader_handle: Option<std::thread::JoinHandle<()>>,
    master: Box<dyn portable_pty::MasterPty + Send>,
}

impl PtyHandle {
    pub fn spawn(shell: &str, cols: u16, rows: u16, home: &str) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-l"); // Login shell
        cmd.cwd(home);

        // Build a safe environment — inherit parent env but strip secrets
        // and vars that cause problems (like CLAUDECODE which makes nested
        // Claude Code sessions think they're inside another session).
        let filtered: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| !is_filtered_env(k))
            .collect();
        cmd.env_clear();
        for (k, v) in filtered {
            cmd.env(k, v);
        }

        // Override specific vars
        cmd.env("TERM", "xterm-256color");
        cmd.env("TERM_PROGRAM", "abot");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("HOME", home);
        cmd.env(
            "LANG",
            std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into()),
        );

        let child = pair.slave.spawn_command(cmd)?;
        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;
        let master = pair.master;

        // Read PTY output in a blocking thread, send via channel
        let (tx, rx) = mpsc::channel::<String>(256);
        let reader_handle = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        if tx.blocking_send(data).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            writer,
            child,
            reader_rx: Some(rx),
            reader_handle: Some(reader_handle),
            master,
        })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        use std::io::Write;
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn is_alive(&mut self) -> bool {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .unwrap_or(false)
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

/// Env vars to strip from the child PTY environment.
/// Prevents secrets leaking to shell commands and avoids
/// nested-session detection from tools like Claude Code.
fn is_filtered_env(key: &str) -> bool {
    matches!(
        key,
        "CLAUDECODE"
            | "CLAUDE_CODE"
            | "CLAUDE_API_KEY"
            | "ANTHROPIC_API_KEY"
            | "SSH_PASSWORD"
            | "SETUP_TOKEN"
            | "ABOT_NO_AUTH"
    ) || key.starts_with("CLAUDE_CODE_")
}

impl SessionBackend for PtyHandle {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        PtyHandle::write(self, data)
    }

    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        PtyHandle::resize(self, cols, rows)
    }

    fn take_reader(&mut self) -> Option<mpsc::Receiver<String>> {
        self.reader_rx.take()
    }

    fn kill(&mut self) {
        PtyHandle::kill(self);
    }

    fn is_alive(&mut self) -> bool {
        PtyHandle::is_alive(self)
    }
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        self.kill();
        if let Some(handle) = self.reader_handle.take() {
            let _ = handle.join();
        }
    }
}
