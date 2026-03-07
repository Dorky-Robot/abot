//! Git operations for abot repos — init, auto-commit, worktrees, variant lifecycle.

use anyhow::{Context, Result};
use std::path::Path;

/// Default .gitignore for an abot repo.
const ABOT_GITIGNORE: &str = "\
credentials.json
scrollback
scrollback.tmp
home/.cache/
home/.local/share/
home/.claude/
home/.bash_history
home/.zsh_history
home/.node_repl_history
home/.python_history
";

/// Run a git command in the given directory and return stdout.
pub(crate) fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git {:?}", args))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {:?} failed: {}", args, stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Initialize an abot directory as a git repo with .gitignore and initial commit.
/// No-op if `.git` already exists (as directory for regular repos, or file for worktrees).
pub fn git_init_abot(abot_path: &Path) -> Result<()> {
    if abot_path.join(".git").exists() {
        return Ok(());
    }

    // Write .gitignore
    let gitignore_path = abot_path.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, ABOT_GITIGNORE)
            .with_context(|| "failed to write .gitignore")?;
    }

    // Update manifest version to 2
    let manifest_path = abot_path.join("manifest.json");
    if manifest_path.exists() {
        if let Ok(mut manifest) = super::bundle::read_json(&manifest_path) {
            manifest["version"] = serde_json::Value::Number(super::bundle::BUNDLE_VERSION.into());
            let _ = super::bundle::write_json(&manifest_path, &manifest);
        }
    }

    // git init + initial commit
    run_git(abot_path, &["init"])?;
    run_git(abot_path, &["add", "-A"])?;
    // Check if there's anything to commit
    let status = run_git(abot_path, &["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        run_git(abot_path, &["commit", "-m", "Initial abot snapshot"])?;
    }

    tracing::info!("initialized git repo at {}", abot_path.display());
    Ok(())
}

/// Auto-commit changes in an abot git repo (used by autosave loop).
/// Works for both regular repos (.git dir) and worktrees (.git file).
/// Returns Ok(true) if a commit was made, Ok(false) if nothing to commit.
pub fn auto_commit_abot(abot_path: &Path) -> Result<bool> {
    if !abot_path.join(".git").exists() {
        return Ok(false);
    }

    run_git(abot_path, &["add", "-A"])?;
    let status = run_git(abot_path, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        return Ok(false);
    }

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let msg = format!("autosave {}", now);
    run_git(abot_path, &["commit", "-m", &msg])?;
    Ok(true)
}

/// Create a canonical `.abot` bundle in the abots directory.
/// Returns the path to the created bundle (e.g. `{abots_dir}/{name}.abot/`).
/// Skips if the bundle already exists. Validates name for path and git-ref safety.
pub fn create_canonical_abot(abots_dir: &Path, name: &str) -> Result<std::path::PathBuf> {
    super::kubo::validate_name(name)?;

    let bundle_path = abots_dir.join(format!("{name}.abot"));
    if bundle_path.exists() {
        return Ok(bundle_path);
    }

    let home_dir = bundle_path.join("home");
    std::fs::create_dir_all(&home_dir)
        .with_context(|| format!("failed to create canonical abot: {}", bundle_path.display()))?;

    // Write a minimal manifest so the canonical abot is a valid bundle
    let now = chrono::Utc::now().to_rfc3339();
    let manifest = serde_json::json!({
        "version": super::bundle::BUNDLE_VERSION,
        "name": name,
        "created_at": now,
        "updated_at": now,
    });
    super::bundle::write_json(&bundle_path.join("manifest.json"), &manifest)?;

    git_init_abot(&bundle_path)?;

    tracing::info!("created canonical abot at {}", bundle_path.display());
    Ok(bundle_path)
}

// ── Git worktree operations ─────────────────────────────────────

