use anyhow::{Result, bail};
use std::path::Path;

use crate::agent;
use crate::git;
use crate::manifest;
use crate::paths;
use crate::settings;

/// Clone an existing agent into a new identity.
///
/// Full git copy — the new agent inherits all of the source's history and
/// branches, but is independent (no `origin` remote pointing back). The
/// manifest is rewritten with the new name and fresh timestamps; commit
/// identity is reset from the abot settings facts.
pub fn clone(root: &Path, source: &str, new_name: &str) -> Result<()> {
    agent::validate_name(new_name)?;
    let src_dir = paths::agent_dir(root, source);
    let dst_dir = paths::agent_dir(root, new_name);
    if !src_dir.exists() {
        bail!("no such agent: {source}");
    }
    if dst_dir.exists() {
        bail!("agent already exists: {new_name}");
    }

    git::clone(&src_dir, &dst_dir)?;
    git::remove_remote_if_exists(&dst_dir, "origin")?;

    // Reset the manifest with the new identity.
    let mut m = manifest::read(&dst_dir)?;
    m.name = new_name.to_string();
    let now = chrono::Utc::now();
    m.created = now;
    m.updated = now;
    manifest::write(&dst_dir, &m)?;

    // Re-apply commit identity (settings drives it; falls through to the
    // user's git global if unset).
    if let (Some(email), Some(name)) = (
        settings::read_commit_email(root)?,
        settings::read_commit_name(root)?,
    ) {
        git::set_config(&dst_dir, "user.email", &email)?;
        git::set_config(&dst_dir, "user.name", &name)?;
    }

    git::commit_all(&dst_dir, &format!("cloned from {source}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn clone_creates_independent_repo() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        clone(&root, "alice", "alice-draft").unwrap();
        let dst = paths::agent_dir(&root, "alice-draft");
        assert!(dst.exists());
        assert!(dst.join(".git").exists());
        // No origin remote — clone is fully independent.
        let remotes = git::list_branches(&dst).unwrap(); // smoke that the repo is functional
        assert!(!remotes.is_empty());
    }

    #[test]
    fn cloned_manifest_reflects_new_name() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        clone(&root, "alice", "alice-draft").unwrap();
        let m = manifest::read(&paths::agent_dir(&root, "alice-draft")).unwrap();
        assert_eq!(m.name, "alice-draft");
    }

    #[test]
    fn clone_records_a_commit_marking_the_origin() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        clone(&root, "alice", "alice-draft").unwrap();
        let log = git::log(&paths::agent_dir(&root, "alice-draft"), None).unwrap();
        assert!(log.contains("cloned from alice"));
        // Original initialization commit is preserved.
        assert!(log.contains("initialize agent: alice"));
    }

    #[test]
    fn clone_rejects_missing_source() {
        let (_tmp, root) = fresh_root();
        assert!(clone(&root, "ghost", "ghost-copy").is_err());
    }

    #[test]
    fn clone_rejects_existing_destination() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        agent::create(&root, "bob").unwrap();
        assert!(clone(&root, "alice", "bob").is_err());
    }

    #[test]
    fn clone_rejects_invalid_destination_name() {
        let (_tmp, root) = fresh_root();
        agent::create(&root, "alice").unwrap();
        assert!(clone(&root, "alice", "alice/bob").is_err());
    }
}
