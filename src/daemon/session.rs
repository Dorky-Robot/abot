use super::backend::SessionBackend;
use super::ring_buffer::RingBuffer;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const DEFAULT_MAX_BUFFER_ITEMS: usize = 5000;
const DEFAULT_MAX_BUFFER_BYTES: usize = 5 * 1024 * 1024; // 5MB

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SessionStatus {
    Running,
    Exited(u32),
}

pub struct Session {
    pub name: String,
    /// Shared name that background tasks (output relay) can read.
    /// Updated on rename so relay tasks always broadcast the current name.
    pub shared_name: Arc<Mutex<String>>,
    pub backend: Box<dyn SessionBackend>,
    pub buffer: RingBuffer,
    pub status: SessionStatus,
    /// Per-session environment variables (e.g. credentials).
    /// Merged with global agent_env when creating the backend; session env wins on conflicts.
    pub env: HashMap<String, String>,
}

impl Session {
    pub fn new(
        name: String,
        backend: Box<dyn SessionBackend>,
        env: HashMap<String, String>,
    ) -> Self {
        let shared_name = Arc::new(Mutex::new(name.clone()));
        Self {
            name,
            shared_name,
            backend,
            buffer: RingBuffer::new(DEFAULT_MAX_BUFFER_ITEMS, DEFAULT_MAX_BUFFER_BYTES),
            status: SessionStatus::Running,
            env,
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

    pub fn to_json(&self) -> serde_json::Value {
        let (alive, exit_code) = match self.status {
            SessionStatus::Running => (true, None),
            SessionStatus::Exited(code) => (false, Some(code)),
        };
        serde_json::json!({
            "name": self.name,
            "alive": alive,
            "exitCode": exit_code,
            "bufferItems": self.buffer.len(),
            "bufferBytes": self.buffer.bytes(),
            "envKeys": self.env.len(),
        })
    }
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
        fn is_alive(&mut self) -> bool {
            true
        }
        fn try_exit_code(&mut self) -> Option<u32> {
            None
        }
    }

    #[test]
    fn shared_name_tracks_rename() {
        let mut session = Session::new("original".into(), Box::new(StubBackend), HashMap::new());
        let task_name = session.shared_name.clone();

        // Background task would read this
        assert_eq!(*task_name.lock().unwrap(), "original");

        // Simulate rename (as done in handle_request)
        session.name = "renamed".into();
        *session.shared_name.lock().unwrap() = "renamed".into();

        // Background task now reads the updated name
        assert_eq!(*task_name.lock().unwrap(), "renamed");
    }

    #[test]
    fn session_lifecycle() {
        let mut session = Session::new("test".into(), Box::new(StubBackend), HashMap::new());
        assert!(session.is_alive());
        assert_eq!(session.status, SessionStatus::Running);

        session.mark_exited(42);
        assert!(!session.is_alive());
        assert_eq!(session.status, SessionStatus::Exited(42));

        let json = session.to_json();
        assert_eq!(json["alive"], false);
        assert_eq!(json["exitCode"], 42);
    }

    #[test]
    fn write_to_exited_session_fails() {
        let mut session = Session::new("test".into(), Box::new(StubBackend), HashMap::new());
        session.mark_exited(0);
        assert!(session.write(b"hello").is_err());
    }
}
