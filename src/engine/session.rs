use super::backend::SessionBackend;
use super::ring_buffer::RingBuffer;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::watch;

/// Global monotonic counter for session generations.
static GENERATION_COUNTER: AtomicU64 = AtomicU64::new(1);

const DEFAULT_MAX_BUFFER_ITEMS: usize = 5000;
const DEFAULT_MAX_BUFFER_BYTES: usize = 5 * 1024 * 1024; // 5MB

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SessionStatus {
    Running,
    Exited(u32),
}

pub struct Session {
    pub name: String,
    /// Watch channel for the session name — background tasks (output relay)
    /// subscribe to get the current name without locking.
    pub name_tx: watch::Sender<String>,
    pub backend: Box<dyn SessionBackend>,
    pub buffer: RingBuffer,
    pub status: SessionStatus,
    /// Per-session environment variables (e.g. credentials).
    /// Merged with global agent_env when creating the backend; session env wins on conflicts.
    pub env: HashMap<String, String>,
    /// Path to the backing `.abot` bundle directory, if saved.
    pub bundle_path: Option<PathBuf>,
    /// Whether the session has unsaved changes since last save.
    pub dirty: bool,
    /// Kubo this session belongs to.
    pub kubo: Option<String>,
    /// Monotonic generation — incremented on each session creation so stale
    /// output relays can detect they belong to an overwritten session.
    pub generation: u64,
}

