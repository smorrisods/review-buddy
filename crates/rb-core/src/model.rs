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

    /// The ref on the remote that holds the change's head, as `git fetch <remote> <ref>` takes it.
    pub fn checkout_refspec(&self) -> String {
        match self.kind {
            ForgeKind::GitHub => format!("pull/{}/head", self.number),
            ForgeKind::GitLab => format!("merge-requests/{}/head", self.number),
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
    /// Finished without a verdict either way (for example a GitHub "neutral" run).
    Neutral,
    /// Deliberately not run.
    Skipped,
    /// Stopped before it finished.
    Cancelled,
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

/// How many review threads are still open, as far as the forge's list said.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenThreads {
    /// Not loaded yet (or an older cache entry).
    #[default]
    Unknown,
    /// An exact count; GitHub counts the first 50 threads, so a longer list reads as a floor.
    Count(u32),
    /// Some are open but the list didn't say how many (GitLab's list sends only a flag).
    Any,
}

impl OpenThreads {
    /// `true` when at least one thread is known to be open.
    pub fn any_open(self) -> bool {
        matches!(self, Self::Count(n) if n > 0) || self == Self::Any
    }
}

/// What other people have done on a change, and how much talk it has, for the queue's status
/// cluster. Counts leave out the current user, whose own review is `my_review`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Signals {
    /// Comments of every kind: conversation, review and thread comments.
    pub comments: u32,
    pub open_threads: OpenThreads,
    /// Other people's approvals.
    pub approvals: u32,
    /// Other people whose latest review asks for changes.
    pub changes_requested: u32,
    /// Reviewers (other than you) who were asked and haven't answered.
    pub outstanding: u32,
    /// The forge says approval is still required to merge.
    pub review_required: bool,
}

impl Signals {
    /// The review part of the signals from the reviewer list. `me` is the current user's login.
    pub fn from_reviewers(reviewers: &[Reviewer], me: &str, review_required: bool) -> Self {
        let others = reviewers
            .iter()
            .filter(|r| !r.login.eq_ignore_ascii_case(me));
        let count = |state| {
            u32::try_from(others.clone().filter(|r| r.state == state).count()).unwrap_or(u32::MAX)
        };
        Self {
            approvals: count(ReviewerState::Approved),
            changes_requested: count(ReviewerState::ChangesRequested),
            outstanding: count(ReviewerState::Requested),
            review_required,
            ..Self::default()
        }
    }
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
    /// Other people's review state and the comment counts, for the queue's status cluster.
    #[serde(default)]
    pub signals: Signals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub summary: ChangeSummary,
    /// Markdown description.
    pub body: String,
    pub web_url: Url,
    /// `None` while the forge is still working it out.
    pub mergeable: Option<bool>,
    /// Finer merge readiness than `mergeable`.
    #[serde(default)]
    pub mergeability: Mergeability,
    /// Number of commits on the change; 0 when the forge didn't say.
    #[serde(default)]
    pub commit_count: u32,
}

/// Why a change can or can't be merged right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mergeability {
    #[default]
    Unknown,
    Clean,
    Conflicts,
    /// Waiting on required reviews or checks.
    Blocked,
    /// The branch is behind its base.
    Behind,
    /// Mergeable, but some non-required check isn't passing.
    Unstable,
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
    /// Part of a pending (draft) review that hasn't been submitted.
    #[serde(default)]
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: ThreadId,
    /// `None` for a conversation-level thread.
    pub path: Option<String>,
    /// Last line of the range (or the only line).
    pub line: Option<u32>,
    pub side: Side,
    /// First line of a range comment; `None` for a single-line thread.
    #[serde(default)]
    pub start_line: Option<u32>,
    /// Side of `start_line`; `None` means the same as `side`.
    #[serde(default)]
    pub start_side: Option<Side>,
    pub resolved: bool,
    pub outdated: bool,
    /// Every comment is part of a pending review.
    #[serde(default)]
    pub pending: bool,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub state: CiState,
    pub url: Option<Url>,
    #[serde(default)]
    pub started_at: Option<Timestamp>,
    #[serde(default)]
    pub completed_at: Option<Timestamp>,
    /// `None` when the forge doesn't say whether branch protection requires it.
    #[serde(default)]
    pub required: Option<bool>,
}

