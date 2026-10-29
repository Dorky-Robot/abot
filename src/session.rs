//! The Claude Code session this shell runs in: its sidebar id (for the
//! `Session:` trailer) and, for a `claude --bg` job, its short id (for
//! `claude stop`).
//!
//! Remote Control exports $CLAUDE_CODE_BRIDGE_SESSION_ID and a job exports
//! $CLAUDE_JOB_DIR, but not every session gets both: one started from the
//! desktop app or by overview (Claude Code 2.1.310) has neither, so it
//! started with no `Session:` and nothing could close it (10-29, "Kapwa ·
//! one machine"). Every session has $CLAUDE_CODE_SESSION_ID, and a job's
//! `~/.claude/jobs/<short>/state.json` holds that id as `sessionId`, next to
//! `daemonShort` and `bridgeSessionId` (cse_X, the sidebar's session_X).

use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Me {
    /// the sidebar's `session_…`
    pub session: Option<String>,
    /// the job's short id, for `claude stop`
    pub job: Option<String>,
}

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn me() -> Me {
    let mut me = Me {
        session: var("CLAUDE_CODE_BRIDGE_SESSION_ID").map(|s| crate::cmd::lifecycle::session_id(&s)),
        job: var("CLAUDE_JOB_DIR").and_then(|j| short_of(Path::new(j.trim_end_matches('/')))),
    };
    if me.session.is_some() && me.job.is_some() {
        return me;
    }
    let state = match var("CLAUDE_JOB_DIR") {
        Some(j) => read_state(&Path::new(&j).join("state.json")),
        None => var("CLAUDE_CODE_SESSION_ID").and_then(|id| find_job(&id)),
    };
    if let Some(s) = state {
        if me.session.is_none() {
            me.session = s.get("bridgeSessionId").and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(crate::cmd::lifecycle::session_id);
        }
        if me.job.is_none() {
            me.job = s.get("daemonShort").and_then(|v| v.as_str()).filter(|v| valid_short(v)).map(str::to_string);
        }
    }
    me
}

fn short_of(dir: &Path) -> Option<String> {
    dir.file_name().and_then(|n| n.to_str()).filter(|s| valid_short(s)).map(str::to_string)
}

fn valid_short(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn read_state(p: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

fn jobs_dir() -> Option<PathBuf> {
    match var("CLAUDE_CONFIG_DIR") {
        Some(d) => Some(PathBuf::from(d).join("jobs")),
        None => var("HOME").map(|h| PathBuf::from(h).join(".claude/jobs")),
    }
}

/// The job whose state.json names this session.
fn find_job(session: &str) -> Option<serde_json::Value> {
    for e in std::fs::read_dir(jobs_dir()?).ok()?.flatten() {
        if let Some(s) = read_state(&e.path().join("state.json")) {
            if s.get("sessionId").and_then(|v| v.as_str()) == Some(session) {
                return Some(s);
            }
        }
    }
    None
}
