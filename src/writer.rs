//! The ticket writer: a cheap `claude -p` session that helps the filing
//! agent say what it means. It either writes the ticket out or asks the
//! agent short questions; `abot ticket answer` resumes the same session, so
//! it keeps what it was told. It is a co-writer, not a gate: the gate is
//! `ticket::lint`, which holds with or without it.
//!
//! It runs on the box the agent is on, with no tools, no MCP, no settings
//! files and no hooks, on Sonnet at low effort (ABOT_TICKET_MODEL to
//! change). The model is always named: unnamed, `claude` picks the costly
//! default. Its pending conversations live in the log clone's git dir.

use crate::ctx::Ctx;
use crate::ticket::{self, Step, Ticket, Waits};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(240);

pub fn model() -> String {
    std::env::var("ABOT_TICKET_MODEL").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| "sonnet".into())
}

fn claude_bin() -> String {
    std::env::var("ABOT_CLAUDE").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| "claude".into())
}

/// What the agent handed in.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Draft {
    pub project: String,
    pub id: Option<String>,
    pub title: String,
    pub why: String,
    pub done_when: String,
    pub steps: Vec<String>,
    pub waits_on: Option<String>,
    pub detail: Option<String>,
}

/// One round of the conversation, for the filing commit's body.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Round {
    pub asked: Vec<String>,
    pub answered: String,
}

/// A conversation that hasn't filed yet.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Pending {
    pub session: String,
    pub model: String,
    pub draft: Draft,
    pub rounds: Vec<Round>,
    /// The filing agent's own session, when it has one.
    pub filed_by: Option<String>,
    pub cost_usd: f64,
}

pub fn pending_dir(ctx: &Ctx) -> PathBuf {
    let common = ctx.log_git().common_dir().unwrap_or_else(|| ctx.log.join(".git"));
    common.join("abot-tickets")
}

impl Pending {
    pub fn save(&self, ctx: &Ctx) -> Result<()> {
        let d = pending_dir(ctx);
        std::fs::create_dir_all(&d)?;
        std::fs::write(d.join(format!("{}.json", self.session)), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn load(ctx: &Ctx, prefix: &str) -> Result<Pending> {
        let d = pending_dir(ctx);
        let prefix = prefix.trim();
        if prefix.len() < 4 {
            bail!("give at least the first 4 characters of the session the writer named");
        }
        let mut hits: Vec<PathBuf> = std::fs::read_dir(&d)
            .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        hits.retain(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(prefix) && n.ends_with(".json")));
        match hits.len() {
            0 => bail!("no ticket waiting on answers under session {prefix} in this clone ({}); it was filed, or it was started on another box", d.display()),
            1 => Ok(serde_json::from_str(&std::fs::read_to_string(&hits[0])?)?),
            n => bail!("{n} conversations start with {prefix}; give more of the session id"),
        }
    }

    pub fn forget(&self, ctx: &Ctx) {
        let _ = std::fs::remove_file(pending_dir(ctx).join(format!("{}.json", self.session)));
    }
}

/// What the writer said back.
#[derive(Clone, Debug)]
pub enum Reply {
    File(Ticket, String),
    Ask(Vec<String>, String),
}

pub struct Turn {
    pub session: String,
    pub reply: Reply,
    pub cost_usd: f64,
}

const SYSTEM: &str = "\
You are the ticket writer for a small company's work board. An agent (another AI session) is \
filing a ticket for a project. Felix, who runs the company, reads the board first, and other \
agents pick tickets up with no context at all. Your job is to help the agent say what it means, \
so the ticket is understandable on its own. You are a co-writer, not a reviewer: keep the agent's \
meaning and its facts, and write them plainly.

Write it out when the draft, the project's state file and the agent's answers give you enough. \
Ask instead when something only the agent knows is missing or unclear: what done looks like from \
outside, what a step means, who it waits on and for what. Ask at most three short, specific \
questions, each answerable in a sentence. Never invent facts, names, dates or numbers; never pad.

House style: plain English, short sentences, concrete nouns. No internal code names, no labels, no \
private shorthand, no markdown. Say what will be true, not what someone will do in a meeting.

The rules every filed ticket meets (a program checks them after you):
";

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "status": { "type": "string", "enum": ["file", "ask"] },
            "questions": { "type": "array", "items": { "type": "string" } },
            "ticket": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "lowercase-words-with-dashes, at most 40 characters" },
                    "title": { "type": "string" },
                    "why": { "type": "string" },
                    "done_when": { "type": "string" },
                    "steps": { "type": "array", "items": { "type": "string" } },
                    "waits_on_who": { "type": "string", "description": "empty when it waits on nobody" },
                    "waits_on_what": { "type": "string" },
                    "detail": { "type": "string" }
                },
                "required": ["id", "title", "why", "done_when", "steps"]
            },
            "note": { "type": "string", "description": "one line to the agent: what you changed and why, or why you ask" }
        },
        "required": ["status", "note"]
    })
}

