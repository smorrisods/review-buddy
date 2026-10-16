//! Changes you flagged to come back to: `flagged.json` in the state directory.
//!
//! A flag is yours alone and never reaches a forge. Reading never fails: a missing or corrupt
//! file is an empty list. Demo mode passes no path, so nothing is read or written there.

use std::io;
use std::path::{Path, PathBuf};

use rb_core::ChangeId;

const FILE: &str = "flagged.json";

/// The path of the flag list under a state directory.
pub fn file_in(state_dir: &Path) -> PathBuf {
    state_dir.join(FILE)
}

pub fn load(path: &Path) -> Vec<ChangeId> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Writes the list atomically, `0600` in a private directory.
pub fn save(path: &Path, ids: &[ChangeId]) -> io::Result<()> {
    let text = serde_json::to_vec_pretty(ids).map_err(io::Error::other)?;
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

    fn id(n: u64) -> ChangeId {
        ChangeId {
            source_id: SourceId("gh".into()),
            kind: ForgeKind::GitHub,
            repo: "o/r".into(),
            number: n,
        }
    }

    #[test]
    fn round_trips_and_tolerates_junk() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_in(&dir.path().join("state"));
        assert!(load(&path).is_empty());
        save(&path, &[id(2), id(1)]).unwrap();
        assert_eq!(load(&path), vec![id(2), id(1)]);
        std::fs::write(&path, "not json").unwrap();
        assert!(load(&path).is_empty());
    }
}
