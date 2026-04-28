use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MANIFEST_FILE: &str = "manifest.json";

/// Identity-level facts about an agent. Lives at `<agent_dir>/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

impl Manifest {
    /// A fresh manifest stamped with the current time.
    pub fn new(name: &str) -> Self {
        let now = Utc::now();
        Self {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            created: now,
            updated: now,
        }
    }
}

pub fn read(agent_dir: &Path) -> Result<Manifest> {
    let path = agent_dir.join(MANIFEST_FILE);
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

pub fn write(agent_dir: &Path, manifest: &Manifest) -> Result<()> {
    let path = agent_dir.join(MANIFEST_FILE);
    let mut raw = serde_json::to_string_pretty(manifest).context("serializing manifest")?;
    raw.push('\n');
    std::fs::write(&path, raw).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn new_stamps_created_and_updated_to_the_same_instant() {
        let m = Manifest::new("alice");
        assert_eq!(m.name, "alice");
        assert_eq!(m.created, m.updated);
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        let original = Manifest::new("bob");
        write(dir, &original).unwrap();
        let read_back = read(dir).unwrap();
        assert_eq!(original, read_back);
    }

    #[test]
    fn read_missing_file_errors() {
        let tmp = TempDir::new().unwrap();
        let result = read(tmp.path());
        assert!(result.is_err());
    }
}
