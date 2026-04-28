use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

/// One worktree as reported by `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// `git init <path>` (creates the directory if missing).
pub fn init(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    run(&["init", "--quiet"], Some(path))
}

/// `git -C <repo> config <key> <value>` — set a repo-local config value.
pub fn set_config(repo: &Path, key: &str, value: &str) -> Result<()> {
    run(&["config", key, value], Some(repo))
}

/// `git clone <src> <dst>`.
pub fn clone(src: &Path, dst: &Path) -> Result<()> {
    run(
        &[
            "clone",
            "--quiet",
            src.to_str().ok_or_else(|| anyhow!("src is not utf-8: {}", src.display()))?,
            dst.to_str().ok_or_else(|| anyhow!("dst is not utf-8: {}", dst.display()))?,
        ],
        None,
    )
}

/// Create `branch` if it does not already exist. Idempotent.
pub fn ensure_branch(repo: &Path, branch: &str) -> Result<()> {
    if branch_exists(repo, branch)? {
        return Ok(());
    }
    run(&["branch", branch], Some(repo))
}

/// `true` if `branch` exists locally in `repo`.
pub fn branch_exists(repo: &Path, branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .output()
        .context("running git show-ref")?;
    Ok(output.status.success())
}

/// `git -C <repo> branch --format=%(refname:short)` returned as a sorted list.
pub fn list_branches(repo: &Path) -> Result<Vec<String>> {
    let stdout = capture(&["branch", "--format=%(refname:short)"], Some(repo))?;
    let mut branches: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    branches.sort();
    Ok(branches)
}

/// `git worktree add <path> <branch>`. Creates `branch` if missing.
pub fn worktree_add(repo: &Path, path: &Path, branch: &str) -> Result<()> {
    let path_str = path.to_str().ok_or_else(|| anyhow!("worktree path is not utf-8"))?;
    if branch_exists(repo, branch)? {
        run(&["worktree", "add", path_str, branch], Some(repo))
    } else {
        run(&["worktree", "add", "-b", branch, path_str], Some(repo))
    }
}

/// `git worktree remove [--force] <path>`. Does NOT delete the branch.
///
/// `force = true` lets removal succeed even if the worktree has uncommitted
/// changes — useful when the agent itself is being deleted.
pub fn worktree_remove(repo: &Path, path: &Path, force: bool) -> Result<()> {
    let path_str = path.to_str().ok_or_else(|| anyhow!("worktree path is not utf-8"))?;
    if force {
        run(&["worktree", "remove", "--force", path_str], Some(repo))
    } else {
        run(&["worktree", "remove", path_str], Some(repo))
    }
}

/// Parse `git worktree list --porcelain` into structured records.
pub fn worktree_list(repo: &Path) -> Result<Vec<Worktree>> {
    let stdout = capture(&["worktree", "list", "--porcelain"], Some(repo))?;
    let mut out = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            if let Some(p) = current_path.take() {
                out.push(Worktree { path: p, branch: current_branch.take() });
            }
            current_path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(rest.to_string());
        }
    }
    if let Some(p) = current_path.take() {
        out.push(Worktree { path: p, branch: current_branch.take() });
    }
    Ok(out)
}

/// Merge `branch` into the current HEAD, then delete `branch`.
pub fn merge_and_delete(repo: &Path, branch: &str, message: &str) -> Result<()> {
    run(&["merge", "--no-edit", "-m", message, branch], Some(repo))?;
    run(&["branch", "-d", branch], Some(repo))
}

/// Delete `branch`. Set `force = true` to drop unmerged commits.
pub fn delete_branch(repo: &Path, branch: &str, force: bool) -> Result<()> {
    let flag = if force { "-D" } else { "-d" };
    run(&["branch", flag, branch], Some(repo))
}

/// `git log` with an optional revision range or branch name. Returns stdout.
pub fn log(repo: &Path, target: Option<&str>) -> Result<String> {
    let mut args = vec!["log", "--oneline"];
    if let Some(t) = target {
        args.push(t);
    }
    capture(&args, Some(repo))
}

/// `git diff <range>`. Returns stdout.
pub fn diff(repo: &Path, range: &str) -> Result<String> {
    capture(&["diff", range], Some(repo))
}

/// `git add -A && git commit -m <message>`. No-op if nothing is staged.
pub fn commit_all(repo: &Path, message: &str) -> Result<()> {
    run(&["add", "-A"], Some(repo))?;
    // `git commit` returns nonzero if there's nothing to commit; treat that as ok.
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["commit", "--allow-empty-message", "-m", message])
        .output()
        .context("running git commit")?;
    if output.status.success() {
        return Ok(());
    }
    // git emits "nothing to commit" on stdout, not stderr. Check both.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stdout.contains("nothing to commit")
        || stdout.contains("nothing added")
        || stderr.contains("nothing to commit")
        || stderr.contains("nothing added")
    {
        return Ok(());
    }
    Err(anyhow!("git commit failed: stdout={stdout} stderr={stderr}"))
}

