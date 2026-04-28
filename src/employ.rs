use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::git;
use crate::paths;

/// Bind agent `name` to `room` by creating a worktree on `kubo/<room>`.
///
/// The branch is created if it doesn't already exist (auto-branched off
/// the agent's current HEAD). Worktree lives at
/// `<root>/kubos/<room>/<name>/`.
pub fn employ(root: &Path, name: &str, room: &str) -> Result<()> {
    let canonical = paths::agent_dir(root, name);
    if !canonical.exists() {
        bail!("no such agent: {name}");
    }
    let worktree = paths::agent_in_kubo(root, room, name);
    if worktree.exists() {
        bail!("{name} is already employed in {room}");
    }
    let kubo = paths::kubo_dir(root, room);
    std::fs::create_dir_all(&kubo).with_context(|| format!("creating {}", kubo.display()))?;
    let branch = paths::room_branch(room);
    git::worktree_add(&canonical, &worktree, &branch)?;
    Ok(())
}

/// Remove the worktree for `name` in `room`. Branch is preserved — the
/// agent's work in that room stays in git history. Polite removal, so
/// uncommitted changes block the operation.
pub fn dismiss(root: &Path, name: &str, room: &str) -> Result<()> {
    let canonical = paths::agent_dir(root, name);
    if !canonical.exists() {
        bail!("no such agent: {name}");
    }
    let worktree = paths::agent_in_kubo(root, room, name);
    if !worktree.exists() {
        bail!("{name} is not employed in {room}");
    }
    git::worktree_remove(&canonical, &worktree, false)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent;
    use crate::settings;
    use tempfile::TempDir;

    fn fresh_root() -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        settings::write_commit_email(&root, "tests@abot").unwrap();
        settings::write_commit_name(&root, "abot tests").unwrap();
        (tmp, root)
    }

    #[test]
    fn employ_creates_worktree_and_branch() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        employ(&root, "alice", "daily-room").unwrap();
        let wt = paths::agent_in_kubo(&root, "daily-room", "alice");
        assert!(wt.exists());
        let branch = paths::room_branch("daily-room");
        let canonical = paths::agent_dir(&root, "alice");
        assert!(git::branch_exists(&canonical, &branch).unwrap());
    }

    #[test]
    fn employ_errors_for_missing_agent() {
        let (_tmp, root) = fresh_root();
        assert!(employ(&root, "ghost", "daily-room").is_err());
    }

    #[test]
    fn employ_rejects_double_employment_in_same_room() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        employ(&root, "alice", "daily-room").unwrap();
        assert!(employ(&root, "alice", "daily-room").is_err());
    }

    #[test]
    fn dismiss_removes_worktree_keeps_branch() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        employ(&root, "alice", "daily-room").unwrap();
        dismiss(&root, "alice", "daily-room").unwrap();
        let wt = paths::agent_in_kubo(&root, "daily-room", "alice");
        assert!(!wt.exists());
        let branch = paths::room_branch("daily-room");
        let canonical = paths::agent_dir(&root, "alice");
        assert!(git::branch_exists(&canonical, &branch).unwrap());
    }

    #[test]
    fn dismiss_errors_when_not_employed() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        assert!(dismiss(&root, "alice", "daily-room").is_err());
    }
}