/// Add an abot as a git worktree in a kubo directory.
/// Creates a `kubo/<kubo_name>` branch in the canonical abot repo, then
/// runs `git worktree add` to place it at `{kubo_path}/{abot_name}`.
/// Validates names for path and git-ref safety.
pub fn worktree_add_abot(
    canonical_path: &Path,
    kubo_path: &Path,
    abot_name: &str,
    kubo_name: &str,
) -> Result<()> {
    super::kubo::validate_name(abot_name)?;
    super::kubo::validate_name(kubo_name)?;

    let worktree_path = kubo_path.join(abot_name);
    if worktree_path.exists() {
        let git_file = worktree_path.join(".git");
        if git_file.exists() && !git_file.is_dir() {
            // .git file exists — verify the gitdir target is still valid
            if let Ok(content) = std::fs::read_to_string(&git_file) {
                let gitdir = content.trim().strip_prefix("gitdir: ").unwrap_or("");
                if !gitdir.is_empty() && std::path::Path::new(gitdir).exists() {
                    return Ok(()); // Valid worktree, reuse it
                }
            }
            // Stale .git file — remove and recreate
            tracing::warn!(
                "removing stale worktree at {} (gitdir target missing)",
                worktree_path.display()
            );
        } else {
            // Directory exists but isn't a worktree
            tracing::warn!(
                "removing non-worktree directory at {} to set up worktree",
                worktree_path.display()
            );
        }
        std::fs::remove_dir_all(&worktree_path)?;
    }

    if !canonical_path.join(".git").exists() {
        anyhow::bail!(
            "canonical abot is not a git repo: {}",
            canonical_path.display()
        );
    }

    let branch = format!("kubo/{kubo_name}");
    let worktree_str = worktree_path.to_string_lossy().to_string();

    // Create the branch if it doesn't exist (based on current HEAD)
    let branches = run_git(canonical_path, &["branch", "--list", &branch])?;
    if branches.trim().is_empty() {
        run_git(canonical_path, &["branch", &branch])?;
    }

    // Create the worktree
    run_git(canonical_path, &["worktree", "add", &worktree_str, &branch])?;

    tracing::info!(
        "added worktree for '{}' at {} on branch '{}'",
        abot_name,
        worktree_path.display(),
        branch,
    );
    Ok(())
}

/// Remove an abot's worktree from a kubo directory.
/// Currently unused — remove-abot keeps worktrees for resume semantics.
#[allow(dead_code)]
pub fn worktree_remove_abot(
    canonical_path: &Path,
    kubo_path: &Path,
    abot_name: &str,
) -> Result<()> {
    super::kubo::validate_name(abot_name)?;
    let worktree_path = kubo_path.join(abot_name);
    if !worktree_path.exists() {
        return Ok(());
    }

    let worktree_str = worktree_path.to_string_lossy().to_string();
    run_git(
        canonical_path,
        &["worktree", "remove", &worktree_str, "--force"],
    )?;

    // Prune stale worktree metadata so the branch can be re-used
    let _ = run_git(canonical_path, &["worktree", "prune"]);

    tracing::info!(
        "removed worktree for '{}' from {}",
        abot_name,
        kubo_path.display(),
    );
    Ok(())
}

// ── Variant lifecycle ────────────────────────────────────────────

/// Find the filesystem path of a worktree checked out on `kubo_branch`, if any.
fn find_worktree_path(canonical_path: &Path, kubo_branch: &str) -> Option<String> {
    let output = run_git(canonical_path, &["worktree", "list", "--porcelain"]).ok()?;
    let needle = format!("branch refs/heads/{}", kubo_branch);
    for block in output.split("\n\n") {
        if block.lines().any(|l| l == needle) {
            return block
                .lines()
                .find(|l| l.starts_with("worktree "))
                .and_then(|l| l.strip_prefix("worktree "))
                .map(String::from);
        }
    }
    None
}

/// Auto-commit any outstanding changes in a worktree, then remove it.
/// Ensures no work is lost when the worktree is cleaned up.
fn commit_and_remove_worktree(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    if let Some(wt_path) = find_worktree_path(canonical_path, kubo_branch) {
        let wt = std::path::Path::new(&wt_path);
        match auto_commit_abot(wt) {
            Ok(true) => tracing::info!("auto-committed changes in worktree '{}'", wt_path),
            Ok(false) => {}
            Err(e) => tracing::warn!(
                "auto-commit failed in '{}': {} — proceeding with removal",
                wt_path,
                e
            ),
        }
        run_git(canonical_path, &["worktree", "remove", &wt_path, "--force"])
            .with_context(|| format!("failed to remove worktree '{}'", wt_path))?;
        let _ = run_git(canonical_path, &["worktree", "prune"]);
    }
    Ok(())
}

/// Force-remove a worktree without committing (for discard).
fn force_remove_worktree(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    if let Some(wt_path) = find_worktree_path(canonical_path, kubo_branch) {
        if let Err(e) = run_git(canonical_path, &["worktree", "remove", &wt_path, "--force"]) {
            tracing::warn!("force worktree remove failed for '{}': {}", wt_path, e);
        }
        if let Err(e) = run_git(canonical_path, &["worktree", "prune"]) {
            tracing::warn!("worktree prune failed: {}", e);
        }
    }
    Ok(())
}

