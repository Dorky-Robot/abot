use super::backend::SessionBackend;
use super::ring_buffer::RingBuffer;
use anyhow::Result;

const DEFAULT_MAX_BUFFER_ITEMS: usize = 5000;
const DEFAULT_MAX_BUFFER_BYTES: usize = 5 * 1024 * 1024; // 5MB

pub struct Session {
    pub name: String,
    pub backend: Box<dyn SessionBackend>,
    pub buffer: RingBuffer,
    pub alive: bool,
    pub exit_code: Option<u32>,
}

impl Session {
    pub fn new(name: String, backend: Box<dyn SessionBackend>) -> Self {
        Self {
            name,
            backend,
            buffer: RingBuffer::new(DEFAULT_MAX_BUFFER_ITEMS, DEFAULT_MAX_BUFFER_BYTES),
            alive: true,
            exit_code: None,
        }
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        if !self.alive {
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

    #[allow(dead_code)]
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
