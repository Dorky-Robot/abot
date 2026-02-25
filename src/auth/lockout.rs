use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

const MAX_ATTEMPTS: usize = 5;
const WINDOW_MS: u64 = 15 * 60 * 1000; // 15 minutes
const LOCKOUT_MS: u64 = 15 * 60 * 1000; // 15 minutes

/// Brute-force protection: 5 failures in 15 min → 15 min lockout.
/// Matches katulong's CredentialLockout.
pub struct LockoutTracker {
    failures: Arc<Mutex<HashMap<String, Vec<u64>>>>,
    lockouts: Arc<Mutex<HashMap<String, u64>>>,
}

impl LockoutTracker {
    pub fn new() -> Self {
        let tracker = Self {
            failures: Arc::new(Mutex::new(HashMap::new())),
            lockouts: Arc::new(Mutex::new(HashMap::new())),
        };

        // Background cleanup
        let failures = tracker.failures.clone();
        let lockouts = tracker.lockouts.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let now = now_ms();

                let mut f = failures.lock().await;
                for timestamps in f.values_mut() {
                    timestamps.retain(|t| now - t < WINDOW_MS);
                }
                f.retain(|_, v| !v.is_empty());

                let mut l = lockouts.lock().await;
                l.retain(|_, expires| *expires > now);
            }
        });

        tracker
    }

    pub async fn is_locked(&self, credential_id: &str) -> (bool, Option<u64>) {
        let lockouts = self.lockouts.lock().await;
        if let Some(&expires) = lockouts.get(credential_id) {
            let now = now_ms();
            if expires > now {
                let retry_after = (expires - now) / 1000;
                return (true, Some(retry_after));
            }
        }
        (false, None)
    }

    pub async fn record_failure(&self, credential_id: &str) {
        let now = now_ms();

        let mut failures = self.failures.lock().await;
        let timestamps = failures.entry(credential_id.to_string()).or_default();
        timestamps.push(now);
        timestamps.retain(|t| now - t < WINDOW_MS);

        if timestamps.len() >= MAX_ATTEMPTS {
            let mut lockouts = self.lockouts.lock().await;
            lockouts.insert(credential_id.to_string(), now + LOCKOUT_MS);
            timestamps.clear();
        }
    }

    pub async fn record_success(&self, credential_id: &str) {
        let mut failures = self.failures.lock().await;
        failures.remove(credential_id);

        let mut lockouts = self.lockouts.lock().await;
        lockouts.remove(credential_id);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