/// Integrate a kubo variant into the abot's default branch, then delete the branch.
/// Commits any outstanding worktree changes before merging.
/// Returns Err if the merge has conflicts (merge is aborted in that case).
pub fn integrate_variant(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    commit_and_remove_worktree(canonical_path, kubo_branch)?;

    // Ensure we're on the default branch before merging.
    // Try symbolic-ref first; if HEAD is detached, fall back to init.defaultBranch config.
    let default_branch = run_git(canonical_path, &["symbolic-ref", "--short", "HEAD"])
        .map(|s| s.trim().to_string())
        .or_else(|_| {
            run_git(canonical_path, &["config", "--get", "init.defaultBranch"])
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|_| "main".to_string());
    run_git(canonical_path, &["checkout", &default_branch])
        .with_context(|| format!("failed to checkout default branch '{}'", default_branch))?;

    // Merge the kubo branch into the default branch
    let merge_output = run_git(canonical_path, &["merge", kubo_branch, "--no-edit"]);
    if merge_output.is_err() {
        let _ = run_git(canonical_path, &["merge", "--abort"]);
        anyhow::bail!("merge conflict integrating variant '{}'", kubo_branch);
    }

    // Delete the branch (it's now merged)
    run_git(canonical_path, &["branch", "-d", kubo_branch])?;

    tracing::info!(
        "integrated variant '{}' into {}",
        kubo_branch,
        canonical_path.display()
    );
    Ok(())
}

/// Dismiss a kubo variant — commit outstanding changes, remove worktree, keep branch.
pub fn dismiss_variant(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    commit_and_remove_worktree(canonical_path, kubo_branch)?;

    tracing::info!(
        "dismissed variant '{}' from {}",
        kubo_branch,
        canonical_path.display()
    );
    Ok(())
}

/// Discard a kubo variant — delete the branch and worktree without saving.
pub fn discard_variant(canonical_path: &Path, kubo_branch: &str) -> Result<()> {
    force_remove_worktree(canonical_path, kubo_branch)?;

    // Force-delete the branch
    run_git(canonical_path, &["branch", "-D", kubo_branch])?;

    tracing::info!(
        "discarded variant '{}' from {}",
        kubo_branch,
        canonical_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_abot_detail_with_git() {
        let dir = std::env::temp_dir().join("abot-detail-git-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Create a canonical abot
        let abots_dir = dir.join("abots");
        let abot_path = create_canonical_abot(&abots_dir, "testbot").unwrap();
        assert!(abot_path.join(".git").exists());

        let detail = super::super::known_abots::get_abot_detail(&dir, "testbot").unwrap();
        assert_eq!(detail.name, "testbot");
        assert!(detail.created_at.is_some());
        assert!(!detail.default_branch.is_empty());
        assert!(detail.kubo_branches.is_empty()); // no kubo branches yet

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_integrate_variant_commits_worktree_changes() {
        let dir = std::env::temp_dir().join("abot-integrate-commit-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Create canonical abot + kubo worktree
        let abots_dir = dir.join("abots");
        let abot_path = create_canonical_abot(&abots_dir, "alice").unwrap();
        let kubos_dir = dir.join("kubos");
        crate::engine::kubo::Kubo::ensure_kubo_dir(&kubos_dir, "lab").unwrap();
        let kubo_path = kubos_dir.join("lab.kubo");
        worktree_add_abot(&abot_path, &kubo_path, "alice", "lab").unwrap();

        // Create a file in the worktree (simulating terminal work)
        let wt_path = kubo_path.join("alice");
        let test_file = wt_path.join("home").join("work.txt");
        std::fs::create_dir_all(test_file.parent().unwrap()).unwrap();
        std::fs::write(&test_file, "hello from lab").unwrap();

        // Verify file is NOT on default branch yet
        let default_file = abot_path.join("home").join("work.txt");
        assert!(!default_file.exists());

        // Integrate — should auto-commit worktree changes, merge into default
        integrate_variant(&abot_path, "kubo/lab").unwrap();

        // File should now be on the default branch
        assert!(
            default_file.exists(),
            "work.txt should exist on default branch after integrate"
        );
        assert_eq!(
            std::fs::read_to_string(&default_file).unwrap(),
            "hello from lab"
        );

        // Kubo branch should be deleted
        let branches = run_git(&abot_path, &["branch", "--list", "kubo/lab"]).unwrap();
        assert!(branches.trim().is_empty(), "kubo/lab branch should be gone");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discard_variant_does_not_commit_changes() {
        let dir = std::env::temp_dir().join("abot-discard-nocommit-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let abots_dir = dir.join("abots");
        let abot_path = create_canonical_abot(&abots_dir, "alice").unwrap();
        let kubos_dir = dir.join("kubos");
        crate::engine::kubo::Kubo::ensure_kubo_dir(&kubos_dir, "lab").unwrap();
        let kubo_path = kubos_dir.join("lab.kubo");
        worktree_add_abot(&abot_path, &kubo_path, "alice", "lab").unwrap();

        // Create a file in the worktree
        let wt_path = kubo_path.join("alice");
        let test_file = wt_path.join("home").join("secret.txt");
        std::fs::create_dir_all(test_file.parent().unwrap()).unwrap();
        std::fs::write(&test_file, "discard me").unwrap();

        // Discard — should NOT commit changes
        discard_variant(&abot_path, "kubo/lab").unwrap();

        // File should NOT be on the default branch
        let default_file = abot_path.join("home").join("secret.txt");
        assert!(
            !default_file.exists(),
            "secret.txt should NOT exist after discard"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
