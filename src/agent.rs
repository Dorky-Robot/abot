use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::config::{self, Config};
use crate::git;
use crate::manifest::{self, Manifest};
use crate::paths;
use crate::settings;

const DEFAULT_GITIGNORE: &str = "\
# Secrets that should never enter git history.
credentials.json
.env
";

/// Create a new agent identity at `<root>/agents/<name>.abot/`.
///
/// Initializes a git repo with manifest.json, config.json, .gitignore,
/// and a `home/` working directory; commits all of it. Commit identity
/// comes from `<root>/settings.json` (commit_email / commit_name) if set,
/// otherwise inherits the user's global git config.
pub fn create(root: &Path, name: &str) -> Result<()> {
    validate_name(name)?;
    let dir = paths::agent_dir(root, name);
    if dir.exists() {
        bail!("agent already exists: {name}");
    }
    let resolved_identity = ensure_commit_identity(root)?;

    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::create_dir_all(dir.join("home")).context("creating home/")?;
    // .gitkeep so home/ survives in git history (and propagates to worktrees).
    std::fs::write(dir.join("home").join(".gitkeep"), "").context("writing home/.gitkeep")?;
    std::fs::write(dir.join(".gitignore"), DEFAULT_GITIGNORE).context("writing .gitignore")?;

    git::init(&dir)?;
    if let Some((email, name)) = resolved_identity {
        git::set_config(&dir, "user.email", &email)?;
        git::set_config(&dir, "user.name", &name)?;
    }

    manifest::write(&dir, &Manifest::new(name))?;
    config::write(&dir, &Config::default())?;
    git::commit_all(&dir, &format!("initialize agent: {name}"))?;
    Ok(())
}

/// Pre-flight check: make sure a commit identity is reachable somewhere.
///
/// Returns Some((email, name)) when both abot settings facts exist — caller
/// applies these as repo-local git config. Returns None when the user's
/// global git config is the source of truth (no repo-local override needed).
/// Errors when neither is set.
fn ensure_commit_identity(root: &Path) -> Result<Option<(String, String)>> {
    let email = settings::read_commit_email(root)?;
    let name = settings::read_commit_name(root)?;
    if let (Some(e), Some(n)) = (email, name) {
        return Ok(Some((e, n)));
    }
    if global_git_identity_present() {
        return Ok(None);
    }
    bail!(
        "no commit identity configured. Either:\n  \
         1) echo 'you@example.com' > {} && echo 'Your Name' > {}\n  \
         2) run: git config --global user.email <email> && git config --global user.name <name>",
        settings::commit_email_path(root).display(),
        settings::commit_name_path(root).display(),
    );
}

fn global_git_identity_present() -> bool {
    let email = std::process::Command::new("git")
        .args(["config", "--global", "user.email"])
        .output();
    let name = std::process::Command::new("git")
        .args(["config", "--global", "user.name"])
        .output();
    match (email, name) {
        (Ok(e), Ok(n)) => {
            !String::from_utf8_lossy(&e.stdout).trim().is_empty()
                && !String::from_utf8_lossy(&n.stdout).trim().is_empty()
        }
        _ => false,
    }
}

/// List agent names (sorted, no `.abot` suffix).
pub fn list(root: &Path) -> Result<Vec<String>> {
    let agents = paths::agents_dir(root);
    if !agents.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = std::fs::read_dir(&agents)
        .with_context(|| format!("reading {}", agents.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_suffix(".abot")
                .map(String::from)
        })
        .collect();
    names.sort();
    Ok(names)
}

/// Aggregated read-only view of an agent.
#[derive(Debug)]
pub struct AgentInfo {
    pub manifest: Manifest,
    pub config: Config,
    pub branches: Vec<String>,
    pub worktrees: Vec<git::Worktree>,
}

/// Read everything `show` needs to print.
pub fn show(root: &Path, name: &str) -> Result<AgentInfo> {
    let dir = paths::agent_dir(root, name);
    if !dir.exists() {
        bail!("no such agent: {name}");
    }
    Ok(AgentInfo {
        manifest: manifest::read(&dir)?,
        config: config::read(&dir)?,
        branches: git::list_branches(&dir)?,
        worktrees: git::worktree_list(&dir)?,
    })
}

