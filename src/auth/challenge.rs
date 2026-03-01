use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

const CHALLENGE_TTL_MS: u64 = 5 * 60 * 1000; // 5 minutes

/// In-memory challenge store with automatic TTL expiry.
/// Single-use challenge store.
pub struct ChallengeStore {
    inner: Arc<Mutex<HashMap<String, (serde_json::Value, u64)>>>,
}

impl ChallengeStore {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let store = Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        };

        // Spawn background sweep task
        let inner = store.inner.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let now = now_ms();
                let mut map = inner.lock().await;
                map.retain(|_, (_, expires)| *expires > now);
            }
        });

        store
    }

    pub async fn store(&self, key: String, value: serde_json::Value) {
        let expires = now_ms() + CHALLENGE_TTL_MS;
        let mut map = self.inner.lock().await;
        map.insert(key, (value, expires));
    }

    /// Consume a challenge (single-use). Returns None if not found or expired.
    pub async fn consume(&self, key: &str) -> Option<serde_json::Value> {
        let mut map = self.inner.lock().await;
        if let Some((value, expires)) = map.remove(key) {
            if expires > now_ms() {
                return Some(value);
            }
        }
        None
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
