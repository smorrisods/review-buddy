//! An in-memory `Provider`. Every write changes memory only and is confirmed with ` (demo)`.

use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use rb_core::{
    Capabilities, ChangeDetail, ChangeId, ChangeState, ChangeSummary, Check, CiState, Comment,
    CommentId, Error, Etag, FeatureAction, FilePatch, ForgeKind, MergeMethod, MergeOpts,
    MergeOutcome, MyReview, Page, Provider, Result, ReviewDraft, ReviewerState, Scope, Source,
    Thread, ThreadId, Timestamp, User, Verdict,
};
use url::Url;

use super::fixtures::{self, DemoChange};
use crate::app::{ChangeInfo, DiffData, Snapshot};

/// The label every visible demo confirmation ends with.
pub const DEMO_SUFFIX: &str = "(demo)";

#[derive(Debug)]
struct State {
    now: Timestamp,
    user: User,
    sources: Vec<Source>,
    changes: Vec<DemoChange>,
    confirmations: Vec<String>,
    version: u64,
    next_id: u64,
}

impl State {
    fn change(&self, id: &ChangeId) -> Result<&DemoChange> {
        self.changes
            .iter()
            .find(|c| c.id() == id)
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    fn change_mut(&mut self, id: &ChangeId) -> Result<&mut DemoChange> {
        self.changes
            .iter_mut()
            .find(|c| c.id() == id)
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    fn confirm(&mut self, text: impl std::fmt::Display) {
        self.confirmations.push(format!("{text} {DEMO_SUFFIX}"));
        self.version += 1;
    }

    fn take_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn host_of(&self, id: &ChangeId) -> String {
        self.sources
            .iter()
            .find(|s| s.id == id.source_id)
            .map_or_else(|| "demo.invalid".to_string(), |s| s.host.clone())
    }
}

/// The shared in-memory forge state behind the demo providers.
#[derive(Debug, Clone)]
pub struct DemoWorld {
    state: Arc<Mutex<State>>,
}

impl DemoWorld {
    pub fn new(now: Timestamp) -> anyhow::Result<Self> {
        let fixtures = fixtures::load(now)?;
        Ok(Self {
            state: Arc::new(Mutex::new(State {
                now,
                user: fixtures.user,
                sources: fixtures.sources,
                changes: fixtures.changes,
                confirmations: Vec::new(),
                version: 0,
                next_id: 100,
            })),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn provider(&self, kind: ForgeKind) -> DemoProvider {
        DemoProvider {
            kind,
            state: Arc::clone(&self.state),
        }
    }

    /// One provider per forge kind, as the app would hold them.
    pub fn providers(&self) -> Vec<Arc<dyn Provider>> {
        [ForgeKind::GitHub, ForgeKind::GitLab]
            .into_iter()
            .map(|kind| Arc::new(self.provider(kind)) as Arc<dyn Provider>)
            .collect()
    }

    pub fn now(&self) -> Timestamp {
        self.lock().now
    }

    pub fn sources(&self) -> Vec<Source> {
        self.lock().sources.clone()
    }

    /// Sets the state of one named check, as if the forge's CI had moved on, and keeps its
    /// start and finish times consistent with the demo clock. Returns false if the change or
    /// check doesn't exist.
    pub fn set_check_state(&self, id: &ChangeId, name: &str, to: CiState) -> bool {
        let mut state = self.lock();
        let now = state.now;
        let Ok(change) = state.change_mut(id) else {
            return false;
        };
        let Some(check) = change.checks.iter_mut().find(|c| c.name == name) else {
            return false;
        };
        check.state = to;
        if to == CiState::Running {
            check.started_at = Some(now);
            check.completed_at = None;
        } else {
            check.started_at.get_or_insert(now);
            check.completed_at = Some(now);
        }
        change.detail.summary.ci = worst_state(&change.checks, change.detail.summary.ci);
        true
    }

    /// Lets the first running check on a change finish passing. Returns false if none was running.
    pub fn settle_next_running(&self, id: &ChangeId) -> bool {
        let name = {
            let state = self.lock();
            let Ok(change) = state.change(id) else {
                return false;
            };
            change
                .checks
                .iter()
                .find(|c| c.state == CiState::Running)
                .map(|c| c.name.clone())
        };
        name.is_some_and(|n| self.set_check_state(id, &n, CiState::Pass))
    }

    /// Every confirmation shown so far, oldest first. Each ends with ` (demo)`.
    pub fn confirmations(&self) -> Vec<String> {
        self.lock().confirmations.clone()
    }

    /// Your pending, unsubmitted review comments on a change.
    pub fn draft(&self, id: &ChangeId) -> Option<ReviewDraft> {
        self.lock().change(id).ok().map(|c| c.draft.clone())
    }

    /// The patches and threads for one change, fetched through its provider, with your
    /// pending comments.
    pub async fn diff_data(&self, id: &ChangeId) -> Result<DiffData> {
        let provider = self.provider(id.kind);
        let files = provider.files(id).await?;
        let threads = provider.threads(id).await?;
        let draft = self.draft(id).unwrap_or_default();
        Ok(DiffData::new(files, threads, draft))
    }

    /// Loads sources and changes through the providers, the way the live app would.
    pub async fn snapshot(&self) -> Result<Snapshot> {
        let mut changes = Vec::new();
        let mut details = std::collections::HashMap::new();
        for provider in self.providers() {
            let page = provider.list_changes(&Scope::everything(), None).await?;
            for change in &page.items {
                let id = &change.id;
                let info = ChangeInfo {
                    body: provider.change_detail(id).await?.body,
                    checks: provider.checks(id).await?,
                    threads: provider.threads(id).await?,
                };
                details.insert(id.clone(), info);
            }
            changes.extend(page.items);
        }
        Ok(Snapshot {
            label: "demo".to_string(),
            sources: self.sources(),
            changes,
            now: self.now(),
            details,
        })
    }
}

#[derive(Debug, Clone)]
pub struct DemoProvider {
    kind: ForgeKind,
    state: Arc<Mutex<State>>,
}

impl DemoProvider {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn require(&self, action: FeatureAction) -> Result<()> {
        match self.capabilities().supports(action) {
            true => Ok(()),
            false => Err(Error::Unsupported(describe(action).to_string())),
        }
    }

    fn own<'a>(&self, state: &'a State, id: &ChangeId) -> Result<&'a DemoChange> {
        if id.kind != self.kind {
            return Err(Error::NotFound(id.to_string()));
        }
        state.change(id)
    }
}

fn describe(action: FeatureAction) -> &'static str {
    match action {
        FeatureAction::RequestChanges => "requesting changes",
        FeatureAction::ViewedFiles => "viewed files",
        FeatureAction::RangeComments => "range comments",
        FeatureAction::Suggestions => "suggestions",
        FeatureAction::ResolveThreads => "resolving threads",
        FeatureAction::RerunFailed => "re-running failed jobs",
    }
}

fn in_scope(change: &ChangeSummary, scope: &Scope, user: &str) -> bool {
    if scope.is_everything() {
        return true;
    }
    let repo = change.id.repo.as_str();
    let owner = repo.split('/').next().unwrap_or_default();
    scope.owners.iter().any(|o| o == owner)
        || scope.repos.iter().any(|r| r == repo)
        || (scope.user && owner == user)
}

fn worst_state(checks: &[Check], fallback: CiState) -> CiState {
    if checks.is_empty() {
        return fallback;
    }
    if checks.iter().any(|c| c.state == CiState::Fail) {
        CiState::Fail
    } else if checks.iter().any(|c| c.state == CiState::Running) {
        CiState::Running
    } else {
        CiState::Pass
    }
}

#[async_trait]
impl Provider for DemoProvider {
    fn kind(&self) -> ForgeKind {
        self.kind
    }

    async fn whoami(&self) -> Result<User> {
        Ok(self.lock().user.clone())
    }

    async fn list_changes(
        &self,
        scope: &Scope,
        since: Option<Etag>,
    ) -> Result<Page<ChangeSummary>> {
        let state = self.lock();
        let etag = Etag::new(format!("demo-{}", state.version));
        if since.as_ref() == Some(&etag) {
            return Ok(Page::not_modified(Some(etag)));
        }
        let items = state
            .changes
            .iter()
            .map(|c| &c.detail.summary)
            .filter(|s| s.id.kind == self.kind && in_scope(s, scope, &state.user.login))
            .cloned()
            .collect();
        let mut page = Page::new(items);
        page.etag = Some(etag);
        Ok(page)
    }

    async fn change_detail(&self, id: &ChangeId) -> Result<ChangeDetail> {
        let state = self.lock();
        Ok(self.own(&state, id)?.detail.clone())
    }

    async fn files(&self, id: &ChangeId) -> Result<Vec<FilePatch>> {
        let state = self.lock();
        Ok(self.own(&state, id)?.files.clone())
    }

    async fn threads(&self, id: &ChangeId) -> Result<Vec<Thread>> {
        let state = self.lock();
        Ok(self.own(&state, id)?.threads.clone())
    }

    async fn checks(&self, id: &ChangeId) -> Result<Vec<Check>> {
        let state = self.lock();
        Ok(self.own(&state, id)?.checks.clone())
    }

    async fn submit_review(
        &self,
        id: &ChangeId,
        review: &ReviewDraft,
        verdict: Verdict,
    ) -> Result<()> {
        if verdict == Verdict::RequestChanges {
            self.require(FeatureAction::RequestChanges)?;
        }
        let mut state = self.lock();
        self.own(&state, id)?;
        let me = state.user.login.clone();
        let now = state.now;
        let (mine, reviewer, phrase) = match verdict {
            Verdict::Approve => (MyReview::Approved, ReviewerState::Approved, "Approved"),
            Verdict::RequestChanges => (
                MyReview::ChangesRequested,
                ReviewerState::ChangesRequested,
                "Requested changes on",
            ),
            Verdict::Comment => (
                MyReview::Commented,
                ReviewerState::Commented,
                "Commented on",
            ),
        };
        let new_threads = review
            .comments
            .iter()
            .map(|draft| {
                let n = state.take_id();
                Thread {
                    id: ThreadId::new(format!("demo-thread-{n}")),
                    path: Some(draft.path.clone()),
                    line: Some(draft.line),
                    side: draft.side,
                    start_line: draft.start_line,
                    start_side: draft.start_line.map(|_| draft.side),
                    pending: false,
                    resolved: false,
                    outdated: false,
                    comments: vec![Comment {
                        id: CommentId::new(format!("demo-comment-{n}")),
                        author: me.clone(),
                        body: draft.body.clone(),
                        created_at: now,
                        pending: false,
                    }],
                }
            })
            .collect::<Vec<_>>();
        let change = state.change_mut(id)?;
        let summary = &mut change.detail.summary;
        summary.my_review = mine;
        summary.my_reviewed_sha = Some(summary.head_sha.clone());
        summary.has_new_activity = false;
        summary.i_commented |= verdict == Verdict::Comment
            || !review.body.trim().is_empty()
            || !review.comments.is_empty();
        match summary.reviewers.iter_mut().find(|r| r.login == me) {
            Some(r) => r.state = reviewer,
            None => summary.reviewers.push(rb_core::Reviewer {
                login: me,
                state: reviewer,
            }),
        }
        change.threads.extend(new_threads);
        change.draft = ReviewDraft::default();
        state.confirm(format!("{phrase} {id}"));
        Ok(())
    }

    async fn reply(&self, thread: &ThreadId, body: &str) -> Result<Comment> {
        if body.trim().is_empty() {
            return Err(Error::Api("a reply can't be empty".into()));
        }
        let mut state = self.lock();
        let n = state.take_id();
        let comment = Comment {
            id: CommentId::new(format!("demo-comment-{n}")),
            author: state.user.login.clone(),
            body: body.to_string(),
            created_at: state.now,
            pending: false,
        };
        let kind = self.kind;
        let change = state
            .changes
            .iter_mut()
            .filter(|c| c.id().kind == kind)
            .find(|c| c.threads.iter().any(|t| &t.id == thread))
            .ok_or_else(|| Error::NotFound(thread.to_string()))?;
        change.detail.summary.i_commented = true;
        let id = change.id().clone();
        if let Some(t) = change.threads.iter_mut().find(|t| &t.id == thread) {
            t.comments.push(comment.clone());
        }
        state.confirm(format!("Replied on {id}"));
        Ok(comment)
    }

    async fn resolve(&self, thread: &ThreadId, resolved: bool) -> Result<()> {
        self.require(FeatureAction::ResolveThreads)?;
        let mut state = self.lock();
        let kind = self.kind;
        let (id, target) = state
            .changes
            .iter_mut()
            .filter(|c| c.id().kind == kind)
            .find_map(|c| {
                let id = c.id().clone();
                c.threads
                    .iter_mut()
                    .find(|t| &t.id == thread)
                    .map(|t| (id, t))
            })
            .ok_or_else(|| Error::NotFound(thread.to_string()))?;
        target.resolved = resolved;
        let verb = if resolved { "Resolved" } else { "Reopened" };
        state.confirm(format!("{verb} a thread on {id}"));
        Ok(())
    }

    async fn merge(&self, id: &ChangeId, opts: &MergeOpts) -> Result<MergeOutcome> {
        let mut state = self.lock();
        self.own(&state, id)?;
        let change = state.change_mut(id)?;
        let summary = &mut change.detail.summary;
        if summary.state != ChangeState::Open {
            return Err(Error::Conflict(format!("{id} is already closed")));
        }
        if change.detail.mergeable == Some(false) {
            return Err(Error::Conflict(format!(
                "{id} has conflicts to resolve first"
            )));
        }
        if summary.ci == CiState::Fail {
            return Err(Error::Conflict(format!("checks are failing on {id}")));
        }
        if summary.ci == CiState::Running {
            state.confirm(format!("Queued the merge of {id}"));
            return Ok(MergeOutcome::Pending {
                reason: format!("Waiting for checks to finish {DEMO_SUFFIX}"),
            });
        }
        summary.state = ChangeState::Merged;
        let sha = summary.head_sha.clone();
        let method = match opts.method {
            MergeMethod::Merge => "merge commit",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
        };
        state.confirm(format!("Merged {id} with {method}"));
        Ok(MergeOutcome::Merged { sha: Some(sha) })
    }

    async fn rerun_failed(&self, id: &ChangeId) -> Result<()> {
        self.require(FeatureAction::RerunFailed)?;
        let mut state = self.lock();
        self.own(&state, id)?;
        let change = state.change_mut(id)?;
        let mut any = false;
        for check in change
            .checks
            .iter_mut()
            .filter(|c| c.state == CiState::Fail)
        {
            check.state = CiState::Running;
            any = true;
        }
        if !any {
            return Err(Error::Conflict(format!("{id} has no failed checks")));
        }
        change.detail.summary.ci = worst_state(&change.checks, change.detail.summary.ci);
        state.confirm(format!("Re-running failed checks on {id}"));
        Ok(())
    }

    fn checkout_refspec(&self, id: &ChangeId) -> String {
        match id.kind {
            ForgeKind::GitHub => format!("pull/{}/head", id.number),
            ForgeKind::GitLab => format!("merge-requests/{}/head", id.number),
        }
    }

    fn web_url(&self, id: &ChangeId) -> Url {
        let host = self.lock().host_of(id);
        fixtures::change_url(&host, id).unwrap_or_else(|_| {
            Url::parse("https://demo.invalid/").expect("a constant URL is valid")
        })
    }

    /// GitHub offers everything. The demo's GitLab can't request changes or track viewed files,
    /// so the UI can exercise hiding and explaining.
    fn capabilities(&self) -> Capabilities {
        match self.kind {
            ForgeKind::GitHub => Capabilities::all(),
            ForgeKind::GitLab => Capabilities {
                request_changes: false,
                viewed_files: false,
                ..Capabilities::all()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use rb_core::{Bucket, DraftComment, Reviewer, Side, TriageConfig};

    use super::*;
    use crate::demo::{parse_iso, Demo, DEFAULT_FROZEN};

    fn world() -> DemoWorld {
        DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap()
    }

    async fn find(world: &DemoWorld, number: u64) -> ChangeSummary {
        let snapshot = world.snapshot().await.unwrap();
        snapshot
            .changes
            .into_iter()
            .find(|c| c.id.number == number)
            .unwrap()
    }

    fn opts() -> MergeOpts {
        MergeOpts {
            method: MergeMethod::Squash,
            delete_branch: true,
        }
    }

    #[tokio::test]
    async fn lists_by_forge_kind_and_scope() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub);
        let gl = w.provider(ForgeKind::GitLab);
        let all = Scope::everything();
        let gh_items = gh.list_changes(&all, None).await.unwrap().items;
        let gl_items = gl.list_changes(&all, None).await.unwrap().items;
        assert_eq!((gh_items.len(), gl_items.len()), (4, 3));
        let scope = Scope {
            owners: vec!["platform".into()],
            ..Scope::default()
        };
        assert_eq!(gl.list_changes(&scope, None).await.unwrap().items.len(), 2);
        let mine = Scope {
            user: true,
            ..Scope::default()
        };
        assert_eq!(gh.list_changes(&mine, None).await.unwrap().items.len(), 1);
    }

    #[tokio::test]
    async fn snapshot_covers_every_bucket() {
        let w = world();
        let snapshot = w.snapshot().await.unwrap();
        assert_eq!(snapshot.changes.len(), 7);
        assert_eq!(snapshot.sources.len(), 4);
        for bucket in Bucket::ORDER {
            assert!(snapshot.changes.iter().any(|c| {
                rb_core::triage::bucket_for(c, &TriageConfig::default(), snapshot.now) == bucket
            }));
        }
    }

    #[tokio::test]
    async fn etag_changes_only_after_a_mutation() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub);
        let first = gh.list_changes(&Scope::everything(), None).await.unwrap();
        let again = gh
            .list_changes(&Scope::everything(), first.etag.clone())
            .await
            .unwrap();
        assert!(again.not_modified && again.items.is_empty());
        let id = find(&w, 209).await.id;
        gh.submit_review(&id, &ReviewDraft::default(), Verdict::Approve)
            .await
            .unwrap();
        let after = gh
            .list_changes(&Scope::everything(), first.etag)
            .await
            .unwrap();
        assert!(!after.not_modified);
    }

    #[tokio::test]
    async fn detail_files_threads_and_checks_are_served() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub);
        let id = find(&w, 214).await.id;
        assert_eq!(gh.change_detail(&id).await.unwrap().summary.id, id);
        assert_eq!(gh.files(&id).await.unwrap().len(), 3);
        let threads = gh.threads(&id).await.unwrap();
        assert_eq!(threads.len(), 2);
        let range = threads.iter().find(|t| t.start_line.is_some()).unwrap();
        assert_eq!((range.start_line, range.line), (Some(5), Some(7)));
        let checks = gh.checks(&id).await.unwrap();
        assert_eq!(checks.len(), 6);
        let state = |n: &str| checks.iter().find(|c| c.name == n).unwrap().state;
        assert_eq!(state("test (windows)"), CiState::Skipped);
        assert_eq!(state("coverage"), CiState::Neutral);
        assert_eq!(state("bench"), CiState::Cancelled);
        let fmt = checks.iter().find(|c| c.name == "fmt").unwrap();
        assert_eq!((fmt.duration_secs(), fmt.required), (Some(18), Some(true)));
        let wrong_forge = w.provider(ForgeKind::GitLab);
        assert!(matches!(
            wrong_forge.files(&id).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn approving_updates_memory_and_confirms_with_demo_suffix() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub);
        let before = find(&w, 214).await;
        assert_eq!(before.my_review, MyReview::None);
        let draft = w.draft(&before.id).unwrap();
        assert_eq!(draft.comments.len(), 1);

        gh.submit_review(&before.id, &draft, Verdict::Approve)
            .await
            .unwrap();

        let after = find(&w, 214).await;
        assert_eq!(after.my_review, MyReview::Approved);
        assert_eq!(
            after.my_reviewed_sha.as_deref(),
            Some(after.head_sha.as_str())
        );
        assert!(after.i_commented);
        assert!(after.reviewers.contains(&Reviewer {
            login: "smorris".into(),
            state: ReviewerState::Approved
        }));
        assert!(w.draft(&before.id).unwrap().is_empty());
        assert_eq!(gh.threads(&before.id).await.unwrap().len(), 3);
        assert_eq!(
            w.confirmations(),
            vec!["Approved liminal-hq/review-buddy#214 (demo)".to_string()]
        );
    }

