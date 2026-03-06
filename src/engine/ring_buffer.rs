use std::collections::VecDeque;
use std::fmt;

/// Front-evicting circular buffer for terminal output.
/// Ring buffer with max items and max bytes.
pub struct RingBuffer {
    items: VecDeque<String>,
    max_items: usize,
    max_bytes: usize,
    current_bytes: usize,
}

impl RingBuffer {
    pub fn new(max_items: usize, max_bytes: usize) -> Self {
        Self {
            items: VecDeque::new(),
            max_items,
            max_bytes,
            current_bytes: 0,
        }
    }

    pub fn push(&mut self, data: String) {
        self.current_bytes += data.len();
        self.items.push_back(data);
        self.evict();
    }

    fn evict(&mut self) {
        while self.items.len() > self.max_items || self.current_bytes > self.max_bytes {
            if let Some(removed) = self.items.pop_front() {
                self.current_bytes -= removed.len();
            } else {
                break;
            }
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.items.clear();
        self.current_bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.current_bytes
    }

    /// Seed the buffer with previously saved content (e.g. scrollback from disk).
    pub fn pre_populate(&mut self, data: String) {
        if !data.is_empty() {
            self.push(data);
        }
    }
}

impl fmt::Display for RingBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for item in &self.items {
            f.write_str(item)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_push() {
        let mut buf = RingBuffer::new(3, 1024);
        buf.push("a".into());
        buf.push("b".into());
        buf.push("c".into());
        assert_eq!(buf.to_string(), "abc");
        assert_eq!(buf.len(), 3);
    }

    #[test]
    fn test_eviction_by_count() {
        let mut buf = RingBuffer::new(2, 1024);
        buf.push("a".into());
        buf.push("b".into());
        buf.push("c".into());
        assert_eq!(buf.to_string(), "bc");
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn test_eviction_by_bytes() {
        let mut buf = RingBuffer::new(100, 5);
        buf.push("abc".into()); // 3 bytes
        buf.push("def".into()); // 6 bytes total, evict "abc"
        assert_eq!(buf.to_string(), "def");
    }

    #[test]
    fn test_pre_populate() {
        let mut buf = RingBuffer::new(10, 1024);
        buf.pre_populate("saved scrollback".into());
        assert_eq!(buf.to_string(), "saved scrollback");
        assert_eq!(buf.len(), 1);

        // New output appends after pre-populated content
        buf.push("new data".into());
        assert_eq!(buf.to_string(), "saved scrollbacknew data");
    }

    #[test]
    fn test_pre_populate_empty_is_noop() {
        let mut buf = RingBuffer::new(10, 1024);
        buf.pre_populate("".into());
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.bytes(), 0);
    }

    #[test]
    fn test_clear() {
        let mut buf = RingBuffer::new(10, 1024);
        buf.push("hello".into());
        buf.clear();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.bytes(), 0);
    }
}
