use super::backend::SessionBackend;
use super::ring_buffer::RingBuffer;
use anyhow::Result;

const DEFAULT_MAX_BUFFER_ITEMS: usize = 5000;
const DEFAULT_MAX_BUFFER_BYTES: usize = 5 * 1024 * 1024; // 5MB

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SessionStatus {
    Running,
    Exited(u32),
}

pub struct Session {
    pub name: String,
    pub backend: Box<dyn SessionBackend>,
    pub buffer: RingBuffer,
    pub status: SessionStatus,
}

impl Session {
    pub fn new(name: String, backend: Box<dyn SessionBackend>) -> Self {
        Self {
            name,
            backend,
            buffer: RingBuffer::new(DEFAULT_MAX_BUFFER_ITEMS, DEFAULT_MAX_BUFFER_BYTES),
            status: SessionStatus::Running,
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
        })
    }
}
