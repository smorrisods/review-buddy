//! File patches from `GET /pulls/:n/files`. See "Diffs" in docs/integrations.md.
//!
//! GitHub omits `patch` for binary files and for diffs it considers too large. Those come
//! back as a `FilePatch` with `patch: None` so the diff view can fall back to a stub and
//! offer "open in browser" rather than an empty hunk list.

use rb_core::{ChangeId, Error, FilePatch, FileStatus, Result};
use serde::Deserialize;

use crate::GithubClient;

/// GitHub caps a pull request at 3,000 files, which is 30 pages of 100.
const MAX_PAGES: usize = 30;

#[derive(Debug, Deserialize)]
struct FileEntry {
    filename: String,
    status: String,
    #[serde(default)]
    additions: u32,
    #[serde(default)]
    deletions: u32,
    patch: Option<String>,
    previous_filename: Option<String>,
}

pub(crate) fn repo_parts(id: &ChangeId) -> Result<(&str, &str)> {
    id.repo
        .split_once('/')
        .ok_or_else(|| Error::NotFound(format!("`{}` isn't an owner/name repository", id.repo)))
}

fn status(raw: &str) -> FileStatus {
    match raw {
        "added" | "copied" => FileStatus::Added,
        "removed" => FileStatus::Removed,
        "renamed" => FileStatus::Renamed,
        _ => FileStatus::Modified,
    }
}

fn map(entry: FileEntry) -> FilePatch {
    let status = status(&entry.status);
    FilePatch {
        path: entry.filename,
        old_path: entry
            .previous_filename
            .filter(|_| status == FileStatus::Renamed),
        status,
        adds: entry.additions,
        dels: entry.deletions,
        patch: entry.patch.filter(|p| !p.is_empty()),
    }
}

pub(crate) async fn files(client: &GithubClient, id: &ChangeId) -> Result<Vec<FilePatch>> {
    let (owner, name) = repo_parts(id)?;
    let path = format!(
        "/repos/{owner}/{name}/pulls/{}/files?per_page=100",
        id.number
    );
    let (pages, truncated) = client.get_pages(&path, MAX_PAGES).await?;
    let mut out = Vec::new();
    for raw in pages {
        let entries: Vec<FileEntry> = client.parse(&raw.body)?;
        out.extend(entries.into_iter().map(map));
    }
    if truncated {
        return Err(Error::Api(format!(
            "{} has more files than {} will load. Open it in the browser to see the rest",
            id.short_ref(),
            MAX_PAGES * 100
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(status: &str, previous: Option<&str>, patch: Option<&str>) -> FileEntry {
        FileEntry {
            filename: "a.rs".into(),
            status: status.into(),
            additions: 2,
            deletions: 1,
            patch: patch.map(String::from),
            previous_filename: previous.map(String::from),
        }
    }

    #[test]
    fn maps_statuses() {
        assert_eq!(status("added"), FileStatus::Added);
        assert_eq!(status("copied"), FileStatus::Added);
        assert_eq!(status("removed"), FileStatus::Removed);
        assert_eq!(status("renamed"), FileStatus::Renamed);
        assert_eq!(status("changed"), FileStatus::Modified);
        assert_eq!(status("unchanged"), FileStatus::Modified);
    }

    #[test]
    fn rename_keeps_old_path_and_empty_patch_is_none() {
        let p = map(entry("renamed", Some("old.rs"), Some("")));
        assert_eq!(p.old_path.as_deref(), Some("old.rs"));
        assert_eq!(p.patch, None);
        let p = map(entry("modified", None, Some("@@ -1 +1 @@\n-a\n+b")));
        assert_eq!(p.old_path, None);
        assert!(p.patch.is_some());
    }
}
