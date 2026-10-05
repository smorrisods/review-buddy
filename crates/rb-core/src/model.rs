use std::fmt;

use serde::{Deserialize, Serialize};
use url::Url;

/// Seconds since the Unix epoch, UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// Whole seconds from `earlier` to `self`; negative when `earlier` is later.
    pub fn secs_since(self, earlier: Timestamp) -> i64 {
        self.0.saturating_sub(earlier.0)
    }
}

macro_rules! string_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(
    /// Stable identifier of a configured source.
    SourceId
);
string_id!(
    /// Forge-native identifier of a review thread.
    ThreadId
);
string_id!(
    /// Forge-native identifier of a comment.
    CommentId
);
string_id!(
    /// Opaque HTTP validator used for conditional refreshes.
    Etag
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeKind {
    GitHub,
    GitLab,
}

impl ForgeKind {
    /// The short tag shown in the queue: `GH` or `GL`.
    pub fn tag(self) -> &'static str {
        match self {
            Self::GitHub => "GH",
            Self::GitLab => "GL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    Cli,
    Token,
}

/// What a source covers. Empty lists and `user = false` mean "everything you can see".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    /// GitHub organisations or GitLab groups.
    pub owners: Vec<String>,
    /// `owner/name` GitHub repositories or GitLab project paths.
    pub repos: Vec<String>,
    /// GitHub: include the authenticated user's own repositories.
    pub user: bool,
}

impl Scope {
    pub fn everything() -> Self {
        Self::default()
    }

    pub fn is_everything(&self) -> bool {
        self.owners.is_empty() && self.repos.is_empty() && !self.user
    }
}

/// One authenticated scope on one host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub kind: ForgeKind,
    pub host: String,
    pub label: String,
    pub scope: Scope,
    pub auth: AuthMode,
    pub in_all: bool,
    pub include_drafts: bool,
    pub tag_colour: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct User {
    pub login: String,
    pub name: Option<String>,
}

/// Addresses one change on one source. `number` is the PR number or the MR iid.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChangeId {
    pub source_id: SourceId,
    pub kind: ForgeKind,
    pub repo: String,
    pub number: u64,
}

impl ChangeId {
    /// The native short reference: `owner/name#214` or `group/project!1182`.
    pub fn short_ref(&self) -> String {
        match self.kind {
            ForgeKind::GitHub => format!("{}#{}", self.repo, self.number),
            ForgeKind::GitLab => format!("{}!{}", self.repo, self.number),
        }
    }
}

impl fmt::Display for ChangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.short_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeState {
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiState {
    Pass,
    Running,
    Fail,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerState {
    Requested,
    Approved,
    Commented,
    ChangesRequested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reviewer {
    pub login: String,
    pub state: ReviewerState,
}

/// How the current user relates to a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MyRole {
    Reviewing,
    Assigned,
    Authored,
    Mentioned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MyReview {
    None,
    Approved,
    ChangesRequested,
    Commented,
}

/// Everything the queue needs about a change, without the body, files or threads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSummary {
    pub id: ChangeId,
    pub title: String,
    pub author: String,
    /// The forge flagged the author as a bot or app account.
    pub author_is_bot: bool,
    pub state: ChangeState,
    pub draft: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub branch: String,
    pub base: String,
    pub head_sha: String,
    pub base_sha: String,
    pub adds: u32,
    pub dels: u32,
    pub files: u32,
    pub ci: CiState,
    pub labels: Vec<String>,
    pub reviewers: Vec<Reviewer>,
    pub my_role: MyRole,
    pub my_review: MyReview,
    /// The head commit your latest review covered, if you have reviewed.
    pub my_reviewed_sha: Option<String>,
    /// You have left a comment on this change at some point.
    pub i_commented: bool,
    /// Something happened since you last looked (computed against the local seen marker).
    pub has_new_activity: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub summary: ChangeSummary,
    /// Markdown description.
    pub body: String,
    pub web_url: Url,
    /// `None` while the forge is still working it out.
    pub mergeable: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Old,
    New,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: CommentId,
    pub author: String,
    pub body: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: ThreadId,
    /// `None` for a conversation-level thread.
    pub path: Option<String>,
    pub line: Option<u32>,
    pub side: Side,
    pub resolved: bool,
    pub outdated: bool,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub state: CiState,
    pub url: Option<Url>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Modified,
    Removed,
    Renamed,
}

/// One changed file. Parsing the patch into hunks is `rb-diff`'s job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePatch {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub adds: u32,
    pub dels: u32,
    /// Raw unified diff body. `None` for binary or oversized files.
    pub patch: Option<String>,
}

/// A pending inline comment. A suggestion is a comment whose body holds a suggestion block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftComment {
    pub path: String,
    pub side: Side,
    /// First line of a range; `None` for a single-line comment.
    pub start_line: Option<u32>,
    pub line: u32,
    pub body: String,
}

