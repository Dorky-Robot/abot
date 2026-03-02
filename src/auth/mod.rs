pub mod challenge;
pub mod handlers;
pub mod lockout;
pub mod middleware;
pub mod state;
pub mod tokens;
pub mod webauthn_config;

/// Current time in milliseconds since UNIX epoch.
/// Returns 0 on clock skew (system time before epoch) rather than panicking.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use webauthn_rs::prelude::*;

pub struct AuthState {
    pub db: Arc<Mutex<Connection>>,
    pub webauthn: Arc<Webauthn>,
    pub challenges: challenge::ChallengeStore,
    pub lockout: lockout::LockoutTracker,
}

impl AuthState {
    pub fn new(db: Connection, addr: &str) -> anyhow::Result<Self> {
        let webauthn = webauthn_config::build_webauthn(addr)?;

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            webauthn: Arc::new(webauthn),
            challenges: challenge::ChallengeStore::new(),
            lockout: lockout::LockoutTracker::new(),
        })
    }
}
