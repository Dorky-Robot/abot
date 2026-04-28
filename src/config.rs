use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const CONFIG_FILE: &str = "config.json";

/// Behavior config for an agent. Lives at `<agent_dir>/config.json`.
///
/// `instructions` is the system prompt — the agent's disposition. `model`
/// is the Ollama model tag used by `abot run`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default = "default_shell")]
    pub shell: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub instructions: String,
    #[serde(default = "default_model")]
    pub model: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shell: default_shell(),
            env: BTreeMap::new(),
            instructions: String::new(),
            model: default_model(),
        }
    }
}

fn default_shell() -> String {
    "/bin/sh".to_string()
}

fn default_model() -> String {
    "gemma4:31b".to_string()
}

pub fn read(agent_dir: &Path) -> Result<Config> {
    let path = agent_dir.join(CONFIG_FILE);
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

pub fn write(agent_dir: &Path, cfg: &Config) -> Result<()> {
    let path = agent_dir.join(CONFIG_FILE);
    let mut raw = serde_json::to_string_pretty(cfg).context("serializing config")?;
    raw.push('\n');
    std::fs::write(&path, raw).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_uses_gemma4_as_model() {
        let cfg = Config::default();
        assert_eq!(cfg.model, "gemma4:31b");
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.instructions = "You are alice.".to_string();
        cfg.env.insert("EDITOR".to_string(), "vim".to_string());
        cfg.model = "qwen3-coder:30b".to_string();
        write(tmp.path(), &cfg).unwrap();
        let read_back = read(tmp.path()).unwrap();
        assert_eq!(cfg, read_back);
    }

    #[test]
    fn missing_fields_apply_defaults_on_read() {
        let tmp = TempDir::new().unwrap();
        // Hand-written config missing model and shell — defaults should kick in.
        std::fs::write(
            tmp.path().join(CONFIG_FILE),
            r#"{"instructions":"hi"}"#,
        )
        .unwrap();
        let cfg = read(tmp.path()).unwrap();
        assert_eq!(cfg.shell, "/bin/sh");
        assert_eq!(cfg.model, "gemma4:31b");
        assert_eq!(cfg.instructions, "hi");
    }

    #[test]
    fn read_missing_file_errors() {
        let tmp = TempDir::new().unwrap();
        let result = read(tmp.path());
        assert!(result.is_err());
    }
}
