//! Branch names: `<project>/<task>`, and nothing else.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskBranch {
    pub project: String,
    pub task: String,
}

impl fmt::Display for TaskBranch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.project, self.task)
    }
}

/// A project slug: lowercase letters, digits and dashes, starting with a
/// letter or digit. These are the stems of `state/projects/*.md`.
pub fn valid_slug(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_task(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !s.contains("..")
        && !s.ends_with('.')
        && !s.ends_with(".lock")
}

pub fn parse(s: &str) -> Result<TaskBranch, String> {
    let Some((project, task)) = s.split_once('/') else {
        return Err(format!(
            "`{s}` is a bare project name; a branch is <project>/<task>, e.g. {s}/fix-login \
             (git can't hold both `{s}` and `{s}/x`, and the project's memory is its state file)"
        ));
    };
    if !valid_slug(project) {
        return Err(format!(
            "`{project}` is not a project slug: use lowercase letters, digits and dashes"
        ));
    }
    if task.contains('/') {
        return Err(format!(
            "`{s}` has more than one `/`: a branch is exactly <project>/<task>"
        ));
    }
    if !valid_task(task) {
        return Err(format!(
            "`{task}` is not a usable task name: use letters, digits, `-`, `_` and `.`, starting with a letter or digit"
        ));
    }
    Ok(TaskBranch { project: project.into(), task: task.into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_project_and_task() {
        let b = parse("everyday-vet/group-thread").unwrap();
        assert_eq!(b.project, "everyday-vet");
        assert_eq!(b.task, "group-thread");
        assert_eq!(b.to_string(), "everyday-vet/group-thread");
    }

    #[test]
    fn refuses_bare_project() {
        let e = parse("everyday-vet").unwrap_err();
        assert!(e.contains("bare project"), "{e}");
    }

    #[test]
    fn refuses_nested_and_bad_names() {
        assert!(parse("org/a/b").is_err());
        assert!(parse("Org/x").is_err());
        assert!(parse("org/").is_err());
        assert!(parse("/x").is_err());
        assert!(parse("org/-x").is_err());
        assert!(parse("org/x..y").is_err());
        assert!(parse("org/x.lock").is_err());
        assert!(parse("org/has space").is_err());
        assert!(parse("org/abot-smoke").is_ok());
        assert!(parse("org/v1.2_x").is_ok());
    }

    #[test]
    fn slugs() {
        assert!(valid_slug("org"));
        assert!(valid_slug("agent-tooling"));
        assert!(!valid_slug("-org"));
        assert!(!valid_slug(""));
        assert!(!valid_slug("Org"));
    }
}
