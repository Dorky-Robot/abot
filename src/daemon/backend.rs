use anyhow::Result;
use tokio::sync::mpsc;

/// Abstract interface for session backends (local PTY, Docker container, etc.)
///
/// Each backend manages a single interactive process with stdin/stdout and TTY support.
/// The daemon creates sessions via a backend, then manages I/O and lifecycle uniformly.
pub trait SessionBackend: Send {
    /// Write data to the process's stdin.
    fn write(&mut self, data: &[u8]) -> Result<()>;

    /// Resize the terminal.
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;

    /// Take the output reader channel. Called once after creation to wire up
    /// the output relay task. Returns None if already taken.
    fn take_reader(&mut self) -> Option<mpsc::Receiver<String>>;

    /// Terminate the process.
    fn kill(&mut self);

    /// Check if the process is still running.
    #[allow(dead_code)]
    fn is_alive(&mut self) -> bool;

    /// Try to retrieve the exit code without blocking.
    /// Returns Some(code) if the process has exited, None if still running or unknown.
    fn try_exit_code(&mut self) -> Option<u32>;

    /// Inject environment variables into a running session.
    /// Default implementation is a no-op (local PTY sessions inherit from parent).
    fn inject_env(&self, _env: &std::collections::HashMap<String, String>) {}
}
