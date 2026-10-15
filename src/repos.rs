//! Finding code repos under ~/Projects: main checkouts one or two levels
//! down, matched by their `origin` URL.

use std::path::{Path, PathBuf};

/// `Org/name` from a git URL: ssh, https, or a local path to `name.git`.
pub fn org_name(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    // git@github.com:Org/name -> Org/name ; https://host/Org/name -> Org/name
    let tail = match u.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map(|(_, p)| p)?,
        None => match u.split_once(':') {
            Some((host, p)) if !host.contains('/') => p,
            _ => u,
        },
    };
    let parts: Vec<&str> = tail.split('/').filter(|s| !s.is_empty()).collect();
    match parts.len() {
        0 => None,
        1 => Some(parts[0].to_string()),
        n => Some(format!("{}/{}", parts[n - 2], parts[n - 1])),
    }
}

/// The repo name: the last part of `Org/name`.
pub fn short_name(org_name: &str) -> &str {
    org_name.rsplit('/').next().unwrap_or(org_name)
}

/// `origin`'s URL, read from `.git/config` without spawning git.
pub fn origin_url(repo: &Path) -> Option<String> {
    let cfg = std::fs::read_to_string(repo.join(".git").join("config")).ok()?;
    let mut in_origin = false;
    for line in cfg.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_origin = t == "[remote \"origin\"]";
        } else if in_origin
            && let Some((k, v)) = t.split_once('=')
            && k.trim() == "url"
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct Repo {
    pub path: PathBuf,
    pub origin: Option<String>, // Org/name
}

/// Main checkouts (a `.git` directory, not a worktree's `.git` file) at
/// `<root>/*/` and `<root>/*/*/`, sorted by path.
pub fn discover(root: &Path) -> Vec<Repo> {
    let mut out = vec![];
    let mut visit = |dir: &Path| {
        if dir.join(".git").is_dir() {
            out.push(Repo {
                path: dir.to_path_buf(),
                origin: origin_url(dir).and_then(|u| org_name(&u)),
            });
            return true;
        }
        false
    };
    let Ok(level1) = std::fs::read_dir(root) else { return vec![] };
    let mut dirs: Vec<PathBuf> = level1
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !hidden(p))
        .collect();
    dirs.sort();
    for d in dirs {
        if visit(&d) {
            continue;
        }
        let Ok(level2) = std::fs::read_dir(&d) else { continue };
        let mut sub: Vec<PathBuf> = level2
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && !hidden(p))
            .collect();
        sub.sort();
        for s in sub {
            visit(&s);
        }
    }
    out
}

fn hidden(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.') || n == "node_modules" || n == "target")
}

/// The repo whose origin is `want` (`Org/name`, or a bare `name`).
pub fn find_by_origin<'a>(repos: &'a [Repo], want: &str) -> Option<&'a Repo> {
    let want = want.trim();
    repos.iter().find(|r| {
        r.origin.as_deref().is_some_and(|o| {
            if want.contains('/') {
                o.eq_ignore_ascii_case(want)
            } else {
                short_name(o).eq_ignore_ascii_case(want)
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_names() {
        assert_eq!(org_name("git@github.com:Dorky-Robot/abot.git").as_deref(), Some("Dorky-Robot/abot"));
        assert_eq!(org_name("https://github.com/Dorky-Robot/kita2.git").as_deref(), Some("Dorky-Robot/kita2"));
        assert_eq!(org_name("https://github.com/Dorky-Robot/kita2/").as_deref(), Some("Dorky-Robot/kita2"));
        assert_eq!(org_name("ssh://git@host:22/a/b.git").as_deref(), Some("a/b"));
        assert_eq!(org_name("/tmp/x/remotes/demo.git").as_deref(), Some("remotes/demo"));
        assert_eq!(short_name("remotes/demo"), "demo");
    }

    #[test]
    fn finds_by_origin() {
        let repos = vec![
            Repo { path: "/a".into(), origin: Some("Dorky-Robot/overview".into()) },
            Repo { path: "/b".into(), origin: None },
        ];
        assert_eq!(find_by_origin(&repos, "dorky-robot/overview").unwrap().path, PathBuf::from("/a"));
        assert_eq!(find_by_origin(&repos, "overview").unwrap().path, PathBuf::from("/a"));
        assert!(find_by_origin(&repos, "Other/overview").is_none());
    }
}