impl Session {
    pub fn new(
        name: String,
        backend: Box<dyn SessionBackend>,
        env: HashMap<String, String>,
        bundle_path: Option<PathBuf>,
        kubo: Option<String>,
    ) -> Self {
        let (name_tx, _) = watch::channel(name.clone());
        Self {
            name,
            name_tx,
            backend,
            buffer: RingBuffer::new(DEFAULT_MAX_BUFFER_ITEMS, DEFAULT_MAX_BUFFER_BYTES),
            status: SessionStatus::Running,
            env,
            bundle_path,
            dirty: false,
            kubo,
            generation: GENERATION_COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn is_alive(&self) -> bool {
        self.status == SessionStatus::Running
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        if !self.is_alive() {
            anyhow::bail!("session '{}' is not alive", self.name);
        }
        self.backend.write(data)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.backend.resize(cols, rows)
    }

    pub fn get_buffer(&self) -> String {
        self.buffer.to_string()
    }

    pub fn mark_exited(&mut self, code: u32) {
        self.status = SessionStatus::Exited(code);
    }

    pub fn summary(&self) -> SessionSummary {
        let (alive, exit_code) = match self.status {
            SessionStatus::Running => (true, None),
            SessionStatus::Exited(code) => (false, Some(code)),
        };
        SessionSummary {
            name: self.name.clone(),
            alive,
            exit_code,
            buffer_items: self.buffer.len(),
            buffer_bytes: self.buffer.bytes(),
            env_keys: self.env.len(),
            bundle_path: self
                .bundle_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            dirty: self.dirty,
            kubo: self.kubo.clone(),
        }
    }
}

/// Typed summary of a session, suitable for JSON serialization.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub name: String,
    pub alive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    pub buffer_items: usize,
    pub buffer_bytes: usize,
    pub env_keys: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_path: Option<String>,
    pub dirty: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubo: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stub backend for tests.
    struct StubBackend;
    impl SessionBackend for StubBackend {
        fn write(&mut self, _data: &[u8]) -> Result<()> {
            Ok(())
        }
        fn resize(&mut self, _cols: u16, _rows: u16) -> Result<()> {
            Ok(())
        }
        fn take_reader(&mut self) -> Option<tokio::sync::mpsc::Receiver<String>> {
            None
        }
        fn kill(&mut self) {}
        fn try_exit_code(&mut self) -> Option<u32> {
            None
        }
    }

    #[test]
    fn watch_name_tracks_rename() {
        let mut session = Session::new(
            "original".into(),
            Box::new(StubBackend),
            HashMap::new(),
            None,
            None,
        );
        let task_rx = session.name_tx.subscribe();

        // Background task would read this
        assert_eq!(*task_rx.borrow(), "original");

        // Simulate rename (as done in session_ops)
        session.name = "renamed".into();
        session.name_tx.send_replace("renamed".into());

        // Background task now reads the updated name
        assert_eq!(*task_rx.borrow(), "renamed");
    }

    #[test]
    fn session_lifecycle() {
        let mut session = Session::new(
            "test".into(),
            Box::new(StubBackend),
            HashMap::new(),
            None,
            None,
        );
        assert!(session.is_alive());
        assert_eq!(session.status, SessionStatus::Running);

        session.mark_exited(42);
        assert!(!session.is_alive());
        assert_eq!(session.status, SessionStatus::Exited(42));

        let s = session.summary();
        assert!(!s.alive);
        assert_eq!(s.exit_code, Some(42));
    }

    #[test]
    fn write_to_exited_session_fails() {
        let mut session = Session::new(
            "test".into(),
            Box::new(StubBackend),
            HashMap::new(),
            None,
            None,
        );
        session.mark_exited(0);
        assert!(session.write(b"hello").is_err());
    }

    #[test]
    fn new_session_starts_clean() {
        let session = Session::new(
            "s".into(),
            Box::new(StubBackend),
            HashMap::new(),
            None,
            None,
        );
        assert!(!session.dirty);
        assert!(session.bundle_path.is_none());
    }

    #[test]
    fn new_session_with_bundle_path() {
        let path = PathBuf::from("/tmp/test.abot");
        let session = Session::new(
            "s".into(),
            Box::new(StubBackend),
            HashMap::new(),
            Some(path.clone()),
            None,
        );
        assert_eq!(session.bundle_path, Some(path));
        assert!(!session.dirty);
    }

    #[test]
    fn summary_includes_bundle_path_and_dirty() {
        let path = PathBuf::from("/home/user/project.abot");
        let mut session = Session::new(
            "proj".into(),
            Box::new(StubBackend),
            HashMap::new(),
            Some(path),
            None,
        );
        let s = session.summary();
        assert_eq!(s.bundle_path.as_deref(), Some("/home/user/project.abot"));
        assert!(!s.dirty);

        session.dirty = true;
        let s = session.summary();
        assert!(s.dirty);
    }

    #[test]
    fn summary_bundle_path_none_when_not_set() {
        let session = Session::new(
            "s".into(),
            Box::new(StubBackend),
            HashMap::new(),
            None,
            None,
        );
        let s = session.summary();
        assert!(s.bundle_path.is_none());
        assert!(!s.dirty);
    }

    #[test]
    fn dirty_flag_is_mutable() {
        let mut session = Session::new(
            "s".into(),
            Box::new(StubBackend),
            HashMap::new(),
            None,
            None,
        );
        assert!(!session.dirty);
        session.dirty = true;
        assert!(session.dirty);
        session.dirty = false;
        assert!(!session.dirty);
    }

    #[test]
    fn summary_kubo_none_when_not_set() {
        let session = Session::new(
            "s".into(),
            Box::new(StubBackend),
            HashMap::new(),
            None,
            None,
        );
        let s = session.summary();
        assert!(s.kubo.is_none());
    }

    #[test]
    fn summary_kubo_present_when_set() {
        let session = Session::new(
            "s".into(),
            Box::new(StubBackend),
            HashMap::new(),
            None,
            Some("ml".into()),
        );
        let s = session.summary();
        assert_eq!(s.kubo.as_deref(), Some("ml"));
    }

    #[test]
    fn new_session_with_kubo() {
        let session = Session::new(
            "abot1".into(),
            Box::new(StubBackend),
            HashMap::new(),
            Some(PathBuf::from("/kubos/default.kubo/abot1")),
            Some("default".into()),
        );
        assert_eq!(session.kubo, Some("default".to_string()));
        assert!(session.is_alive());
        let s = session.summary();
        assert_eq!(s.kubo.as_deref(), Some("default"));
        assert_eq!(s.name, "abot1");
    }
}