/// Delete an agent entirely. Force-removes any active worktrees first
/// so we don't leave orphans pointing at a missing canonical .git dir.
pub fn rm(root: &Path, name: &str) -> Result<()> {
    let dir = paths::agent_dir(root, name);
    if !dir.exists() {
        bail!("no such agent: {name}");
    }
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.clone());
    if let Ok(worktrees) = git::worktree_list(&dir) {
        for wt in worktrees {
            let wt_canonical = wt.path.canonicalize().unwrap_or(wt.path.clone());
            if wt_canonical == canonical {
                continue; // skip the canonical repo itself
            }
            let _ = git::worktree_remove(&dir, &wt.path, true);
        }
    }
    std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    Ok(())
}

/// Read the full config for printing.
pub fn config_read(root: &Path, name: &str) -> Result<Config> {
    let dir = paths::agent_dir(root, name);
    if !dir.exists() {
        bail!("no such agent: {name}");
    }
    config::read(&dir)
}

/// Get one config value by key. Limited to top-level string fields for now.
pub fn config_get(root: &Path, name: &str, key: &str) -> Result<String> {
    let cfg = config_read(root, name)?;
    match key {
        "shell" => Ok(cfg.shell),
        "model" => Ok(cfg.model),
        "instructions" => Ok(cfg.instructions),
        _ => bail!("unknown config key: {key} (try shell, model, instructions)"),
    }
}

/// `git log` for the agent. Without `room`, returns the agent's full history;
/// with `room`, returns just that branch's commits.
pub fn log(root: &Path, name: &str, room: Option<&str>) -> Result<String> {
    let dir = paths::agent_dir(root, name);
    if !dir.exists() {
        bail!("no such agent: {name}");
    }
    let target = room.map(paths::room_branch);
    git::log(&dir, target.as_deref())
}

/// `git diff <main>..kubo/<room>` — what changed in `room` vs the agent's main.
pub fn diff(root: &Path, name: &str, room: &str) -> Result<String> {
    let dir = paths::agent_dir(root, name);
    if !dir.exists() {
        bail!("no such agent: {name}");
    }
    let branch = paths::room_branch(room);
    if !git::branch_exists(&dir, &branch)? {
        bail!("no branch {branch} on agent {name}");
    }
    let head = git::current_branch(&dir)?;
    git::diff(&dir, &format!("{head}..{branch}"))
}

