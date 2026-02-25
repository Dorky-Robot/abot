pub mod challenge;
pub mod handlers;
pub mod lockout;
pub mod middleware;
pub mod state;
pub mod tokens;
pub mod webauthn_config;

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
