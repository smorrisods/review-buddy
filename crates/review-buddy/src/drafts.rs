//! Review drafts on disk: one JSON file per change under the state directory's `drafts/` folder.
//!
//! A draft is your own unsent text (comments, summary, chosen verdict), kept so it survives
//! leaving the diff and restarting the app. It is not the pending review a forge holds: that one
//! lives on the forge and shows up as pending threads. Nothing here is used in demo mode or when
//! `ui.drafts = "off"`; callers decide that by not passing a directory.
//!
//! Files are named from a stable hash of source id, repo and number, so no path or title leaks
//! into a directory listing. Reading never fails: a corrupt, unknown or misnamed file is skipped.

use std::io;
use std::path::{Path, PathBuf};

use rb_core::{ChangeId, DraftComment, ReviewDraft, Verdict};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;
const EXTENSION: &str = "json";

/// Where the cursor was, so reopening the change puts it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub path: String,
    pub hunk: u32,
    pub line: u32,
}

/// Everything saved for one change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDraft {
    pub version: u32,
    pub id: ChangeId,
    /// The change's title when it was written, for lists that can't look it up.
    #[serde(default)]
    pub title: String,
    /// The head commit the comments were written against; empty when unknown.
    #[serde(default)]
    pub head_sha: String,
    /// Seconds since the Unix epoch.
    pub written_at: i64,
    #[serde(default)]
    pub verdict: Option<Verdict>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub comments: Vec<DraftComment>,
    #[serde(default)]
    pub position: Option<Position>,
}

impl StoredDraft {
    pub fn new(
        id: ChangeId,
        title: String,
        head_sha: String,
        written_at: i64,
        draft: &ReviewDraft,
        verdict: Option<Verdict>,
    ) -> Self {
        Self {
            version: VERSION,
            id,
            title,
            head_sha,
            written_at,
            verdict,
            summary: draft.body.clone(),
            comments: draft.comments.clone(),
            position: None,
        }
    }

    pub fn draft(&self) -> ReviewDraft {
        ReviewDraft {
            body: self.summary.clone(),
            comments: self.comments.clone(),
        }
    }

    /// Whether there is anything worth keeping.
    pub fn is_empty(&self) -> bool {
        self.summary.trim().is_empty() && self.comments.is_empty()
    }

    /// Whether the change's head moved since this was written. Unknown SHAs never count.
    pub fn moved(&self, head_sha: &str) -> bool {
        !self.head_sha.is_empty() && !head_sha.is_empty() && self.head_sha != head_sha
    }

    /// The same content as `other`, ignoring when it was written and where the cursor was.
    pub fn same_content(&self, other: &Self) -> bool {
        self.id == other.id
            && self.head_sha == other.head_sha
            && self.verdict == other.verdict
            && self.summary == other.summary
            && self.comments == other.comments
    }
}

