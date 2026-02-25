use super::pty::PtyHandle;
use super::ring_buffer::RingBuffer;
use anyhow::Result;
use tokio::sync::mpsc;

const DEFAULT_MAX_BUFFER_ITEMS: usize = 5000;
const DEFAULT_MAX_BUFFER_BYTES: usize = 5 * 1024 * 1024; // 5MB

pub struct Session {
    pub name: String,
    pub pty: PtyHandle,
    pub buffer: RingBuffer,
    pub alive: bool,
    pub exit_code: Option<u32>,
}

impl Session {
    pub fn new(name: String, pty: PtyHandle) -> Self {
        Self {
            name,
            pty,
            buffer: RingBuffer::new(DEFAULT_MAX_BUFFER_ITEMS, DEFAULT_MAX_BUFFER_BYTES),
            alive: true,
            exit_code: None,
        }
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        if !self.alive {
            anyhow::bail!("session '{}' is not alive", self.name);
        }
        self.pty.write(data)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.pty.resize(cols, rows)
    }

    pub fn get_buffer(&self) -> String {
        self.buffer.to_string()
    }

    pub fn mark_exited(&mut self, code: u32) {
        self.alive = false;
        self.exit_code = Some(code);
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "alive": self.alive,
            "exitCode": self.exit_code,
            "bufferItems": self.buffer.len(),
            "bufferBytes": self.buffer.bytes(),
        })
    }
}
