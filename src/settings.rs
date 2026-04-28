//! Global abot settings stored as one-fact-per-file under `<root>/`.
//!
//! Matches humOS's `feedback_file_based_db` convention: each setting is its
//! own small file editable with `echo "value" > path`. No god JSON files.
//!
//! Current facts:
//!   `<root>/commit_email`  — applied as repo-local user.email on create
//!   `<root>/commit_name`   — applied as repo-local user.name  on create

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const COMMIT_EMAIL: &str = "commit_email";
const COMMIT_NAME: &str = "commit_name";

pub fn commit_email_path(root: &Path) -> PathBuf {
    root.join(COMMIT_EMAIL)
}

pub fn commit_name_path(root: &Path) -> PathBuf {
    root.join(COMMIT_NAME)
}

pub fn read_commit_email(root: &Path) -> Result<Option<String>> {
    read_fact(root, COMMIT_EMAIL)
}

pub fn read_commit_name(root: &Path) -> Result<Option<String>> {
    read_fact(root, COMMIT_NAME)
}

#[allow(dead_code)] // surface kept for symmetry; tests + future verbs may write
pub fn write_commit_email(root: &Path, value: &str) -> Result<()> {
    write_fact(root, COMMIT_EMAIL, value)
}

#[allow(dead_code)]
pub fn write_commit_name(root: &Path, value: &str) -> Result<()> {
    write_fact(root, COMMIT_NAME, value)
}

/// Read a single fact file. Returns Ok(None) when missing or empty after
/// trimming whitespace.
fn read_fact(root: &Path, name: &str) -> Result<Option<String>> {
    let p = root.join(name);
    if !p.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

fn write_fact(root: &Path, name: &str, value: &str) -> Result<()> {
    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    let p = root.join(name);
    let mut content = value.to_string();
    if !content.ends_with('\n') {
        content.push('\n');
    }
    std::fs::write(&p, content).with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_returns_none_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(read_commit_email(tmp.path()).unwrap(), None);
        assert_eq!(read_commit_name(tmp.path()).unwrap(), None);
    }

    #[test]
    fn round_trip_each_fact_independently() {
        let tmp = TempDir::new().unwrap();
        write_commit_email(tmp.path(), "felix@dorkyrobot.com").unwrap();
        write_commit_name(tmp.path(), "Felix Flores").unwrap();
        assert_eq!(
            read_commit_email(tmp.path()).unwrap(),
            Some("felix@dorkyrobot.com".into())
        );
        assert_eq!(
            read_commit_name(tmp.path()).unwrap(),
            Some("Felix Flores".into())
        );
    }

    #[test]
    fn read_trims_surrounding_whitespace() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(COMMIT_EMAIL), "  felix@x.com\n  ").unwrap();
        assert_eq!(
            read_commit_email(tmp.path()).unwrap(),
            Some("felix@x.com".into())
        );
    }

    #[test]
    fn read_treats_whitespace_only_as_missing() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(COMMIT_NAME), "\n   \n").unwrap();
        assert_eq!(read_commit_name(tmp.path()).unwrap(), None);
    }

    #[test]
    fn write_appends_newline_for_easy_cat() {
        let tmp = TempDir::new().unwrap();
        write_commit_email(tmp.path(), "x@y").unwrap();
        let raw = std::fs::read_to_string(tmp.path().join(COMMIT_EMAIL)).unwrap();
        assert_eq!(raw, "x@y\n");
    }
}
