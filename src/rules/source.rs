//! Where the graph is read from: a directory (a worktree being built) or a
//! git revision (origin/main, for `abot check`). Both answer the same two
//! questions, so the fold, the compile and the check never know which.

use crate::git::Git;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub enum Source {
    Dir(PathBuf),
    Rev(Git, String),
}

impl Source {
    /// Every file under `prefix` (a directory, no trailing slash), as paths
    /// relative to the root, sorted.
    pub fn list(&self, prefix: &str) -> Vec<String> {
        let mut v = match self {
            Source::Dir(root) => {
                let mut out = Vec::new();
                walk(root, &root.join(prefix), &mut out);
                out
            }
            Source::Rev(g, rev) => g
                .try_run(&["ls-tree", "-r", "--name-only", rev, "--", prefix])
                .unwrap_or_default()
                .lines()
                .map(String::from)
                .collect(),
        };
        v.sort();
        v
    }

    pub fn read(&self, path: &str) -> Option<String> {
        match self {
            Source::Dir(root) => std::fs::read_to_string(root.join(path)).ok(),
            Source::Rev(g, rev) => g.try_raw(&["show", &format!("{rev}:{path}")]),
        }
    }

    pub fn root(&self) -> Option<&Path> {
        match self {
            Source::Dir(r) => Some(r),
            Source::Rev(..) => None,
        }
    }

    pub fn write(&self, path: &str, text: &str) -> Result<()> {
        let Source::Dir(root) = self else {
            anyhow::bail!("can't write {path} into a git revision");
        };
        let p = root.join(path);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, text).with_context(|| format!("writing {}", p.display()))
    }

    pub fn remove(&self, path: &str) -> Result<()> {
        if let Source::Dir(root) = self {
            let p = root.join(path);
            if p.exists() {
                std::fs::remove_file(&p).with_context(|| format!("removing {}", p.display()))?;
            }
        }
        Ok(())
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
}
