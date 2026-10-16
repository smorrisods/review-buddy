//! Viewed marks kept on this machine, for forges that have nowhere to keep them (GitLab): `viewed.json`
//! in the state directory. A mark belongs to the commit it was made on, so a change that has
//! moved on shows its files as not viewed again. Reading never fails: a missing or corrupt file
//! is empty. Demo mode passes no path.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use rb_core::ChangeId;
use serde::{Deserialize, Serialize};

const FILE: &str = "viewed.json";

/// The files of one change marked viewed at one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub id: ChangeId,
    pub head_sha: String,
    pub paths: BTreeSet<String>,
}

pub fn file_in(state_dir: &Path) -> PathBuf {
    state_dir.join(FILE)
}

pub fn load(path: &Path) -> Vec<Stored> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Writes the marks atomically, `0600` in a private directory.
pub fn save(path: &Path, entries: &[Stored]) -> io::Result<()> {
    let text = serde_json::to_vec_pretty(entries).map_err(io::Error::other)?;
    if std::fs::read(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        rb_paths::ensure_private_dir(dir)?;
    }
    let temp = path.with_extension("tmp");
    rb_paths::write_private_file(&temp, &text)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::{ForgeKind, SourceId};

    #[test]
    fn round_trips_and_tolerates_junk() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_in(&dir.path().join("state"));
        assert!(load(&path).is_empty());
        let entry = Stored {
            id: ChangeId {
                source_id: SourceId("s".into()),
                kind: ForgeKind::GitLab,
                repo: "g/p".into(),
                number: 3,
            },
            head_sha: "abc".into(),
            paths: ["a.rs".to_string()].into(),
        };
        save(&path, std::slice::from_ref(&entry)).unwrap();
        assert_eq!(load(&path), vec![entry]);
        std::fs::write(&path, "nope").unwrap();
        assert!(load(&path).is_empty());
    }
}
