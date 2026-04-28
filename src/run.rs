use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::config;
use crate::paths;

/// Single-turn LLM dispatch. Production uses [`OllamaClient`]; tests use
/// fakes so they don't depend on a live Ollama.
pub trait LlmClient {
    fn chat_stream(
        &self,
        model: &str,
        system: &str,
        user: &str,
        sink: &mut dyn Write,
    ) -> Result<()>;
}

pub struct OllamaClient {
    pub base_url: String,
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".to_string(),
        }
    }
}

#[derive(Deserialize)]
struct OllamaChunk {
    message: Option<OllamaMessage>,
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    content: String,
}

impl LlmClient for OllamaClient {
    fn chat_stream(
        &self,
        model: &str,
        system: &str,
        user: &str,
        sink: &mut dyn Write,
    ) -> Result<()> {
        let mut messages: Vec<serde_json::Value> = Vec::new();
        if !system.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        messages.push(serde_json::json!({"role": "user", "content": user}));

        let url = format!("{}/api/chat", self.base_url);
        let resp = ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(serde_json::json!({
                "model": model,
                "messages": messages,
                "stream": true,
            }))
            .with_context(|| format!("POST {url}"))?;

        let mut reader = BufReader::new(resp.into_reader());
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let chunk: OllamaChunk = serde_json::from_str(trimmed)
                .with_context(|| format!("parsing Ollama response chunk: {trimmed}"))?;
            if let Some(err) = chunk.error {
                bail!("Ollama error: {err}");
            }
            if let Some(msg) = chunk.message {
                if !msg.content.is_empty() {
                    sink.write_all(msg.content.as_bytes())?;
                    sink.flush()?;
                }
            }
            if chunk.done {
                break;
            }
        }
        // Convention: most replies don't end with newline; add one so shells
        // get a clean prompt back.
        sink.write_all(b"\n")?;
        sink.flush()?;
        Ok(())
    }
}

/// Run an agent: read agent config, send (system + user) to the LLM,
/// stream the response to `sink`.
///
/// `room` is validated when present (the agent must be employed there).
/// In phase 1 the room only gates the call; future runs may use the
/// worktree's `home/` as a working directory.
pub fn run(
    root: &Path,
    name: &str,
    room: Option<&str>,
    input: &str,
    client: &dyn LlmClient,
    sink: &mut dyn Write,
) -> Result<()> {
    let canonical = paths::agent_dir(root, name);
    if !canonical.exists() {
        bail!("no such agent: {name}");
    }
    if let Some(r) = room {
        let wt = paths::agent_in_kubo(root, r, name);
        if !wt.exists() {
            bail!("{name} is not employed in {r}");
        }
    }
    let cfg = config::read(&canonical)?;
    client.chat_stream(&cfg.model, &cfg.instructions, input, sink)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent;
    use crate::employ;
    use crate::settings;
    use std::cell::RefCell;
    use tempfile::TempDir;

    fn fresh_root() -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        settings::write_commit_email(&root, "tests@abot").unwrap();
        settings::write_commit_name(&root, "abot tests").unwrap();
        (tmp, root)
    }

    /// Records the inputs it was called with and emits a fixed reply.
    struct RecordingClient {
        reply: String,
        seen: RefCell<Option<(String, String, String)>>,
    }

    impl RecordingClient {
        fn new(reply: &str) -> Self {
            Self {
                reply: reply.to_string(),
                seen: RefCell::new(None),
            }
        }
    }

    impl LlmClient for RecordingClient {
        fn chat_stream(
            &self,
            model: &str,
            system: &str,
            user: &str,
            sink: &mut dyn Write,
        ) -> Result<()> {
            *self.seen.borrow_mut() = Some((model.into(), system.into(), user.into()));
            sink.write_all(self.reply.as_bytes())?;
            Ok(())
        }
    }

    #[test]
    fn run_passes_config_and_input_to_the_client() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        agent::config_set(&root, "alice", "instructions", "you are alice").unwrap();
        let client = RecordingClient::new("hello back");
        let mut sink: Vec<u8> = Vec::new();
        run(&root, "alice", None, "hi alice", &client, &mut sink).unwrap();
        let seen = client.seen.borrow().clone().unwrap();
        assert_eq!(seen.0, "gemma4:31b");
        assert_eq!(seen.1, "you are alice");
        assert_eq!(seen.2, "hi alice");
        assert_eq!(String::from_utf8(sink).unwrap(), "hello back");
    }

    #[test]
    fn run_uses_per_agent_model_override() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        agent::config_set(&root, "alice", "model", "qwen2.5-coder:7b").unwrap();
        let client = RecordingClient::new("");
        let mut sink: Vec<u8> = Vec::new();
        run(&root, "alice", None, "ping", &client, &mut sink).unwrap();
        let seen = client.seen.borrow().clone().unwrap();
        assert_eq!(seen.0, "qwen2.5-coder:7b");
    }

    #[test]
    fn run_errors_for_missing_agent() {
        let (_tmp, root) = fresh_root();
        let client = RecordingClient::new("");
        let mut sink: Vec<u8> = Vec::new();
        assert!(run(&root, "ghost", None, "x", &client, &mut sink).is_err());
    }

    #[test]
    fn run_errors_when_room_isnt_employed() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        let client = RecordingClient::new("");
        let mut sink: Vec<u8> = Vec::new();
        assert!(run(&root, "alice", Some("nowhere"), "x", &client, &mut sink).is_err());
    }

    #[test]
    fn run_succeeds_when_room_exists_and_agent_is_employed() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        employ::employ(&root, "alice", "daily-room").unwrap();
        let client = RecordingClient::new("ok");
        let mut sink: Vec<u8> = Vec::new();
        run(&root, "alice", Some("daily-room"), "hi", &client, &mut sink).unwrap();
        assert_eq!(String::from_utf8(sink).unwrap(), "ok");
    }
}