fn fnv(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = seed;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The file name for a change: 32 hex digits from a stable hash of source, repo and number.
pub fn file_name(id: &ChangeId) -> String {
    let key = format!("{}\0{}\0{}", id.source_id.as_str(), id.repo, id.number);
    let a = fnv(0xcbf2_9ce4_8422_2325, key.as_bytes());
    let b = fnv(0x8422_2325_cbf2_9ce4, key.as_bytes());
    format!("{a:016x}{b:016x}.{EXTENSION}")
}

pub fn path_of(dir: &Path, id: &ChangeId) -> PathBuf {
    dir.join(file_name(id))
}

/// Reads one draft file. `None` for anything that isn't a draft this version understands.
fn read(path: &Path) -> Option<StoredDraft> {
    let bytes = std::fs::read(path).ok()?;
    let draft: StoredDraft = serde_json::from_slice(&bytes).ok()?;
    let named_right =
        path.file_name().and_then(|n| n.to_str()) == Some(file_name(&draft.id).as_str());
    (draft.version == VERSION && named_right).then_some(draft)
}

/// Every readable draft in `dir`, newest first. A missing directory is an empty list.
pub fn load_all(dir: &Path) -> Vec<StoredDraft> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut all: Vec<StoredDraft> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
        .filter_map(|p| read(&p))
        .filter(|d| !d.is_empty())
        .collect();
    all.sort_by(|a, b| {
        b.written_at
            .cmp(&a.written_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    all
}

/// Writes a draft atomically, `0600` in a `0700` directory. Does nothing when the file already
/// holds the same text.
pub fn save(dir: &Path, draft: &StoredDraft) -> io::Result<()> {
    let text = serde_json::to_vec_pretty(draft).map_err(io::Error::other)?;
    let path = path_of(dir, &draft.id);
    if std::fs::read(&path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    rb_paths::ensure_private_dir(dir)?;
    let temp = path.with_extension("tmp");
    rb_paths::write_private_file(&temp, &text)?;
    std::fs::rename(&temp, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// Deletes the draft for `id`. A draft that isn't there is not an error.
pub fn remove(dir: &Path, id: &ChangeId) -> io::Result<()> {
    match std::fs::remove_file(path_of(dir, id)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Deletes every draft file (and leftover temp files) in `dir`; returns how many drafts went.
pub fn clear(dir: &Path) -> io::Result<usize> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(0);
    };
    let mut removed = 0;
    for path in entries.filter_map(Result::ok).map(|e| e.path()) {
        match path.extension().and_then(|e| e.to_str()) {
            Some(EXTENSION) => {
                std::fs::remove_file(&path)?;
                removed += 1;
            }
            Some("tmp") => {
                let _ = std::fs::remove_file(&path);
            }
            _ => {}
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::{ForgeKind, Side, SourceId};

    fn id(number: u64) -> ChangeId {
        ChangeId {
            source_id: SourceId::new("work"),
            kind: ForgeKind::GitHub,
            repo: "acme/widgets".into(),
            number,
        }
    }

    fn stored(number: u64, sha: &str, body: &str) -> StoredDraft {
        let draft = ReviewDraft {
            body: "Looks close".into(),
            comments: vec![DraftComment {
                path: "src/a.rs".into(),
                side: Side::New,
                start_line: Some(3),
                line: 5,
                body: body.into(),
            }],
        };
        StoredDraft::new(
            id(number),
            "Fix the thing".into(),
            sha.into(),
            1_700_000_000,
            &draft,
            Some(Verdict::Approve),
        )
    }

    #[test]
    fn a_draft_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let d = stored(7, "abc", "Why not a map?");
        save(dir.path(), &d).unwrap();
        assert_eq!(load_all(dir.path()), vec![d.clone()]);
        assert_eq!(d.draft().comments[0].start_line, Some(3));
    }

    #[test]
    fn names_are_stable_distinct_and_leak_nothing() {
        let a = file_name(&id(7));
        assert_eq!(a, file_name(&id(7)));
        assert_ne!(a, file_name(&id(8)));
        assert_eq!(a.len(), 32 + 1 + EXTENSION.len());
        assert!(!a.contains("acme") && !a.contains("work"));
        let mut other = id(7);
        other.source_id = SourceId::new("home");
        assert_ne!(a, file_name(&other));
        assert!(a[..32].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_head_moving_is_detected_only_when_both_shas_are_known() {
        let d = stored(1, "abc", "x");
        assert!(d.moved("def"));
        assert!(!d.moved("abc"));
        assert!(!d.moved(""));
        assert!(!stored(1, "", "x").moved("def"));
    }

    #[test]
    fn same_content_ignores_the_clock_and_the_cursor() {
        let a = stored(1, "abc", "x");
        let mut b = a.clone();
        b.written_at += 60;
        b.position = Some(Position {
            path: "src/a.rs".into(),
            hunk: 0,
            line: 2,
        });
        assert!(a.same_content(&b));
        b.comments[0].body = "y".into();
        assert!(!a.same_content(&b));
    }

    #[test]
    fn corrupt_unknown_and_misnamed_files_are_skipped_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let good = stored(1, "abc", "x");
        save(dir.path(), &good).unwrap();
        std::fs::write(dir.path().join("0123.json"), b"{ not json").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"hello").unwrap();
        let mut future = stored(2, "abc", "x");
        future.version = 99;
        std::fs::write(
            path_of(dir.path(), &future.id),
            serde_json::to_vec(&future).unwrap(),
        )
        .unwrap();
        let wrong = serde_json::to_vec(&stored(3, "abc", "x")).unwrap();
        std::fs::write(
            dir.path().join("ffffffffffffffffffffffffffffffff.json"),
            wrong,
        )
        .unwrap();
        assert_eq!(load_all(dir.path()), vec![good]);
        assert!(load_all(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn saving_twice_writes_once_and_removing_is_forgiving() {
        let dir = tempfile::tempdir().unwrap();
        let d = stored(1, "abc", "x");
        save(dir.path(), &d).unwrap();
        let first = std::fs::metadata(path_of(dir.path(), &d.id))
            .unwrap()
            .modified()
            .unwrap();
        save(dir.path(), &d).unwrap();
        let again = std::fs::metadata(path_of(dir.path(), &d.id))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(first, again);
        remove(dir.path(), &d.id).unwrap();
        remove(dir.path(), &d.id).unwrap();
        assert!(load_all(dir.path()).is_empty());
    }

    #[test]
    fn newest_comes_first_and_clear_removes_everything() {
        let dir = tempfile::tempdir().unwrap();
        let old = stored(1, "abc", "x");
        let mut new = stored(2, "abc", "y");
        new.written_at += 100;
        save(dir.path(), &old).unwrap();
        save(dir.path(), &new).unwrap();
        let ids: Vec<u64> = load_all(dir.path()).iter().map(|d| d.id.number).collect();
        assert_eq!(ids, vec![2, 1]);
        std::fs::write(dir.path().join("stale.tmp"), b"x").unwrap();
        assert_eq!(clear(dir.path()).unwrap(), 2);
        assert!(load_all(dir.path()).is_empty());
        assert!(!dir.path().join("stale.tmp").exists());
        assert_eq!(clear(&dir.path().join("missing")).unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private_and_the_directory_too() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let drafts = dir.path().join("state/drafts");
        let d = stored(1, "abc", "x");
        save(&drafts, &d).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&drafts), 0o700);
        assert_eq!(mode(&path_of(&drafts, &d.id)), 0o600);
    }

    #[test]
    fn a_draft_with_nothing_in_it_is_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = stored(1, "abc", "x");
        d.comments.clear();
        d.summary = "  ".into();
        save(dir.path(), &d).unwrap();
        assert!(load_all(dir.path()).is_empty());
    }
}