    #[tokio::test]
    async fn gitlab_cannot_request_changes_but_github_can() {
        let w = world();
        let gl = w.provider(ForgeKind::GitLab);
        let id = find(&w, 1182).await.id;
        let err = gl
            .submit_review(&id, &ReviewDraft::default(), Verdict::RequestChanges)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
        assert!(w.confirmations().is_empty());
        gl.submit_review(&id, &ReviewDraft::default(), Verdict::Comment)
            .await
            .unwrap();

        let gh = w.provider(ForgeKind::GitHub);
        let gh_id = find(&w, 214).await.id;
        gh.submit_review(&gh_id, &ReviewDraft::default(), Verdict::RequestChanges)
            .await
            .unwrap();
        assert_eq!(find(&w, 214).await.my_review, MyReview::ChangesRequested);
    }

    #[test]
    fn capabilities_differ_by_forge() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub).capabilities();
        let gl = w.provider(ForgeKind::GitLab).capabilities();
        assert_ne!(gh, gl);
        assert!(gh.supports(FeatureAction::RequestChanges));
        assert_eq!(
            gl.explain_unsupported(FeatureAction::RequestChanges, "gitlab.com")
                .as_deref(),
            Some("gitlab.com doesn't support request changes. Leave a comment instead (c).")
        );
    }

    #[tokio::test]
    async fn submitted_draft_comments_become_threads() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub);
        let id = find(&w, 209).await.id;
        let draft = ReviewDraft {
            body: "Nice".into(),
            comments: vec![DraftComment {
                path: "crates/rb-theme/src/palette.rs".into(),
                side: Side::New,
                start_line: None,
                line: 42,
                body: "Could this share a constant?".into(),
            }],
        };
        gh.submit_review(&id, &draft, Verdict::Comment)
            .await
            .unwrap();
        let threads = gh.threads(&id).await.unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].comments[0].author, "smorris");
        assert_eq!(find(&w, 209).await.my_review, MyReview::Commented);
    }

    #[tokio::test]
    async fn replying_and_resolving_change_the_thread() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub);
        let id = find(&w, 214).await.id;
        let thread = gh.threads(&id).await.unwrap().remove(0).id;
        let comment = gh.reply(&thread, "Wrapping is fine by me.").await.unwrap();
        assert_eq!(comment.author, "smorris");
        let after = gh.threads(&id).await.unwrap().remove(0);
        assert_eq!(after.comments.len(), 3);
        assert_eq!(after.comments[2].body, "Wrapping is fine by me.");

        gh.resolve(&thread, true).await.unwrap();
        assert!(gh.threads(&id).await.unwrap()[0].resolved);
        gh.resolve(&thread, false).await.unwrap();
        assert!(!gh.threads(&id).await.unwrap()[0].resolved);
        assert!(w.confirmations().iter().all(|c| c.ends_with(" (demo)")));
        assert_eq!(w.confirmations().len(), 3);
    }

    #[tokio::test]
    async fn empty_replies_and_unknown_threads_are_refused() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub);
        let known = ThreadId::new("demo-thread-1");
        assert!(matches!(gh.reply(&known, "  ").await, Err(Error::Api(_))));
        let unknown = ThreadId::new("nope");
        assert!(matches!(
            gh.reply(&unknown, "hi").await,
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            gh.resolve(&unknown, true).await,
            Err(Error::NotFound(_))
        ));
        assert!(w.confirmations().is_empty());
    }

    #[tokio::test]
    async fn merge_depends_on_checks() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub);
        let gl = w.provider(ForgeKind::GitLab);

        let running = find(&w, 214).await.id;
        match gh.merge(&running, &opts()).await.unwrap() {
            MergeOutcome::Pending { reason } => assert!(reason.ends_with("(demo)")),
            other => panic!("expected pending, got {other:?}"),
        }
        assert_eq!(find(&w, 214).await.state, ChangeState::Open);

        let failing = find(&w, 1182).await.id;
        assert!(matches!(
            gl.merge(&failing, &opts()).await,
            Err(Error::Conflict(_))
        ));

        let green = find(&w, 209).await;
        let outcome = gh.merge(&green.id, &opts()).await.unwrap();
        assert_eq!(
            outcome,
            MergeOutcome::Merged {
                sha: Some(green.head_sha.clone())
            }
        );
        assert_eq!(find(&w, 209).await.state, ChangeState::Merged);
        assert!(matches!(
            gh.merge(&green.id, &opts()).await,
            Err(Error::Conflict(_))
        ));
        assert!(w
            .confirmations()
            .contains(&"Merged liminal-hq/review-buddy#209 with squash (demo)".to_string()));
    }

    #[tokio::test]
    async fn rerunning_failed_checks_moves_them_to_running() {
        let w = world();
        let gl = w.provider(ForgeKind::GitLab);
        let id = find(&w, 1182).await.id;
        gl.rerun_failed(&id).await.unwrap();
        assert_eq!(find(&w, 1182).await.ci, CiState::Running);
        let checks = gl.checks(&id).await.unwrap();
        assert!(checks.iter().all(|c| c.state != CiState::Fail));
        assert!(matches!(
            gl.rerun_failed(&id).await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            w.confirmations(),
            vec!["Re-running failed checks on platform/flow!1182 (demo)".to_string()]
        );
    }

    #[tokio::test]
    async fn refspecs_and_urls_follow_the_forge() {
        let w = world();
        let gh = w.provider(ForgeKind::GitHub);
        let gl = w.provider(ForgeKind::GitLab);
        let gh_id = find(&w, 214).await.id;
        let gl_id = find(&w, 1182).await.id;
        assert_eq!(gh.checkout_refspec(&gh_id), "pull/214/head");
        assert_eq!(gl.checkout_refspec(&gl_id), "merge-requests/1182/head");
        assert_eq!(
            gl.web_url(&gl_id).as_str(),
            "https://gitlab.platform.example/platform/flow/-/merge_requests/1182"
        );
        assert_eq!(gh.whoami().await.unwrap().login, "smorris");
    }

    #[tokio::test]
    async fn mutations_write_nothing_to_disk() {
        let demo = Demo::start(Some(DEFAULT_FROZEN)).unwrap();
        let home = tempfile::tempdir().unwrap();
        let w = &demo.world;
        let gh = w.provider(ForgeKind::GitHub);
        let id = find(w, 209).await.id;
        gh.submit_review(&id, &ReviewDraft::default(), Verdict::Approve)
            .await
            .unwrap();
        gh.merge(&id, &opts()).await.unwrap();
        let thread = gh.threads(&find(w, 214).await.id).await.unwrap().remove(0);
        gh.reply(&thread.id, "ok").await.unwrap();

        let entries = |p: std::path::PathBuf| std::fs::read_dir(p).unwrap().count();
        assert_eq!(entries(home.path().to_path_buf()), 0);
        for dir in [
            demo.env.config_dir(),
            demo.env.data_dir(),
            demo.env.cache_dir(),
            demo.env.state_dir(),
        ] {
            assert_eq!(entries(dir), 0);
        }
    }

    #[test]
    fn bad_frozen_time_is_a_helpful_error() {
        let err = Demo::start(Some("yesterday")).unwrap_err().to_string();
        assert!(err.contains("2026-10-05T10:00"), "{err}");
    }
}
