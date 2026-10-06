//! File patches from `GET …/merge_requests/:iid/diffs`. See "Diffs" in docs/integrations.md.
//!
//! GitLab's `diff` is hunks only (no `---`/`+++` header), the same shape the GitHub provider
//! hands to `rb-diff`. Binary files come with an empty `diff`, and files GitLab refuses to
//! inline are flagged `too_large`, `collapsed` or `generated_file`; all of those come back with
//! `patch: None` so the diff view shows a stub and offers "open in browser". Instances without
//! `/diffs` (before GitLab 15.7) answer 404, and then `/changes` is used instead.

use rb_core::{ChangeId, Error, FilePatch, FileStatus, Result};
use serde::Deserialize;

use crate::changes::get;
use crate::rest::{list, mr_base};
use crate::GitlabClient;

const PER_PAGE: u32 = 50;
/// GitLab stops listing at 1,000 files.
const MAX_PAGES: usize = 20;

#[derive(Debug, Deserialize)]
struct Entry {
    #[serde(default)]
    old_path: String,
    #[serde(default)]
    new_path: String,
    #[serde(default)]
    new_file: bool,
    #[serde(default)]
    renamed_file: bool,
    #[serde(default)]
    deleted_file: bool,
    #[serde(default)]
    diff: String,
    #[serde(default)]
    too_large: bool,
    #[serde(default)]
    collapsed: bool,
    #[serde(default)]
    generated_file: bool,
}

#[derive(Deserialize)]
struct Changes {
    #[serde(default)]
    changes: Vec<Entry>,
}

fn count(patch: &str) -> (u32, u32) {
    let (mut adds, mut dels) = (0, 0);
    for line in patch.lines() {
        match line.as_bytes().first() {
            Some(b'+') => adds += 1,
            Some(b'-') => dels += 1,
            _ => {}
        }
    }
    (adds, dels)
}

fn map(e: Entry) -> FilePatch {
    let status = if e.new_file {
        FileStatus::Added
    } else if e.deleted_file {
        FileStatus::Removed
    } else if e.renamed_file {
        FileStatus::Renamed
    } else {
        FileStatus::Modified
    };
    let hidden = e.too_large || e.collapsed || e.generated_file || e.diff.is_empty();
    let (adds, dels) = if hidden { (0, 0) } else { count(&e.diff) };
    let path = if e.new_path.is_empty() {
        e.old_path.clone()
    } else {
        e.new_path.clone()
    };
    FilePatch {
        old_path: (status == FileStatus::Renamed && e.old_path != path).then_some(e.old_path),
        path,
        status,
        adds,
        dels,
        patch: (!hidden).then_some(e.diff),
    }
}

pub(crate) async fn files(client: &GitlabClient, id: &ChangeId) -> Result<Vec<FilePatch>> {
    let base = mr_base(id);
    match list::<Entry>(client, &format!("{base}/diffs"), PER_PAGE, MAX_PAGES).await {
        Ok((entries, truncated)) => {
            if truncated {
                return Err(too_many(id));
            }
            Ok(entries.into_iter().map(map).collect())
        }
        Err(Error::NotFound(_)) => changes_fallback(client, &base).await,
        Err(e) => Err(e),
    }
}

async fn changes_fallback(client: &GitlabClient, base: &str) -> Result<Vec<FilePatch>> {
    let (body, _) = get(client, &format!("{base}/changes"), &[]).await?;
    let parsed: Changes = client.parse(&body)?;
    Ok(parsed.changes.into_iter().map(map).collect())
}

fn too_many(id: &ChangeId) -> Error {
    Error::Api(format!(
        "{} has more files than Review Buddy will load. Open it in the browser to see the rest",
        id.short_ref()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(f: impl FnOnce(&mut Entry)) -> Entry {
        let mut e = Entry {
            old_path: "a.rs".into(),
            new_path: "a.rs".into(),
            new_file: false,
            renamed_file: false,
            deleted_file: false,
            diff: "@@ -1 +1,2 @@\n-a\n+b\n+c\n\\ No newline at end of file\n".into(),
            too_large: false,
            collapsed: false,
            generated_file: false,
        };
        f(&mut e);
        e
    }

    #[test]
    fn counts_and_statuses() {
        let p = map(entry(|_| {}));
        assert_eq!((p.adds, p.dels, p.status), (2, 1, FileStatus::Modified));
        assert!(p.patch.is_some());
        let p = map(entry(|e| e.new_file = true));
        assert_eq!(p.status, FileStatus::Added);
        let p = map(entry(|e| e.deleted_file = true));
        assert_eq!(p.status, FileStatus::Removed);
    }

    #[test]
    fn rename_keeps_old_path() {
        let p = map(entry(|e| {
            e.renamed_file = true;
            e.old_path = "old.rs".into();
            e.new_path = "new.rs".into();
        }));
        assert_eq!(
            (p.path.as_str(), p.old_path.as_deref()),
            ("new.rs", Some("old.rs"))
        );
    }

    #[test]
    fn hidden_diffs_have_no_patch() {
        for f in [
            (|e: &mut Entry| e.too_large = true) as fn(&mut Entry),
            |e| e.collapsed = true,
            |e| e.generated_file = true,
            |e| e.diff.clear(),
        ] {
            let p = map(entry(f));
            assert_eq!((p.patch, p.adds, p.dels), (None, 0, 0));
        }
    }
}
