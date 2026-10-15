//! Plain git, run as a subprocess. abot never talks to a repository any
//! other way, so everything it does can be repeated by hand.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Clone, Debug)]
pub struct Git {
    pub dir: PathBuf,
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Git { dir: dir.into() }
    }

    fn command(&self, args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut c = Command::new("git");
        c.arg("-C").arg(&self.dir).args(args);
        for (k, v) in env {
            c.env(k, v);
        }
        c
    }

    fn output(&self, args: &[&str], env: &[(&str, &str)]) -> Result<Output> {
        match self.command(args, env).output() {
            Ok(o) => Ok(o),
            Err(e) => bail!("could not run git ({e}); is git installed?"),
        }
    }

    /// Run and return stdout without its trailing newline; a failure is an
    /// error carrying git's own words.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        self.run_env(args, &[])
    }

    pub fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> Result<String> {
        let o = self.output(args, env)?;
        if !o.status.success() {
            let err = String::from_utf8_lossy(&o.stderr);
            let out = String::from_utf8_lossy(&o.stdout);
            let why = if err.trim().is_empty() { out.trim().to_string() } else { err.trim().to_string() };
            bail!("git {} failed in {}: {}", args.join(" "), self.dir.display(), why);
        }
        Ok(trim_nl(String::from_utf8_lossy(&o.stdout).into_owned()))
    }

    /// Run and return stdout exactly as git printed it, or None on failure.
    pub fn try_raw(&self, args: &[&str]) -> Option<String> {
        let o = self.output(args, &[]).ok()?;
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    /// Run and return stdout, or None if git failed.
    pub fn try_run(&self, args: &[&str]) -> Option<String> {
        let o = self.output(args, &[]).ok()?;
        o.status
            .success()
            .then(|| trim_nl(String::from_utf8_lossy(&o.stdout).into_owned()))
    }

    pub fn ok(&self, args: &[&str]) -> bool {
        self.output(args, &[]).map(|o| o.status.success()).unwrap_or(false)
    }

    pub fn has_ref(&self, r: &str) -> bool {
        self.ok(&["rev-parse", "--verify", "-q", r])
    }

    pub fn has_local_branch(&self, b: &str) -> bool {
        self.has_ref(&format!("refs/heads/{b}"))
    }

    /// Whether `origin` has the branch, asked of origin itself rather than a
    /// possibly stale remote-tracking ref. Err when origin can't be reached.
    pub fn origin_has_branch(&self, b: &str) -> Result<bool> {
        let o = self.output(&["ls-remote", "--heads", "origin", &format!("refs/heads/{b}")], &[])?;
        if !o.status.success() {
            bail!(
                "can't reach origin from {}: {}",
                self.dir.display(),
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Ok(!o.stdout.is_empty())
    }

    pub fn has_origin(&self) -> bool {
        self.ok(&["remote", "get-url", "origin"])
    }

    pub fn current_branch(&self) -> Option<String> {
        self.try_run(&["symbolic-ref", "--short", "-q", "HEAD"])
    }

    pub fn is_dirty(&self) -> Result<bool> {
        Ok(!self.run(&["status", "--porcelain"])?.is_empty())
    }

    pub fn path_dirty(&self, path: &str) -> Result<bool> {
        Ok(!self.run(&["status", "--porcelain", "--", path])?.is_empty())
    }

    /// `main` if it exists, else `master`: the trunk of a code repo.
    pub fn trunk(&self) -> String {
        for b in ["main", "master"] {
            if self.has_local_branch(b) {
                return b.to_string();
            }
        }
        "main".to_string()
    }

    /// Absolute path of the shared .git dir, the same for every worktree.
    pub fn common_dir(&self) -> Option<PathBuf> {
        let s = self.try_run(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        Some(canon(Path::new(&s)))
    }

    pub fn toplevel(&self) -> Option<PathBuf> {
        self.try_run(&["rev-parse", "--show-toplevel"]).map(|s| canon(Path::new(&s)))
    }

    /// The worktree that has `branch` checked out, if any.
    pub fn worktree_of(&self, branch: &str) -> Option<PathBuf> {
        let list = self.try_run(&["worktree", "list", "--porcelain"])?;
        let want = format!("branch refs/heads/{branch}");
        let mut path = None;
        for line in list.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if line == want {
                return path;
            }
        }
        None
    }

    pub fn commit_message(&self, rev: &str) -> Result<String> {
        self.run(&["log", "-1", "--format=%B", rev])
    }
}

pub fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn trim_nl(mut s: String) -> String {
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    s
}
