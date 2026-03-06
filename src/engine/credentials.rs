//! Unified credential mapping — one source of truth for how tokens map to env vars and JSON keys.

use std::collections::HashMap;
use std::path::Path;

/// Env var names used for credentials.
pub const ANTHROPIC_API_KEY: &str = "ANTHROPIC_API_KEY";
pub const CLAUDE_API_KEY: &str = "CLAUDE_API_KEY";
pub const CLAUDE_CODE_OAUTH_TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// All credential env var names.
pub const ALL_KEYS: &[&str] = &[ANTHROPIC_API_KEY, CLAUDE_API_KEY, CLAUDE_CODE_OAUTH_TOKEN];

/// Detect whether a token string is an API key or an OAuth token.
fn is_api_key(token: &str) -> bool {
    token.starts_with("sk-ant-api")
}

/// Given a token, produce the env vars it maps to (set values).
/// Returns a map of env_key → value.
pub fn token_to_env(token: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();
    if is_api_key(token) {
        env.insert(ANTHROPIC_API_KEY.into(), token.into());
        env.insert(CLAUDE_API_KEY.into(), token.into());
    } else {
        env.insert(CLAUDE_CODE_OAUTH_TOKEN.into(), token.into());
    }
    env
}

/// Build an env update map for the engine. `Some(token)` sets the appropriate
/// keys and clears the others; `None` clears all credential keys.
pub fn build_env_update(token: Option<&str>) -> HashMap<String, Option<String>> {
    let mut env = HashMap::new();
    match token {
        Some(t) if is_api_key(t) => {
            env.insert(ANTHROPIC_API_KEY.into(), Some(t.to_string()));
            env.insert(CLAUDE_API_KEY.into(), Some(t.to_string()));
            env.insert(CLAUDE_CODE_OAUTH_TOKEN.into(), None);
        }
        Some(t) => {
            env.insert(CLAUDE_CODE_OAUTH_TOKEN.into(), Some(t.to_string()));
            env.insert(ANTHROPIC_API_KEY.into(), None);
            env.insert(CLAUDE_API_KEY.into(), None);
        }
        None => {
            for key in ALL_KEYS {
                env.insert((*key).into(), None);
            }
        }
    }
    env
}

/// Read a `credentials.json` file and return env vars for container injection.
///
/// JSON format:
///   `api_key` → maps to ANTHROPIC_API_KEY + CLAUDE_API_KEY (if sk-ant-api) or CLAUDE_CODE_OAUTH_TOKEN
///   `claude_token` → maps to CLAUDE_CODE_OAUTH_TOKEN
pub fn read_credentials_file(path: &Path) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let creds = match super::bundle::read_json(path) {
        Ok(v) => v,
        Err(_) => return env,
    };
    if let Some(obj) = creds.as_object() {
        if let Some(val) = obj.get("api_key").and_then(|v| v.as_str()) {
            env.extend(token_to_env(val));
        }
        if let Some(val) = obj.get("claude_token").and_then(|v| v.as_str()) {
            env.insert(CLAUDE_CODE_OAUTH_TOKEN.into(), val.into());
        }
    }
    env
}

/// Extract credential env vars from a session env map into a JSON-ready map
/// for writing to credentials.json.
pub fn env_to_credentials_json(
    session_env: &HashMap<String, String>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut creds = serde_json::Map::new();

    // Case 1: API key present — write as api_key (covers ANTHROPIC_API_KEY or CLAUDE_API_KEY)
    if let Some(val) = session_env
        .get(ANTHROPIC_API_KEY)
        .or_else(|| session_env.get(CLAUDE_API_KEY))
    {
        creds.insert("api_key".into(), serde_json::Value::String(val.clone()));
    }

    // Case 2: OAuth token — write as claude_token, and also as api_key if no API key is set
    if let Some(val) = session_env.get(CLAUDE_CODE_OAUTH_TOKEN) {
        if !session_env.contains_key(ANTHROPIC_API_KEY) && !session_env.contains_key(CLAUDE_API_KEY)
        {
            creds.insert("api_key".into(), serde_json::Value::String(val.clone()));
        }
        creds.insert(
            "claude_token".into(),
            serde_json::Value::String(val.clone()),
        );
    }

    creds
}

/// Check if an env var key is a credential key.
pub fn is_credential_key(key: &str) -> bool {
    ALL_KEYS.contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_maps_to_anthropic_and_claude() {
        let env = token_to_env("sk-ant-api03-test");
        assert_eq!(env.get(ANTHROPIC_API_KEY).unwrap(), "sk-ant-api03-test");
        assert_eq!(env.get(CLAUDE_API_KEY).unwrap(), "sk-ant-api03-test");
        assert!(!env.contains_key(CLAUDE_CODE_OAUTH_TOKEN));
    }

    #[test]
    fn oauth_token_maps_to_oauth_only() {
        let env = token_to_env("some-oauth-token");
        assert_eq!(
            env.get(CLAUDE_CODE_OAUTH_TOKEN).unwrap(),
            "some-oauth-token"
        );
        assert!(!env.contains_key(ANTHROPIC_API_KEY));
        assert!(!env.contains_key(CLAUDE_API_KEY));
    }

    #[test]
    fn build_env_update_sets_api_key() {
        let env = build_env_update(Some("sk-ant-api03-test"));
        assert_eq!(env[ANTHROPIC_API_KEY], Some("sk-ant-api03-test".into()));
        assert_eq!(env[CLAUDE_API_KEY], Some("sk-ant-api03-test".into()));
        assert_eq!(env[CLAUDE_CODE_OAUTH_TOKEN], None);
    }

    #[test]
    fn build_env_update_sets_oauth() {
        let env = build_env_update(Some("oauth-token"));
        assert_eq!(env[ANTHROPIC_API_KEY], None);
        assert_eq!(env[CLAUDE_API_KEY], None);
        assert_eq!(env[CLAUDE_CODE_OAUTH_TOKEN], Some("oauth-token".into()));
    }

    #[test]
    fn build_env_update_clears_all() {
        let env = build_env_update(None);
        assert_eq!(env[ANTHROPIC_API_KEY], None);
        assert_eq!(env[CLAUDE_API_KEY], None);
        assert_eq!(env[CLAUDE_CODE_OAUTH_TOKEN], None);
    }

    #[test]
    fn is_credential_key_matches() {
        assert!(is_credential_key("ANTHROPIC_API_KEY"));
        assert!(is_credential_key("CLAUDE_API_KEY"));
        assert!(is_credential_key("CLAUDE_CODE_OAUTH_TOKEN"));
        assert!(!is_credential_key("HOME"));
        assert!(!is_credential_key("PATH"));
    }

    #[test]
    fn env_to_credentials_json_api_key() {
        let mut env = HashMap::new();
        env.insert(ANTHROPIC_API_KEY.into(), "sk-ant-api03-x".into());
        env.insert(CLAUDE_API_KEY.into(), "sk-ant-api03-x".into());
        let creds = env_to_credentials_json(&env);
        assert_eq!(
            creds.get("api_key").unwrap().as_str().unwrap(),
            "sk-ant-api03-x"
        );
        assert!(!creds.contains_key("claude_token"));
    }

    #[test]
    fn env_to_credentials_json_oauth() {
        let mut env = HashMap::new();
        env.insert(CLAUDE_CODE_OAUTH_TOKEN.into(), "token-abc".into());
        let creds = env_to_credentials_json(&env);
        assert_eq!(creds.get("api_key").unwrap().as_str().unwrap(), "token-abc");
        assert_eq!(
            creds.get("claude_token").unwrap().as_str().unwrap(),
            "token-abc"
        );
    }
}
