use std::path::{Path, PathBuf};

/// The canonical abot data root.
///
/// Reads `$ABOT_ROOT` first (used by tests and ad-hoc overrides),
/// then falls back to `$HOME/.abot`.
pub fn default_root() -> anyhow::Result<PathBuf> {
    if let Some(custom) = std::env::var_os("ABOT_ROOT") {
        return Ok(PathBuf::from(custom));
    }
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("HOME environment variable is not set"))?;
    Ok(PathBuf::from(home).join(".abot"))
}

/// `<root>/agents/` — where every agent's canonical repo lives.
pub fn agents_dir(root: &Path) -> PathBuf {
    root.join("agents")
}

/// `<root>/agents/<name>.abot/` — the canonical git repo for agent `<name>`.
///
/// The `.abot` suffix marks the directory as a self-describing agent artifact,
/// the way `.app` marks a macOS application bundle.
pub fn agent_dir(root: &Path, name: &str) -> PathBuf {
    agents_dir(root).join(format!("{name}.abot"))
}

/// `<root>/kubos/` — where every room's worktree directory lives.
pub fn kubos_dir(root: &Path) -> PathBuf {
    root.join("kubos")
}

/// `<root>/kubos/<room>/` — a room directory; contains one subdir per employed agent.
pub fn kubo_dir(root: &Path, room: &str) -> PathBuf {
    kubos_dir(root).join(room)
}

/// `<root>/kubos/<room>/<name>/` — the git worktree for agent `<name>` in `<room>`.
///
/// No `.abot` suffix here: worktrees are not standalone identities.
pub fn agent_in_kubo(root: &Path, room: &str, name: &str) -> PathBuf {
    kubo_dir(root, room).join(name)
}

/// Branch name for a worktree binding agent → room.
///
/// Always prefixed `kubo/` so worktree branches never collide with `main`
/// (or whatever the user's `init.defaultBranch` is).
pub fn room_branch(room: &str) -> String {
    format!("kubo/{room}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_root() -> PathBuf {
        PathBuf::from("/abot-test-root")
    }

    #[test]
    fn agents_dir_lives_directly_under_root() {
        assert_eq!(agents_dir(&fixed_root()), PathBuf::from("/abot-test-root/agents"));
    }

    #[test]
    fn agent_dir_uses_dot_abot_suffix() {
        assert_eq!(
            agent_dir(&fixed_root(), "alice"),
            PathBuf::from("/abot-test-root/agents/alice.abot")
        );
    }

    #[test]
    fn kubos_dir_lives_directly_under_root() {
        assert_eq!(kubos_dir(&fixed_root()), PathBuf::from("/abot-test-root/kubos"));
    }

    #[test]
    fn kubo_dir_lives_under_kubos() {
        assert_eq!(
            kubo_dir(&fixed_root(), "daily-room"),
            PathBuf::from("/abot-test-root/kubos/daily-room")
        );
    }

    #[test]
    fn agent_in_kubo_is_bare_name_no_suffix() {
        assert_eq!(
            agent_in_kubo(&fixed_root(), "daily-room", "alice"),
            PathBuf::from("/abot-test-root/kubos/daily-room/alice")
        );
    }

    #[test]
    fn room_branch_is_prefixed_kubo() {
        assert_eq!(room_branch("daily-room"), "kubo/daily-room");
    }
}
