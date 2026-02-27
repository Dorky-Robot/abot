use std::collections::VecDeque;

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

    pub fn to_string(&self) -> String {
        let mut out = String::with_capacity(self.current_bytes);
        for item in &self.items {
            out.push_str(item);
        }
        out
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.items.clear();
        self.current_bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn bytes(&self) -> usize {
        self.current_bytes
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
    fn test_clear() {
        let mut buf = RingBuffer::new(10, 1024);
        buf.push("hello".into());
        buf.clear();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.bytes(), 0);
    }
}