fn run(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new("git");
    if let Some(dir) = cwd {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args);
    let output = cmd.output().with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn capture(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    if let Some(dir) = cwd {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args);
    let output = cmd.output().with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Init a git repo with a known author so `commit` works without
    /// depending on the user's global git config.
    fn fresh_repo() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().to_path_buf();
        init(&repo).unwrap();
        run(&["config", "user.email", "tests@abot"], Some(&repo)).unwrap();
        run(&["config", "user.name", "abot tests"], Some(&repo)).unwrap();
        (tmp, repo)
    }

    fn write(repo: &Path, name: &str, body: &str) {
        fs::write(repo.join(name), body).unwrap();
    }

    fn current_branch(repo: &Path) -> String {
        capture(&["rev-parse", "--abbrev-ref", "HEAD"], Some(repo))
            .unwrap()
            .trim()
            .to_string()
    }

    #[test]
    fn init_creates_a_git_repo() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("repo");
        init(&target).unwrap();
        assert!(target.join(".git").exists());
    }

    #[test]
    fn commit_all_lands_a_commit() {
        let (_tmp, repo) = fresh_repo();
        write(&repo, "hello.txt", "hi");
        commit_all(&repo, "first").unwrap();
        let out = log(&repo, None).unwrap();
        assert!(out.contains("first"));
    }

    #[test]
    fn commit_all_is_a_noop_when_nothing_changed() {
        let (_tmp, repo) = fresh_repo();
        write(&repo, "hello.txt", "hi");
        commit_all(&repo, "first").unwrap();
        // Second commit_all with no changes should not error.
        commit_all(&repo, "second").unwrap();
    }

    #[test]
    fn ensure_branch_is_idempotent() {
        let (_tmp, repo) = fresh_repo();
        write(&repo, "hello.txt", "hi");
        commit_all(&repo, "first").unwrap();
        ensure_branch(&repo, "kubo/test").unwrap();
        ensure_branch(&repo, "kubo/test").unwrap(); // no error
        assert!(branch_exists(&repo, "kubo/test").unwrap());
        assert!(!branch_exists(&repo, "kubo/missing").unwrap());
    }

    #[test]
    fn list_branches_returns_sorted_local_branches() {
        let (_tmp, repo) = fresh_repo();
        write(&repo, "f", "x");
        commit_all(&repo, "first").unwrap();
        ensure_branch(&repo, "kubo/b").unwrap();
        ensure_branch(&repo, "kubo/a").unwrap();
        let branches = list_branches(&repo).unwrap();
        let kubos: Vec<_> = branches.iter().filter(|b| b.starts_with("kubo/")).collect();
        assert_eq!(kubos, vec!["kubo/a", "kubo/b"]);
    }

    #[test]
    fn worktree_add_then_remove() {
        let (_tmp, repo) = fresh_repo();
        write(&repo, "f", "x");
        commit_all(&repo, "first").unwrap();
        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("wt");
        worktree_add(&repo, &wt, "kubo/test").unwrap();
        assert!(wt.exists());
        let listed = worktree_list(&repo).unwrap();
        assert!(listed.iter().any(|w| w.branch.as_deref() == Some("kubo/test")));
        worktree_remove(&repo, &wt, false).unwrap();
        assert!(!wt.exists());
    }

    #[test]
    fn merge_and_delete_folds_branch_into_head() {
        let (_tmp, repo) = fresh_repo();
        write(&repo, "a", "1");
        commit_all(&repo, "first").unwrap();
        let head = current_branch(&repo);

        // Make a branch with a new commit via a worktree (no checkout dance).
        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("wt");
        worktree_add(&repo, &wt, "kubo/work").unwrap();
        run(&["config", "user.email", "tests@abot"], Some(&wt)).unwrap();
        run(&["config", "user.name", "abot tests"], Some(&wt)).unwrap();
        write(&wt, "b", "2");
        commit_all(&wt, "work commit").unwrap();
        worktree_remove(&repo, &wt, false).unwrap();

        merge_and_delete(&repo, "kubo/work", "integrate work").unwrap();
        assert!(!branch_exists(&repo, "kubo/work").unwrap());
        // The merged file is now visible in the canonical repo's HEAD.
        let log_out = log(&repo, Some(&head)).unwrap();
        assert!(log_out.contains("work commit"));
    }

    #[test]
    fn delete_branch_force_drops_unmerged() {
        let (_tmp, repo) = fresh_repo();
        write(&repo, "a", "1");
        commit_all(&repo, "first").unwrap();
        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("wt");
        worktree_add(&repo, &wt, "kubo/throwaway").unwrap();
        run(&["config", "user.email", "tests@abot"], Some(&wt)).unwrap();
        run(&["config", "user.name", "abot tests"], Some(&wt)).unwrap();
        write(&wt, "b", "2");
        commit_all(&wt, "discard me").unwrap();
        worktree_remove(&repo, &wt, false).unwrap();

        // -d would refuse because the branch isn't merged; -D should succeed.
        assert!(delete_branch(&repo, "kubo/throwaway", false).is_err());
        delete_branch(&repo, "kubo/throwaway", true).unwrap();
        assert!(!branch_exists(&repo, "kubo/throwaway").unwrap());
    }

    #[test]
    fn diff_shows_changes_in_range() {
        let (_tmp, repo) = fresh_repo();
        write(&repo, "a", "1");
        commit_all(&repo, "first").unwrap();
        let head = current_branch(&repo);

        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("wt");
        worktree_add(&repo, &wt, "kubo/diff-test").unwrap();
        run(&["config", "user.email", "tests@abot"], Some(&wt)).unwrap();
        run(&["config", "user.name", "abot tests"], Some(&wt)).unwrap();
        write(&wt, "new-file", "hello");
        commit_all(&wt, "add new-file").unwrap();
        worktree_remove(&repo, &wt, false).unwrap();

        let out = diff(&repo, &format!("{head}..kubo/diff-test")).unwrap();
        assert!(out.contains("new-file"));
    }

    #[test]
    fn clone_copies_repo() {
        let (_tmp, src) = fresh_repo();
        write(&src, "f", "x");
        commit_all(&src, "first").unwrap();
        let dst_root = TempDir::new().unwrap();
        let dst = dst_root.path().join("dst");
        clone(&src, &dst).unwrap();
        assert!(dst.join(".git").exists());
        let out = log(&dst, None).unwrap();
        assert!(out.contains("first"));
    }
}
