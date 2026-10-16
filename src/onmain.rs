//! One file changed on origin's main, without touching anyone's checkout.
//!
//! Tickets are filed from any box and any directory, often while the agent
//! stands in a task worktree of something else. So a ticket commit is made
//! with git's plumbing against origin/main in a private index, and pushed
//! straight to origin; if someone pushed first, it is made again on top of
//! theirs. Nothing is locked and no working tree is written. Afterwards the
//! log's main checkout is fast-forwarded, when it can be.

use crate::ctx::Ctx;
use anyhow::{Result, bail};

const TRIES: usize = 4;

/// Commit `edit(old text)` at `path` on origin's main with `msg`, and push.
/// `edit` may run more than once (once per try), always on the newest main.
/// Returns the new commit's short sha.
pub fn commit_file<F>(ctx: &Ctx, path: &str, msg: &str, mut edit: F) -> Result<String>
where
    F: FnMut(Option<&str>) -> Result<String>,
{
    let g = ctx.require_log()?;
    if !g.has_origin() {
        bail!("the log at {} has no origin, so nothing can be filed where everyone reads it", ctx.log.display());
    }
    let common = g.common_dir().unwrap_or_else(|| ctx.log.join(".git"));
    let index = common.join(format!("abot-index-{}", std::process::id()));
    let index_s = index.display().to_string();
    let env = [("GIT_INDEX_FILE", index_s.as_str())];
    let mut last_err = String::new();
    for _ in 0..TRIES {
        if let Err(e) = g.run(&["fetch", "-q", "origin", "main"]) {
            bail!("origin can't be reached, so nothing was filed (the log's copy of record is origin): {e:#}");
        }
        let base = g.run(&["rev-parse", "refs/remotes/origin/main"])?;
        let old = g.try_raw(&["show", &format!("{base}:{path}")]);
        let new = edit(old.as_deref())?;
        if old.as_deref() == Some(new.as_str()) {
            bail!("nothing to change in {path}");
        }
        let _ = std::fs::remove_file(&index);
        g.run_env(&["read-tree", &base], &env)?;
        let blob = g.run_input(&["hash-object", "-w", "--stdin"], &[], &new)?;
        g.run_env(&["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{path}")], &env)?;
        let tree = g.run_env(&["write-tree"], &env)?;
        let _ = std::fs::remove_file(&index);
        let commit = g.run_input(&["commit-tree", &tree, "-p", &base, "-F", "-"], &[], msg)?;
        match g.run(&["push", "-q", "origin", &format!("{commit}:refs/heads/main")]) {
            Ok(_) => {
                let _ = g.run(&["fetch", "-q", "origin", "main"]);
                // Bring the main checkout along when it is on main and clean;
                // when it isn't, reads still come from origin.
                if g.current_branch().as_deref() == Some("main") && !g.is_dirty().unwrap_or(true) {
                    let _ = g.run(&["merge", "-q", "--ff-only", "refs/remotes/origin/main"]);
                }
                return Ok(commit[..7.min(commit.len())].to_string());
            }
            Err(e) => last_err = format!("{e:#}"),
        }
    }
    bail!("could not push to origin's main after {TRIES} tries (someone else kept landing first?): {last_err}")
}