/// Set one config value by key. Same allow-list as `config_get`.
pub fn config_set(root: &Path, name: &str, key: &str, value: &str) -> Result<()> {
    let dir = paths::agent_dir(root, name);
    if !dir.exists() {
        bail!("no such agent: {name}");
    }
    let mut cfg = config::read(&dir)?;
    match key {
        "shell" => cfg.shell = value.to_string(),
        "model" => cfg.model = value.to_string(),
        "instructions" => cfg.instructions = value.to_string(),
        _ => bail!("unknown config key: {key} (try shell, model, instructions)"),
    }
    config::write(&dir, &cfg)?;
    Ok(())
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("agent name must not be empty");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!(
            "agent name must contain only ASCII letters, digits, '-', or '_': got {name:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_root() -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        // Every test writes deterministic settings facts so commit identity
        // doesn't depend on the user's git global config.
        settings::write_commit_email(&root, "tests@abot").unwrap();
        settings::write_commit_name(&root, "abot tests").unwrap();
        (tmp, root)
    }

    #[test]
    fn create_lays_out_the_expected_files() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        let dir = paths::agent_dir(&root, "alice");
        assert!(dir.join(".git").exists());
        assert!(dir.join("manifest.json").exists());
        assert!(dir.join("config.json").exists());
        assert!(dir.join(".gitignore").exists());
        assert!(dir.join("home").join(".gitkeep").exists());
    }

    #[test]
    fn create_records_the_initial_commit() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        let dir = paths::agent_dir(&root, "alice");
        let log = git::log(&dir, None).unwrap();
        assert!(log.contains("initialize agent: alice"));
    }

    #[test]
    fn create_uses_settings_json_for_commit_identity() {
        let (_tmp, root) = fresh_root();
        // fresh_root() already wrote a settings.json with tests@abot identity.
        create(&root, "alice").unwrap();
        let dir = paths::agent_dir(&root, "alice");
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["log", "-1", "--format=%ae|%an"])
            .output()
            .unwrap();
        let line = String::from_utf8_lossy(&out.stdout);
        let line = line.trim();
        assert_eq!(line, "tests@abot|abot tests");
    }

    #[test]
    fn create_overrides_per_agent_when_settings_change() {
        let (_tmp, root) = fresh_root();
        // Overwrite the default test settings with a different identity.
        settings::write_commit_email(&root, "felix@dorkyrobot.com").unwrap();
        settings::write_commit_name(&root, "Felix Flores").unwrap();
        create(&root, "alice").unwrap();
        let dir = paths::agent_dir(&root, "alice");
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["log", "-1", "--format=%ae|%an"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "felix@dorkyrobot.com|Felix Flores"
        );
    }

    #[test]
    fn create_rejects_a_duplicate() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        assert!(create(&root, "alice").is_err());
    }

    #[test]
    fn create_rejects_invalid_names() {
        let (_tmp, root) = fresh_root();
        assert!(create(&root, "").is_err());
        assert!(create(&root, "alice/bob").is_err());
        assert!(create(&root, "alice bob").is_err());
        assert!(create(&root, "alice.bob").is_err());
    }

    #[test]
    fn list_returns_empty_when_no_agents_dir() {
        let (_tmp, root) = fresh_root();
        let names = list(&root).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn list_returns_sorted_names_without_dot_abot_suffix() {
        let (_tmp, root) = fresh_root();
        create(&root, "charlie").unwrap();
        create(&root, "alice").unwrap();
        create(&root, "bob").unwrap();
        let names = list(&root).unwrap();
        assert_eq!(names, vec!["alice", "bob", "charlie"]);
    }

    #[test]
    fn show_reads_manifest_and_config() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        let info = show(&root, "alice").unwrap();
        assert_eq!(info.manifest.name, "alice");
        assert_eq!(info.config.model, "gemma4:31b");
    }

    #[test]
    fn show_errors_for_missing_agent() {
        let (_tmp, root) = fresh_root();
        assert!(show(&root, "nope").is_err());
    }

    #[test]
    fn rm_removes_the_directory() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        rm(&root, "alice").unwrap();
        assert!(!paths::agent_dir(&root, "alice").exists());
    }

    #[test]
    fn rm_removes_active_worktrees() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        let dir = paths::agent_dir(&root, "alice");
        let wt_dir = paths::agent_in_kubo(&root, "test-room", "alice");
        std::fs::create_dir_all(wt_dir.parent().unwrap()).unwrap();
        git::worktree_add(&dir, &wt_dir, "kubo/test-room").unwrap();
        assert!(wt_dir.exists());

        rm(&root, "alice").unwrap();
        assert!(!dir.exists());
        assert!(!wt_dir.exists());
    }

    #[test]
    fn config_get_returns_known_keys() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        assert_eq!(config_get(&root, "alice", "model").unwrap(), "gemma4:31b");
        assert_eq!(config_get(&root, "alice", "shell").unwrap(), "/bin/sh");
        assert_eq!(config_get(&root, "alice", "instructions").unwrap(), "");
    }

    #[test]
    fn config_get_rejects_unknown_keys() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        assert!(config_get(&root, "alice", "bogus").is_err());
    }

    #[test]
    fn config_set_persists_the_change() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        config_set(&root, "alice", "model", "qwen3-coder:30b").unwrap();
        assert_eq!(config_get(&root, "alice", "model").unwrap(), "qwen3-coder:30b");
    }

    #[test]
    fn config_set_rejects_unknown_keys() {
        let (_tmp, root) = fresh_root();
        create(&root, "alice").unwrap();
        assert!(config_set(&root, "alice", "bogus", "x").is_err());
    }
}
