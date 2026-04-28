use anyhow::{Result, bail};
use std::path::Path;

use crate::git;
use crate::paths;

/// Fold the room's branch into the agent's main and delete the branch.
///
/// If a worktree still exists, it's politely dismissed first — uncommitted
/// changes block the operation; commit or stash before integrating.
pub fn integrate(root: &Path, name: &str, room: &str) -> Result<()> {
    let canonical = paths::agent_dir(root, name);
    if !canonical.exists() {
        bail!("no such agent: {name}");
    }
    let branch = paths::room_branch(room);
    if !git::branch_exists(&canonical, &branch)? {
        bail!("no branch {branch} on agent {name}");
    }
    let worktree = paths::agent_in_kubo(root, room, name);
    if worktree.exists() {
        git::worktree_remove(&canonical, &worktree, false)?;
    }
    git::merge_and_delete(&canonical, &branch, &format!("integrate {room}"))?;
    Ok(())
}

/// Throw the room's branch away. Force-removes any active worktree first.
pub fn discard(root: &Path, name: &str, room: &str) -> Result<()> {
    let canonical = paths::agent_dir(root, name);
    if !canonical.exists() {
        bail!("no such agent: {name}");
    }
    let branch = paths::room_branch(room);
    if !git::branch_exists(&canonical, &branch)? {
        bail!("no branch {branch} on agent {name}");
    }
    let worktree = paths::agent_in_kubo(root, room, name);
    if worktree.exists() {
        git::worktree_remove(&canonical, &worktree, true)?;
    }
    git::delete_branch(&canonical, &branch, true)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent;
    use crate::employ;
    use crate::settings;
    use std::fs;
    use tempfile::TempDir;

    fn fresh_root() -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        settings::write_commit_email(&root, "tests@abot").unwrap();
        settings::write_commit_name(&root, "abot tests").unwrap();
        (tmp, root)
    }

    /// Make a commit inside the worktree so integrate has something to merge.
    fn commit_in_worktree(wt: &Path, filename: &str, contents: &str, msg: &str) {
        fs::write(wt.join(filename), contents).unwrap();
        git::commit_all(wt, msg).unwrap();
    }

    #[test]
    fn integrate_folds_work_into_main_and_drops_branch() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        employ::employ(&root, "alice", "daily-room").unwrap();
        let wt = paths::agent_in_kubo(&root, "daily-room", "alice");
        commit_in_worktree(&wt, "note.txt", "hello", "alice's room work");

        integrate(&root, "alice", "daily-room").unwrap();

        let canonical = paths::agent_dir(&root, "alice");
        let branch = paths::room_branch("daily-room");
        assert!(!git::branch_exists(&canonical, &branch).unwrap());
        // The committed file is now visible in main's history.
        let log = git::log(&canonical, None).unwrap();
        assert!(log.contains("alice's room work"));
        // Worktree dir was dismissed during integrate.
        assert!(!wt.exists());
    }

    #[test]
    fn integrate_errors_for_missing_branch() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        assert!(integrate(&root, "alice", "never-existed").is_err());
    }

    #[test]
    fn discard_drops_branch_and_worktree() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        employ::employ(&root, "alice", "throwaway").unwrap();
        let wt = paths::agent_in_kubo(&root, "throwaway", "alice");
        commit_in_worktree(&wt, "scratch.txt", "junk", "scratch");

        discard(&root, "alice", "throwaway").unwrap();

        let canonical = paths::agent_dir(&root, "alice");
        let branch = paths::room_branch("throwaway");
        assert!(!git::branch_exists(&canonical, &branch).unwrap());
        assert!(!wt.exists());
        // Main history is untouched.
        let log = git::log(&canonical, None).unwrap();
        assert!(!log.contains("scratch"));
    }

    #[test]
    fn discard_errors_for_missing_branch() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        assert!(discard(&root, "alice", "never-existed").is_err());
    }
}
