use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;

pub fn init_db(path: &Path) -> Result<Connection> {
    let db = Connection::open(path)?;

    db.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS credentials (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            public_key BLOB NOT NULL,
            counter INTEGER DEFAULT 0,
            device_id TEXT,
            name TEXT,
            user_agent TEXT,
            setup_token_id TEXT,
            created_at INTEGER NOT NULL,
            last_used_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            credential_id TEXT NOT NULL REFERENCES credentials(id),
            csrf_token TEXT NOT NULL,
            expiry INTEGER NOT NULL,
            last_activity_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS setup_tokens (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            hash TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS anthropic_oauth (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            access_token TEXT NOT NULL,
            refresh_token TEXT NOT NULL,
            scopes TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        ",
    )?;

    Ok(db)
}

// --- User CRUD ---

pub fn get_user(db: &Connection) -> Result<Option<(String, String)>> {
    let mut stmt = db.prepare("SELECT id, name FROM users LIMIT 1")?;
    let result = stmt
        .query_row([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .ok();
    Ok(result)
}

pub fn create_user(db: &Connection, id: &str, name: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.execute(
        "INSERT INTO users (id, name, created_at) VALUES (?1, ?2, ?3)",
        params![id, name, now],
    )?;
    Ok(())
}

// --- Credential CRUD ---

#[allow(clippy::too_many_arguments)]
pub fn add_credential(
    db: &Connection,
    id: &str,
    user_id: &str,
    public_key: &[u8],
    counter: u32,
    device_id: Option<&str>,
    name: Option<&str>,
    user_agent: Option<&str>,
    setup_token_id: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.execute(
        "INSERT INTO credentials (id, user_id, public_key, counter, device_id, name, user_agent, setup_token_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![id, user_id, public_key, counter, device_id, name, user_agent, setup_token_id, now],
    )?;
    Ok(())
}

pub fn get_credentials(db: &Connection) -> Result<Vec<CredentialRow>> {
    let mut stmt = db.prepare(
        "SELECT id, user_id, public_key, counter, device_id, name, user_agent, setup_token_id, created_at, last_used_at
         FROM credentials"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CredentialRow {
            id: row.get(0)?,
            user_id: row.get(1)?,
            public_key: row.get(2)?,
            counter: row.get(3)?,
            device_id: row.get(4)?,
            name: row.get(5)?,
            user_agent: row.get(6)?,
            setup_token_id: row.get(7)?,
            created_at: row.get(8)?,
            last_used_at: row.get(9)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn update_credential_counter(db: &Connection, id: &str, counter: u32) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.execute(
        "UPDATE credentials SET counter = ?1, last_used_at = ?2 WHERE id = ?3",
        params![counter, now, id],
    )?;
    Ok(())
}

pub fn credential_count(db: &Connection) -> Result<usize> {
    let count: i64 = db.query_row("SELECT COUNT(*) FROM credentials", [], |row| row.get(0))?;
    Ok(count as usize)
}

// --- Auth Grant CRUD (browser login sessions — renamed to avoid collision with PTY sessions) ---

pub fn create_auth_grant(
    db: &Connection,
    token: &str,
    credential_id: &str,
    csrf_token: &str,
    expiry: i64,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.execute(
        "INSERT INTO sessions (token, credential_id, csrf_token, expiry, last_activity_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![token, credential_id, csrf_token, expiry, now],
    )?;
    Ok(())
}

pub fn get_auth_grant(db: &Connection, token: &str) -> Result<Option<AuthGrantRow>> {
    let mut stmt = db.prepare(
        "SELECT token, credential_id, csrf_token, expiry, last_activity_at FROM sessions WHERE token = ?1",
    )?;
    let result = stmt
        .query_row(params![token], |row| {
            Ok(AuthGrantRow {
                token: row.get(0)?,
                credential_id: row.get(1)?,
                csrf_token: row.get(2)?,
                expiry: row.get(3)?,
                last_activity_at: row.get(4)?,
            })
        })
        .ok();
    Ok(result)
}

pub fn validate_auth_grant(db: &Connection, token: &str) -> Result<bool> {
    let now = chrono::Utc::now().timestamp();
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM sessions s
         JOIN credentials c ON s.credential_id = c.id
         WHERE s.token = ?1 AND s.expiry > ?2",
        params![token, now],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn refresh_auth_grant(db: &Connection, token: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    // Extend expiry if last activity > 24h ago
    let threshold = 24 * 60 * 60;
    db.execute(
        "UPDATE sessions SET last_activity_at = ?1,
         expiry = CASE WHEN (?1 - last_activity_at) > ?2 THEN ?1 + (30 * 24 * 60 * 60) ELSE expiry END
         WHERE token = ?3",
        params![now, threshold, token],
    )?;
    Ok(())
}

pub fn delete_auth_grant(db: &Connection, token: &str) -> Result<()> {
    db.execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
    Ok(())
}

pub fn prune_expired_auth_grants(db: &Connection) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.execute("DELETE FROM sessions WHERE expiry < ?1", params![now])?;
    Ok(())
}

// --- Setup Token CRUD ---

pub fn add_setup_token(
    db: &Connection,
    id: &str,
    name: &str,
    hash: &str,
    expires_at: i64,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.execute(
        "INSERT INTO setup_tokens (id, name, hash, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, name, hash, now, expires_at],
    )?;
    Ok(())
}

pub fn get_setup_tokens(db: &Connection) -> Result<Vec<SetupTokenRow>> {
    let mut stmt = db.prepare("SELECT id, name, created_at, expires_at FROM setup_tokens")?;
    let rows = stmt.query_map([], |row| {
        Ok(SetupTokenRow {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at: row.get(2)?,
            expires_at: row.get(3)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_setup_token_hash(db: &Connection, id: &str) -> Result<Option<String>> {
    let mut stmt = db.prepare("SELECT hash FROM setup_tokens WHERE id = ?1 AND expires_at > ?2")?;
    let now = chrono::Utc::now().timestamp();
    let result = stmt
        .query_row(params![id, now], |row| row.get::<_, String>(0))
        .ok();
    Ok(result)
}

pub fn delete_setup_token(db: &Connection, id: &str) -> Result<()> {
    db.execute("DELETE FROM setup_tokens WHERE id = ?1", params![id])?;
    Ok(())
}

// --- Credential queries ---

pub fn get_credential_for_token(db: &Connection, token_id: &str) -> Result<Option<CredentialRow>> {
    let mut stmt = db.prepare(
        "SELECT id, user_id, public_key, counter, device_id, name, user_agent, setup_token_id, created_at, last_used_at
         FROM credentials WHERE setup_token_id = ?1"
    )?;
    let result = stmt
        .query_row(params![token_id], |row| {
            Ok(CredentialRow {
                id: row.get(0)?,
                user_id: row.get(1)?,
                public_key: row.get(2)?,
                counter: row.get(3)?,
                device_id: row.get(4)?,
                name: row.get(5)?,
                user_agent: row.get(6)?,
                setup_token_id: row.get(7)?,
                created_at: row.get(8)?,
                last_used_at: row.get(9)?,
            })
        })
        .ok();
    Ok(result)
}

pub fn delete_auth_grants_for_credential(
    db: &Connection,
    credential_id: &str,
) -> Result<Vec<String>> {
    let mut stmt = db.prepare("SELECT token FROM sessions WHERE credential_id = ?1")?;
    let tokens: Vec<String> = stmt
        .query_map(params![credential_id], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    db.execute(
        "DELETE FROM sessions WHERE credential_id = ?1",
        params![credential_id],
    )?;
    Ok(tokens)
}

pub fn delete_credential(db: &Connection, credential_id: &str) -> Result<()> {
    db.execute(
        "DELETE FROM credentials WHERE id = ?1",
        params![credential_id],
    )?;
    Ok(())
}

pub fn get_auth_grant_credential_id(db: &Connection, token: &str) -> Result<Option<String>> {
    let mut stmt = db.prepare("SELECT credential_id FROM sessions WHERE token = ?1")?;
    let result = stmt
        .query_row(params![token], |row| row.get::<_, String>(0))
        .ok();
    Ok(result)
}

// --- Row types ---

#[derive(Debug)]
pub struct CredentialRow {
    pub id: String,
    pub user_id: String,
    pub public_key: Vec<u8>,
    pub counter: u32,
    pub device_id: Option<String>,
    pub name: Option<String>,
    pub user_agent: Option<String>,
    pub setup_token_id: Option<String>,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug)]
pub struct AuthGrantRow {
    pub token: String,
    pub credential_id: String,
    pub csrf_token: String,
    pub expiry: i64,
    pub last_activity_at: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct SetupTokenRow {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AnthropicOAuthRow {
    pub access_token: String,
    pub refresh_token: String,
    pub scopes: String,
    pub expires_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

// --- Anthropic OAuth CRUD ---

pub fn get_anthropic_oauth(db: &Connection) -> Result<Option<AnthropicOAuthRow>> {
    let mut stmt = db.prepare(
        "SELECT access_token, refresh_token, scopes, expires_at, created_at, updated_at
         FROM anthropic_oauth WHERE id = 1",
    )?;
    let result = stmt
        .query_row([], |row| {
            Ok(AnthropicOAuthRow {
                access_token: row.get(0)?,
                refresh_token: row.get(1)?,
                scopes: row.get(2)?,
                expires_at: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .ok();
    Ok(result)
}

pub fn upsert_anthropic_oauth(
    db: &Connection,
    access_token: &str,
    refresh_token: &str,
    scopes: &str,
    expires_at: i64,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.execute(
        "INSERT INTO anthropic_oauth (id, access_token, refresh_token, scopes, expires_at, created_at, updated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(id) DO UPDATE SET
           access_token = excluded.access_token,
           refresh_token = excluded.refresh_token,
           scopes = excluded.scopes,
           expires_at = excluded.expires_at,
           updated_at = excluded.updated_at",
        params![access_token, refresh_token, scopes, expires_at, now],
    )?;
    Ok(())
}

pub fn delete_anthropic_oauth(db: &Connection) -> Result<()> {
    db.execute("DELETE FROM anthropic_oauth WHERE id = 1", [])?;
    Ok(())
}
