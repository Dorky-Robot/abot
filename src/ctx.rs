//! Where things are: the log, the projects root, the decisions file, diwa.
//! Every path has an environment override, which is also how the tests
//! build a scratch world.

use crate::branch::{self, TaskBranch};
use crate::git::{Git, canon};
use crate::repos::{self, Repo};
use anyhow::{Result, bail};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

pub struct Ctx {
    pub log: PathBuf,
    pub projects: PathBuf,
    pub decisions: PathBuf,
    pub diwa: Option<String>,
    pub tty: bool,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}

fn env_path(key: &str, default: &str) -> PathBuf {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => expand(&v),
        _ => expand(default),
    }
}

impl Ctx {
    pub fn from_env() -> Ctx {
        let diwa = match std::env::var("ABOT_DIWA") {
            Ok(v) if v.is_empty() => None,
            Ok(v) => Some(v),
            Err(_) => Some("diwa".to_string()),
        };
        Ctx {
            log: canon(&env_path("ABOT_LOG", "~/Projects/ecosystem-log")),
            projects: canon(&env_path("ABOT_PROJECTS", "~/Projects")),
            decisions: env_path("ABOT_DECISIONS", "~/Projects/ceo/felix-decisions.json"),
            diwa,
            tty: std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
        }
    }

    pub fn log_git(&self) -> Git {
        Git::new(&self.log)
    }

    /// The log must exist and be a git repo; everything else follows.
    pub fn require_log(&self) -> Result<Git> {
        if !self.log.join(".git").exists() {
            bail!(
                "no ecosystem log at {} (clone Dorky-Robot/ecosystem-log there, or set ABOT_LOG)",
                self.log.display()
            );
        }
        Ok(self.log_git())
    }

    /// `Org/name` of the log, for diwa.
    pub fn log_slug(&self) -> String {
        repos::origin_url(&self.log)
            .and_then(|u| repos::org_name(&u))
            .unwrap_or_else(|| "Dorky-Robot/ecosystem-log".to_string())
    }

    pub fn log_name(&self) -> String {
        self.log
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("ecosystem-log")
            .to_string()
    }

    /// `../ecosystem-log-<task>`
    pub fn log_worktree(&self, task: &str) -> PathBuf {
        sibling(&self.log, task)
    }

    /// The state file for a project on `rev`: `state/projects/<p>.md`, or
    /// `state/<p>.md` for org-wide subjects that predate the projects dir.
    pub fn state_path_at(&self, rev: &str, project: &str) -> Option<String> {
        let g = self.log_git();
        [format!("state/projects/{project}.md"), format!("state/{project}.md")]
            .into_iter()
            .find(|f| g.ok(&["cat-file", "-e", &format!("{rev}:{f}")]))
    }

    pub fn read_at(&self, rev: &str, path: &str) -> Option<String> {
        self.log_git().try_run(&["show", &format!("{rev}:{path}")])
    }

    /// Project slugs: the stems of `state/projects/*.md` on main.
    pub fn projects_on_main(&self) -> Vec<String> {
        let out = self
            .log_git()
            .try_run(&["ls-tree", "--name-only", "main", "state/projects/"])
            .unwrap_or_default();
        let mut v: Vec<String> = out
            .lines()
            .filter_map(|l| l.strip_prefix("state/projects/"))
            .filter_map(|l| l.strip_suffix(".md"))
            .filter(|s| branch::valid_slug(s))
            .map(String::from)
            .collect();
        v.sort();
        v
    }

    pub fn code_repos(&self) -> Vec<Repo> {
        let log = canon(&self.log);
        repos::discover(&self.projects)
            .into_iter()
            .filter(|r| canon(&r.path) != log)
            .collect()
    }

    /// The code repo for a project: `--repo`, else the state file's `Repo:`
    /// line (an `Org/name` or a path), else a repo under ~/Projects whose
    /// origin is named like the project.
    pub fn code_repo(&self, project: &str, override_path: Option<&Path>) -> Result<Option<PathBuf>> {
        if let Some(p) = override_path {
            let p = canon(p);
            if !p.join(".git").exists() {
                bail!("--repo {} is not a git repository", p.display());
            }
            return Ok(Some(p));
        }
        let state = self
            .state_path_at("main", project)
            .and_then(|f| self.read_at("main", &f))
            .unwrap_or_default();
        let named = crate::statefile::field(&state, "Repo");
        if let Some(r) = named.as_deref() {
            if r.starts_with('/') || r.starts_with('~') {
                let p = canon(&expand(r));
                return Ok(p.join(".git").exists().then_some(p));
            }
            let repos = self.code_repos();
            return Ok(repos::find_by_origin(&repos, r).map(|r| r.path.clone()));
        }
        let repos = self.code_repos();
        Ok(repos::find_by_origin(&repos, project).map(|r| r.path.clone()))
    }
}

/// `<dir>/../<name>-<task>`
pub fn sibling(dir: &Path, task: &str) -> PathBuf {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("repo");
    dir.parent().unwrap_or(dir).join(format!("{name}-{task}"))
}

/// The task worktree we are standing in.
pub struct Here {
    pub wt: Git,
    pub path: PathBuf,
    pub branch: TaskBranch,
}

pub fn here(ctx: &Ctx) -> Result<Here> {
    let cwd = std::env::current_dir()?;
    let g = Git::new(&cwd);
    let Some(top) = g.toplevel() else {
        bail!("{} is not inside a git worktree; run this inside the task's log worktree", cwd.display());
    };
    let log = ctx.require_log()?;
    if g.common_dir() != log.common_dir() {
        bail!(
            "{} is not a worktree of the log ({}); run this inside the task's log worktree",
            top.display(),
            ctx.log.display()
        );
    }
    if top == canon(&ctx.log) {
        bail!(
            "this is the log's main checkout, where landing happens; run this inside the task's worktree (../{}-<task>)",
            ctx.log_name()
        );
    }
    let wt = Git::new(&top);
    let Some(b) = wt.current_branch() else {
        bail!("{} has a detached HEAD; check out the task branch", top.display());
    };
    let branch = branch::parse(&b).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(Here { wt, path: top, branch })
}

/// The main checkout must be on main for landing to happen there.
pub fn require_on_main(g: &Git, what: &str) -> Result<()> {
    match g.current_branch() {
        Some(b) if b == "main" => Ok(()),
        Some(b) => bail!("{what} ({}) is on `{b}`, not main; landing happens on main", g.dir.display()),
        None => bail!("{what} ({}) has a detached HEAD; landing happens on main", g.dir.display()),
    }
}