/// Your pending set of comments on one change, submitted together with a verdict.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDraft {
    pub body: String,
    pub comments: Vec<DraftComment>,
}

impl ReviewDraft {
    pub fn is_empty(&self) -> bool {
        self.body.trim().is_empty() && self.comments.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeOpts {
    pub method: MergeMethod,
    pub delete_branch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeOutcome {
    Merged {
        sha: Option<String>,
    },
    /// Accepted but not merged yet (merge queue or merge when pipeline succeeds).
    Pending {
        reason: String,
    },
}

/// One page of results. `not_modified` is set when `since` matched and `items` is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub etag: Option<Etag>,
    pub not_modified: bool,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            next_cursor: None,
            etag: None,
            not_modified: false,
        }
    }

    pub fn not_modified(etag: Option<Etag>) -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
            etag,
            not_modified: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(kind: ForgeKind) -> ChangeId {
        ChangeId {
            source_id: SourceId::new("work"),
            kind,
            repo: "platform/flow".into(),
            number: 88,
        }
    }

    #[test]
    fn short_ref_uses_native_prefix() {
        assert_eq!(id(ForgeKind::GitHub).to_string(), "platform/flow#88");
        assert_eq!(id(ForgeKind::GitLab).to_string(), "platform/flow!88");
    }

    #[test]
    fn forge_tags() {
        assert_eq!(ForgeKind::GitHub.tag(), "GH");
        assert_eq!(ForgeKind::GitLab.tag(), "GL");
    }

    #[test]
    fn scope_everything() {
        assert!(Scope::everything().is_everything());
        let scoped = Scope {
            user: true,
            ..Scope::default()
        };
        assert!(!scoped.is_everything());
    }

    #[test]
    fn review_draft_emptiness() {
        let mut draft = ReviewDraft::default();
        assert!(draft.is_empty());
        draft.body = "  \n".into();
        assert!(draft.is_empty());
        draft.body = "Looks good".into();
        assert!(!draft.is_empty());
    }

    #[test]
    fn page_not_modified_is_empty() {
        let page: Page<u8> = Page::not_modified(Some(Etag::new("abc")));
        assert!(page.not_modified && page.items.is_empty());
        assert!(!Page::new(vec![1u8]).not_modified);
    }

    #[test]
    fn ids_round_trip_as_plain_strings() {
        let json = serde_json::to_string(&id(ForgeKind::GitLab)).unwrap();
        assert!(json.contains(r#""source_id":"work""#));
        assert!(json.contains(r#""kind":"gitlab""#));
        let back: ChangeId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id(ForgeKind::GitLab));
    }

    #[test]
    fn timestamp_difference_saturates() {
        assert_eq!(Timestamp(10).secs_since(Timestamp(4)), 6);
        assert_eq!(Timestamp(4).secs_since(Timestamp(10)), -6);
        assert_eq!(Timestamp(i64::MIN).secs_since(Timestamp(1)), i64::MIN);
    }
}
