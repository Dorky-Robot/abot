pub mod decisions;
pub mod dig;
pub mod lifecycle;
pub mod map;
pub mod orient;
pub mod sessions;

use crate::git::Git;
use anyhow::{Result, bail};

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Fast-forward a main checkout from origin. Ok(None) when it pulled, or
/// Ok(Some(reason)) when it couldn't (offline, no origin, diverged).
pub fn pull_main(g: &Git, trunk: &str) -> Result<Option<String>> {
    if !g.has_origin() {
        return Ok(Some("no origin remote".into()));
    }
    match g.current_branch() {
        Some(b) if b == trunk => {}
        Some(b) => return Ok(Some(format!("main checkout is on `{b}`, not {trunk}"))),
        None => return Ok(Some("main checkout has a detached HEAD".into())),
    }
    match g.run(&["pull", "--ff-only", "-q", "origin", trunk]) {
        Ok(_) => Ok(None),
        Err(e) => Ok(Some(format!("{e:#}"))),
    }
}

/// Like pull_main, but a failure is a refusal: landing on a stale main
/// would make the push fail or, worse, land on top of something old.
pub fn sync_main(g: &Git, trunk: &str, what: &str) -> Result<()> {
    if let Some(why) = pull_main(g, trunk)? {
        if why == "no origin remote" {
            return Ok(());
        }
        bail!("could not bring {what}'s {trunk} up to date with origin: {why}");
    }
    Ok(())
}

/// First line, whitespace collapsed.
pub fn one_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// All of it on one line, whitespace collapsed.
pub fn flatten(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