fn system_prompt(rules: &str) -> String {
    format!(
        "{SYSTEM}{rules}\n\nAnswer only through the JSON schema: status \"file\" with the ticket, or status \"ask\" with \
         questions. Keep steps in the agent's order; steps already marked done stay worded as they were."
    )
}

pub fn first_prompt(d: &Draft, state: &str, taken: &[String]) -> String {
    let mut s = format!("Project: {}\n\nIts state file (what is true now):\n<<<\n{}\n>>>\n\n", d.project, state.trim());
    if taken.is_empty() {
        s.push_str("No tickets yet in this project.\n\n");
    } else {
        s.push_str("Ticket ids already used in this project (never reuse one):\n");
        for t in taken {
            s.push_str(&format!("- {t}\n"));
        }
        s.push('\n');
    }
    s.push_str("The agent's draft:\n");
    if let Some(id) = &d.id {
        s.push_str(&format!("id: {id}\n"));
    }
    s.push_str(&format!("title: {}\nwhy: {}\ndone when: {}\n", d.title, d.why, d.done_when));
    for (i, st) in d.steps.iter().enumerate() {
        s.push_str(&format!("step {}: {st}\n", i + 1));
    }
    if let Some(w) = &d.waits_on {
        s.push_str(&format!("waits on: {w}\n"));
    }
    if let Some(x) = &d.detail {
        s.push_str(&format!("detail: {x}\n"));
    }
    s
}

/// One turn: a new session (`resume` None) or the next turn of one.
pub fn turn(ctx: &Ctx, resume: Option<&str>, rules: &str, prompt: &str) -> Result<Turn> {
    let dir = pending_dir(ctx);
    std::fs::create_dir_all(&dir)?;
    let model = model();
    let schema = schema().to_string();
    let sys = system_prompt(rules);
    let mut c = Command::new(claude_bin());
    c.current_dir(&dir)
        .args(["-p", "--model", &model, "--effort", "low", "--tools", "", "--setting-sources", "", "--strict-mcp-config"])
        .args(["--system-prompt", &sys, "--output-format", "json", "--json-schema", &schema]);
    if let Some(r) = resume {
        c.args(["--resume", r]);
    }
    c.arg(prompt).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().with_context(|| format!("could not run `{}` for the ticket writer", claude_bin()))?;
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            bail!("the ticket writer took over {}s and was stopped; nothing was filed", TIMEOUT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let o = child.wait_with_output()?;
    let out = String::from_utf8_lossy(&o.stdout);
    let v: Value = serde_json::from_str(out.trim()).with_context(|| {
        format!(
            "the ticket writer's answer was not JSON (exit {}): {}",
            o.status,
            crate::out::clip(&format!("{} {}", out.trim(), String::from_utf8_lossy(&o.stderr).trim()), 400)
        )
    })?;
    if v["is_error"].as_bool() == Some(true) {
        bail!("the ticket writer failed: {}", crate::out::clip(&v["result"].to_string(), 400));
    }
    let session = v["session_id"].as_str().unwrap_or("").to_string();
    let cost_usd = v["total_cost_usd"].as_f64().unwrap_or(0.0);
    let so = if v["structured_output"].is_object() {
        v["structured_output"].clone()
    } else {
        serde_json::from_str(v["result"].as_str().unwrap_or("")).unwrap_or(Value::Null)
    };
    Ok(Turn { session, reply: reply_of(&so)?, cost_usd })
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").trim().to_string()
}

pub fn reply_of(v: &Value) -> Result<Reply> {
    let note = s(&v["note"]);
    match v["status"].as_str() {
        Some("ask") => {
            let qs: Vec<String> = v["questions"].as_array().map(|a| a.iter().map(s).filter(|q| !q.is_empty()).collect()).unwrap_or_default();
            if qs.is_empty() {
                bail!("the ticket writer asked, but put no question");
            }
            Ok(Reply::Ask(qs, note))
        }
        Some("file") => {
            let t = &v["ticket"];
            let who = s(&t["waits_on_who"]).to_ascii_lowercase();
            let waits = (!who.is_empty() && !["nobody", "none", "no one", "n/a"].contains(&who.as_str()))
                .then(|| Waits { who, what: s(&t["waits_on_what"]) });
            let detail = Some(s(&t["detail"])).filter(|d| !d.is_empty());
            let steps = t["steps"]
                .as_array()
                .map(|a| a.iter().map(s).filter(|x| !x.is_empty()).map(|text| Step { text, done: false, by: None }).collect())
                .unwrap_or_default();
            let raw = s(&t["id"]);
            let id = if crate::branch::valid_slug(&raw) && raw.len() <= 48 { raw } else { ticket::slug(&raw) };
            Ok(Reply::File(
                Ticket { id, title: s(&t["title"]), why: s(&t["why"]), done_when: s(&t["done_when"]), waits, detail, filed: None, steps },
                note,
            ))
        }
        _ => bail!("the ticket writer answered without a status: {}", crate::out::clip(&v.to_string(), 300)),
    }
}