impl Check {
    /// Seconds between start and completion, when both are known.
    pub fn duration_secs(&self) -> Option<i64> {
        let (s, c) = (self.started_at?, self.completed_at?);
        (c.0 >= s.0).then_some(c.0 - s.0)
    }
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
    fn the_head_ref_follows_the_forge() {
        assert_eq!(id(ForgeKind::GitHub).checkout_refspec(), "pull/88/head");
        assert_eq!(
            id(ForgeKind::GitLab).checkout_refspec(),
            "merge-requests/88/head"
        );
    }

    #[test]
    fn check_duration_needs_both_ends() {
        let mut c = Check {
            name: "x".into(),
            state: CiState::Pass,
            url: None,
            started_at: Some(Timestamp(10)),
            completed_at: None,
            required: None,
        };
        assert_eq!(c.duration_secs(), None);
        c.completed_at = Some(Timestamp(70));
        assert_eq!(c.duration_secs(), Some(60));
        c.completed_at = Some(Timestamp(5));
        assert_eq!(c.duration_secs(), None);
    }

    #[test]
    fn old_rows_without_new_fields_deserialize() {
        let t: Thread = serde_json::from_str(
            r#"{"id":"t","path":null,"line":3,"side":"new","resolved":false,"outdated":false,"comments":[{"id":"c","author":"a","body":"b","created_at":1}]}"#,
        )
        .unwrap();
        assert_eq!(
            (t.start_line, t.pending, t.comments[0].pending),
            (None, false, false)
        );
        let c: Check = serde_json::from_str(r#"{"name":"n","state":"pass","url":null}"#).unwrap();
        assert_eq!((c.started_at, c.required), (None, None));
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

    fn reviewer(login: &str, state: ReviewerState) -> Reviewer {
        Reviewer {
            login: login.into(),
            state,
        }
    }

    #[test]
    fn signals_count_other_peoples_reviews_and_leave_you_out() {
        let reviewers = [
            reviewer("Octo", ReviewerState::Approved),
            reviewer("tess", ReviewerState::Approved),
            reviewer("mira", ReviewerState::ChangesRequested),
            reviewer("web-team", ReviewerState::Requested),
            reviewer("octo2", ReviewerState::Commented),
        ];
        let s = Signals::from_reviewers(&reviewers, "octo", true);
        assert_eq!((s.approvals, s.changes_requested, s.outstanding), (1, 1, 1));
        assert!(s.review_required);
        assert_eq!((s.comments, s.open_threads), (0, OpenThreads::Unknown));
    }

    #[test]
    fn open_threads_know_when_any_are_open() {
        assert!(!OpenThreads::Unknown.any_open());
        assert!(!OpenThreads::Count(0).any_open());
        assert!(OpenThreads::Count(2).any_open());
        assert!(OpenThreads::Any.any_open());
    }

    #[test]
    fn a_summary_without_signals_still_deserialises() {
        let json = r#"{
            "id": {"source_id": "s", "kind": "github", "repo": "o/r", "number": 1},
            "title": "t", "author": "a", "author_is_bot": false, "state": "open",
            "draft": false, "created_at": 1, "updated_at": 2, "branch": "b", "base": "main",
            "head_sha": "h", "base_sha": "b", "adds": 1, "dels": 2, "files": 3, "ci": "pass",
            "labels": [], "reviewers": [], "my_role": "reviewing", "my_review": "none",
            "my_reviewed_sha": null, "i_commented": false, "has_new_activity": false
        }"#;
        let old: ChangeSummary = serde_json::from_str(json).unwrap();
        assert_eq!(old.signals, Signals::default());
        let partial = r#"{"comments": 3}"#;
        let s: Signals = serde_json::from_str(partial).unwrap();
        assert_eq!((s.comments, s.open_threads), (3, OpenThreads::Unknown));
    }
}
