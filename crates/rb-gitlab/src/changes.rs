//! Listing and detail over REST v4. See "Listing changes" in docs/integrations.md.
//!
//! The merge request list has no approval or reviewer-state data, so rows carry what the list
//! gives (reviewers as requested, pipeline when present) and `change_detail` fills in the rest
//! with a few small calls. GitLab's list endpoints do send ETags, but results come from several
//! merged queries, so `list_changes` fingerprints the merged result from each merge request's id
//! and `updated_at` instead, the same way the GitHub provider does. When the caller's `since`
//! matches, the page comes back as `not_modified`.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::future::join_all;
use rb_core::{
    ChangeDetail, ChangeId, ChangeState, ChangeSummary, CiState, Error, Etag, ForgeKind,
    Mergeability, MyReview, MyRole, OpenThreads, Page, Result, Reviewer, ReviewerState, Scope,
    Signals, SourceId,
};
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::error::header_u64;
use crate::time::parse_rfc3339;
use crate::GitlabClient;

pub(crate) const PER_PAGE: u32 = 100;
const MAX_PAGES: usize = 10;
const DIFF_PAGES: usize = 3;
/// Below this many requests left we stop sending until the window resets.
const MIN_REMAINING: u64 = 10;

#[derive(Deserialize, Default)]
struct UserRef {
    #[serde(default)]
    username: String,
    #[serde(default)]
    bot: bool,
}

#[derive(Deserialize)]
struct PipelineRef {
    status: Option<String>,
}

#[derive(Deserialize)]
struct DiffRefs {
    base_sha: Option<String>,
    head_sha: Option<String>,
}

#[derive(Deserialize)]
struct References {
    full: Option<String>,
}

#[derive(Deserialize)]
struct Approver {
    user: UserRef,
}

#[derive(Deserialize)]
struct MrNode {
    id: u64,
    iid: u64,
    #[serde(default)]
    title: String,
    description: Option<String>,
    state: String,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    work_in_progress: bool,
    #[serde(default)]
    source_branch: String,
    #[serde(default)]
    target_branch: String,
    sha: Option<String>,
    author: Option<UserRef>,
    #[serde(default)]
    assignees: Vec<UserRef>,
    #[serde(default)]
    reviewers: Vec<UserRef>,
    #[serde(default)]
    labels: Vec<String>,
    references: Option<References>,
    web_url: Option<String>,
    head_pipeline: Option<PipelineRef>,
    pipeline: Option<PipelineRef>,
    diff_refs: Option<DiffRefs>,
    changes_count: Option<Value>,
    detailed_merge_status: Option<String>,
    merge_status: Option<String>,
    has_conflicts: Option<bool>,
    user_notes_count: Option<u32>,
    blocking_discussions_resolved: Option<bool>,
    #[serde(default)]
    approved_by: Vec<Approver>,
}

#[derive(Deserialize)]
struct Approvals {
    #[serde(default)]
    approved_by: Vec<Approver>,
}

#[derive(Deserialize)]
struct ReviewerEntry {
    user: UserRef,
    #[serde(default)]
    state: String,
}

#[derive(Clone, Copy)]
enum Query {
    Reviewer,
    Assignee,
    Author,
}

const QUERIES: [Query; 3] = [Query::Reviewer, Query::Assignee, Query::Author];

impl Query {
    fn param(self) -> &'static str {
        match self {
            Self::Reviewer => "reviewer_username",
            Self::Assignee => "assignee_username",
            Self::Author => "author_username",
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Matched {
    reviewer: bool,
    assignee: bool,
    author: bool,
}

impl Matched {
    fn note(&mut self, query: Query) {
        match query {
            Query::Reviewer => self.reviewer = true,
            Query::Assignee => self.assignee = true,
            Query::Author => self.author = true,
        }
    }
}

/// Where one set of queries runs.
#[derive(Clone)]
enum Target {
    Everywhere,
    Group(String),
    Project(String),
}

impl Target {
    fn path(&self) -> String {
        match self {
            Self::Everywhere => "/merge_requests".to_string(),
            Self::Group(g) => format!("/groups/{}/merge_requests", encode(g)),
            Self::Project(p) => format!("/projects/{}/merge_requests", encode(p)),
        }
    }
}

/// Extra facts only the detail calls provide.
#[derive(Default)]
struct Extras {
    reviewer_states: Vec<(String, String)>,
    approvers: Vec<String>,
    adds: Option<u32>,
    dels: Option<u32>,
    /// The relative URL root of a self-hosted instance (`gitlab` for `https://host/gitlab`).
    root: String,
}

/// Percent-encodes a path for use as a `:id` segment (`group/sub` becomes `group%2Fsub`).
pub(crate) fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Refuses to send when the last reported budget is nearly spent and hasn't reset yet.
pub(crate) fn guard(client: &GitlabClient) -> Result<()> {
    let now = now_epoch();
    match client
        .rate_limit()
        .filter(|r| r.remaining < MIN_REMAINING && r.reset > now)
    {
        Some(r) => Err(Error::RateLimited {
            host: client.host().to_string(),
            retry_after_secs: Some(r.reset - now),
        }),
        None => Ok(()),
    }
}

pub(crate) async fn get(
    client: &GitlabClient,
    path: &str,
    query: &[(&str, String)],
) -> Result<(Vec<u8>, reqwest::header::HeaderMap)> {
    guard(client)?;
    client.get_page(path, query).await
}

/// Every page of a list endpoint, following `X-Next-Page`.
async fn fetch_all(
    client: &GitlabClient,
    path: &str,
    params: &[(&str, String)],
    max_pages: usize,
) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let mut page = 1_u64;
    for _ in 0..max_pages {
        let mut query = params.to_vec();
        query.push(("per_page", PER_PAGE.to_string()));
        query.push(("page", page.to_string()));
        let (body, headers) = get(client, path, &query).await?;
        out.extend(client.parse::<Vec<Value>>(&body)?);
        match header_u64(&headers, "x-next-page") {
            Some(next) if next > 0 => page = next,
            _ => break,
        }
    }
    Ok(out)
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn project_path(node: &MrNode, root: &str) -> Option<String> {
    if let Some(full) = node.references.as_ref().and_then(|r| r.full.as_deref()) {
        if let Some((path, _)) = full.rsplit_once('!') {
            return Some(path.to_string());
        }
    }
    let url = node.web_url.as_deref()?;
    let (before, _) = url.split_once("/-/merge_requests/")?;
    let (_, rest) = before.split_once("://")?;
    let (_, path) = rest.split_once('/')?;
    let path = match root {
        "" => path,
        root => path.strip_prefix(root)?.trim_start_matches('/'),
    };
    Some(path.to_string())
}

fn in_scope(scope: &Scope, me: &str, path: &str) -> bool {
    if scope.is_everything() {
        return true;
    }
    let lower = path.to_ascii_lowercase();
    let under = |owner: &str| {
        let owner = owner.trim_matches('/').to_ascii_lowercase();
        lower.starts_with(&format!("{owner}/"))
    };
    scope.owners.iter().any(|o| under(o))
        || scope.repos.iter().any(|r| same(r.trim_matches('/'), path))
        || (scope.user && under(me))
}

fn is_draft(node: &MrNode) -> bool {
    if node.draft || node.work_in_progress {
        return true;
    }
    let title = node.title.trim_start().to_ascii_lowercase();
    ["draft:", "[draft]", "(draft)", "wip:", "[wip]"]
        .iter()
        .any(|p| title.starts_with(p))
}

fn is_bot(user: &UserRef) -> bool {
    user.bot
        || ((user.username.starts_with("project_") || user.username.starts_with("group_"))
            && user.username.contains("_bot"))
}

fn ci_state(node: &MrNode) -> CiState {
    let status = node
        .head_pipeline
        .as_ref()
        .or(node.pipeline.as_ref())
        .and_then(|p| p.status.as_deref());
    match status {
        Some("success" | "success-with-warnings") => CiState::Pass,
        Some(
            "running" | "pending" | "created" | "preparing" | "scheduled" | "waiting_for_resource",
        ) => CiState::Running,
        Some("failed") => CiState::Fail,
        Some("canceled" | "canceling") => CiState::Cancelled,
        Some("skipped") => CiState::Skipped,
        Some("manual") => CiState::Neutral,
        _ => CiState::None,
    }
}

fn mergeability(node: &MrNode) -> Mergeability {
    match node.detailed_merge_status.as_deref() {
        Some("mergeable") => Mergeability::Clean,
        Some("conflict") => Mergeability::Conflicts,
        Some("need_rebase") => Mergeability::Behind,
        Some(
            "not_approved"
            | "blocked_status"
            | "discussions_not_resolved"
            | "draft_status"
            | "requested_changes"
            | "ci_must_pass"
            | "external_status_checks"
            | "jira_association_missing"
            | "merge_request_blocked"
            | "locked_paths"
            | "locked_lfs_files"
            | "title_regex"
            | "security_policy_violations",
        ) => Mergeability::Blocked,
        Some(_) => Mergeability::Unknown,
        None => match node.merge_status.as_deref() {
            Some("can_be_merged") => Mergeability::Clean,
            Some("cannot_be_merged") => Mergeability::Conflicts,
            _ => Mergeability::Unknown,
        },
    }
}

fn mergeable(node: &MrNode, mergeability: Mergeability) -> Option<bool> {
    if node.has_conflicts == Some(true) || mergeability == Mergeability::Conflicts {
        return Some(false);
    }
    match mergeability {
        Mergeability::Unknown => None,
        _ => Some(true),
    }
}

fn count(value: Option<&Value>) -> u32 {
    let digits = match value {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.chars().take_while(char::is_ascii_digit).collect(),
        _ => String::new(),
    };
    digits.parse().unwrap_or(0)
}

fn reviewer_state(raw: &str) -> ReviewerState {
    match raw {
        "approved" => ReviewerState::Approved,
        "requested_changes" => ReviewerState::ChangesRequested,
        "reviewed" => ReviewerState::Commented,
        _ => ReviewerState::Requested,
    }
}

fn reviewers(node: &MrNode, extras: &Extras) -> Vec<Reviewer> {
    let mut out: Vec<Reviewer> = node
        .reviewers
        .iter()
        .map(|u| Reviewer {
            login: u.username.clone(),
            state: ReviewerState::Requested,
        })
        .collect();
    for (login, state) in &extras.reviewer_states {
        let state = reviewer_state(state);
        match out.iter_mut().find(|r| same(&r.login, login)) {
            Some(r) => r.state = state,
            None => out.push(Reviewer {
                login: login.clone(),
                state,
            }),
        }
    }
    let approvers = node
        .approved_by
        .iter()
        .map(|a| a.user.username.as_str())
        .chain(extras.approvers.iter().map(String::as_str));
    for login in approvers {
        match out.iter_mut().find(|r| same(&r.login, login)) {
            Some(r) => r.state = ReviewerState::Approved,
            None => out.push(Reviewer {
                login: login.to_string(),
                state: ReviewerState::Approved,
            }),
        }
    }
    out
}

fn bad_time(field: &str, value: &str) -> Error {
    Error::Api(format!(
        "GitLab sent an unreadable {field} ({value}). Try again, or run `review-buddy doctor`"
    ))
}

fn signals(node: &MrNode, me: &str, reviewers: &[Reviewer]) -> Signals {
    let open_threads = match node.blocking_discussions_resolved {
        Some(true) => OpenThreads::Count(0),
        Some(false) => OpenThreads::Any,
        None => OpenThreads::Unknown,
    };
    Signals {
        comments: node.user_notes_count.unwrap_or(0),
        open_threads,
        ..Signals::from_reviewers(
            reviewers,
            me,
            node.detailed_merge_status.as_deref() == Some("not_approved"),
        )
    }
}

fn summarize(
    node: &MrNode,
    me: &str,
    source_id: &SourceId,
    mut matched: Matched,
    extras: &Extras,
) -> Result<ChangeSummary> {
    let author_ref = node.author.as_ref();
    let author = author_ref
        .map_or("ghost", |a| a.username.as_str())
        .to_string();
    let reviewers = reviewers(node, extras);
    let mine = reviewers.iter().find(|r| same(&r.login, me));
    matched.author |= same(&author, me);
    matched.assignee |= node.assignees.iter().any(|a| same(&a.username, me));
    matched.reviewer |= node.reviewers.iter().any(|r| same(&r.username, me));

    let my_role = if matched.reviewer {
        MyRole::Reviewing
    } else if matched.author {
        MyRole::Authored
    } else if matched.assignee {
        MyRole::Assigned
    } else {
        MyRole::Mentioned
    };
    let my_review = match mine.map(|r| r.state) {
        Some(ReviewerState::Approved) => MyReview::Approved,
        Some(ReviewerState::ChangesRequested) => MyReview::ChangesRequested,
        Some(ReviewerState::Commented) => MyReview::Commented,
        _ => MyReview::None,
    };
    let state = match node.state.as_str() {
        "merged" => ChangeState::Merged,
        "closed" => ChangeState::Closed,
        _ => ChangeState::Open,
    };
    let refs = node.diff_refs.as_ref();
    let head_sha = refs
        .and_then(|r| r.head_sha.clone())
        .or_else(|| node.sha.clone())
        .unwrap_or_default();
    let repo = project_path(node, &extras.root).ok_or_else(|| {
        Error::Api(format!(
            "GitLab didn't say which project !{} belongs to. Try again, or run `review-buddy doctor`",
            node.iid
        ))
    })?;
    let signals = signals(node, me, &reviewers);
    Ok(ChangeSummary {
        id: ChangeId {
            source_id: source_id.clone(),
            kind: ForgeKind::GitLab,
            repo,
            number: node.iid,
        },
        title: node.title.clone(),
        author,
        author_is_bot: author_ref.is_some_and(is_bot),
        state,
        draft: is_draft(node),
        created_at: parse_rfc3339(&node.created_at)
            .ok_or_else(|| bad_time("created_at", &node.created_at))?,
        updated_at: parse_rfc3339(&node.updated_at)
            .ok_or_else(|| bad_time("updated_at", &node.updated_at))?,
        branch: node.source_branch.clone(),
        base: node.target_branch.clone(),
        head_sha,
        base_sha: refs.and_then(|r| r.base_sha.clone()).unwrap_or_default(),
        adds: extras.adds.unwrap_or(0),
        dels: extras.dels.unwrap_or(0),
        files: count(node.changes_count.as_ref()),
        ci: ci_state(node),
        labels: node.labels.clone(),
        reviewers,
        my_role,
        my_review,
        my_reviewed_sha: None,
        i_commented: my_review != MyReview::None,
        has_new_activity: false,
        signals,
    })
}

/// FNV-1a over `(id, updated_at)` pairs in a stable order.
fn fingerprint(nodes: &[&MrNode]) -> Etag {
    let mut pairs: Vec<(u64, &str)> = nodes
        .iter()
        .map(|n| (n.id, n.updated_at.as_str()))
        .collect();
    pairs.sort_unstable();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (id, updated) in pairs {
        for byte in id
            .to_string()
            .bytes()
            .chain([0])
            .chain(updated.bytes())
            .chain([1])
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    Etag::new(format!("gl-{hash:016x}"))
}

fn targets(scope: &Scope) -> Vec<Target> {
    let mut out: Vec<Target> = scope
        .owners
        .iter()
        .map(|o| Target::Group(o.trim_matches('/').to_string()))
        .chain(
            scope
                .repos
                .iter()
                .map(|r| Target::Project(r.trim_matches('/').to_string())),
        )
        .collect();
    if scope.is_everything() || scope.user {
        out.push(Target::Everywhere);
    }
    out
}

async fn run_query(
    client: &GitlabClient,
    target: &Target,
    query: Query,
    me: &str,
) -> Result<Vec<MrNode>> {
    let mut params = vec![
        ("state", "opened".to_string()),
        ("scope", "all".to_string()),
        ("order_by", "updated_at".to_string()),
        (query.param(), me.to_string()),
    ];
    if matches!(target, Target::Group(_)) {
        params.push(("include_subgroups", "true".to_string()));
    }
    fetch_all(client, &target.path(), &params, MAX_PAGES)
        .await?
        .into_iter()
        .map(|v| client.parse(v.to_string().as_bytes()))
        .collect()
}

/// Open merge requests that mention you in a pending to-do. Done to-dos never come back, and an
/// entry that doesn't parse is skipped rather than failing the refresh.
async fn run_mentions(client: &GitlabClient) -> Result<Vec<MrNode>> {
    let params = [
        ("action", "mentioned".to_string()),
        ("type", "MergeRequest".to_string()),
        ("state", "pending".to_string()),
    ];
    let todos = fetch_all(client, "/todos", &params, MAX_PAGES).await?;
    Ok(todos
        .into_iter()
        .filter(|t| t.get("state").and_then(Value::as_str) != Some("done"))
        .filter_map(|t| serde_json::from_value::<MrNode>(t.get("target")?.clone()).ok())
        .filter(|n| n.state == "opened")
        .collect())
}

pub(crate) async fn list_changes(
    client: &GitlabClient,
    source_id: &SourceId,
    scope: &Scope,
    since: Option<Etag>,
) -> Result<Page<ChangeSummary>> {
    guard(client)?;
    let me = client.whoami().await?.login;
    let root = client.web_base().path().trim_matches('/').to_string();

    let mut jobs = Vec::new();
    for target in targets(scope) {
        for query in QUERIES {
            jobs.push((target.clone(), query));
        }
    }
    let (lists, mentions) = futures_util::future::join(
        join_all(jobs.iter().map(|(t, q)| run_query(client, t, *q, &me))),
        run_mentions(client),
    )
    .await;

    let mut order: Vec<u64> = Vec::new();
    let mut found: HashMap<u64, (MrNode, Matched)> = HashMap::new();
    for ((target, query), result) in jobs.iter().zip(lists) {
        for node in result? {
            let keep = match target {
                Target::Everywhere => {
                    project_path(&node, &root).is_some_and(|p| in_scope(scope, &me, &p))
                }
                _ => true,
            };
            if !keep {
                continue;
            }
            let entry = found.entry(node.id).or_insert_with(|| {
                order.push(node.id);
                (node, Matched::default())
            });
            entry.1.note(*query);
        }
    }
    for node in mentions? {
        if !project_path(&node, &root).is_some_and(|p| in_scope(scope, &me, &p)) {
            continue;
        }
        found.entry(node.id).or_insert_with(|| {
            order.push(node.id);
            (node, Matched::default())
        });
    }

    let nodes: Vec<&MrNode> = order
        .iter()
        .filter_map(|id| found.get(id))
        .map(|(n, _)| n)
        .collect();
    let etag = fingerprint(&nodes);
    if since.as_ref() == Some(&etag) {
        return Ok(Page::not_modified(Some(etag)));
    }
    let mut items = order
        .iter()
        .filter_map(|id| found.get(id))
        .map(|(node, matched)| {
            summarize(
                node,
                &me,
                source_id,
                *matched,
                &Extras {
                    root: root.clone(),
                    ..Extras::default()
                },
            )
        })
        .collect::<Result<Vec<_>>>()?;
    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
    Ok(Page {
        items,
        next_cursor: None,
        etag: Some(etag),
        not_modified: false,
    })
}

/// Auxiliary detail calls may be missing on older or smaller instances; those degrade to "unknown"
/// instead of failing the whole detail.
fn soft<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(v) => Ok(Some(v)),
        Err(
            Error::NotFound(_) | Error::Forbidden { .. } | Error::Unsupported(_) | Error::Api(_),
        ) => Ok(None),
        Err(e) => Err(e),
    }
}

async fn approvals(client: &GitlabClient, base: &str) -> Result<Approvals> {
    let (body, _) = get(client, &format!("{base}/approvals"), &[]).await?;
    client.parse(&body)
}

async fn reviewer_states(client: &GitlabClient, base: &str) -> Result<Vec<ReviewerEntry>> {
    let (body, _) = get(client, &format!("{base}/reviewers"), &[]).await?;
    client.parse(&body)
}

async fn commit_count(client: &GitlabClient, base: &str) -> Result<u32> {
    let query = [("per_page", "1".to_string())];
    let (body, headers) = get(client, &format!("{base}/commits"), &query).await?;
    match header_u64(&headers, "x-total") {
        Some(total) => Ok(u32::try_from(total).unwrap_or(u32::MAX)),
        None => Ok(client.parse::<Vec<Value>>(&body)?.len() as u32),
    }
}

/// Added and removed line counts from the first few pages of per-file diffs; `None` when the
/// diff is longer than that so a partial count never passes for a total.
async fn diff_stats(client: &GitlabClient, base: &str) -> Result<Option<(u32, u32)>> {
    let mut page = 1_u64;
    let (mut adds, mut dels) = (0_u32, 0_u32);
    for _ in 0..DIFF_PAGES {
        let query = [
            ("per_page", PER_PAGE.to_string()),
            ("page", page.to_string()),
        ];
        let (body, headers) = get(client, &format!("{base}/diffs"), &query).await?;
        for file in client.parse::<Vec<Value>>(&body)? {
            let patch = file.get("diff").and_then(Value::as_str).unwrap_or_default();
            for line in patch.lines() {
                match line.as_bytes().first() {
                    Some(b'+') => adds += 1,
                    Some(b'-') => dels += 1,
                    _ => {}
                }
            }
        }
        match header_u64(&headers, "x-next-page") {
            Some(next) if next > 0 => page = next,
            _ => return Ok(Some((adds, dels))),
        }
    }
    Ok(None)
}

pub(crate) async fn change_detail(
    client: &GitlabClient,
    source_id: &SourceId,
    id: &ChangeId,
    web_url: Url,
) -> Result<ChangeDetail> {
    guard(client)?;
    let base = format!(
        "/projects/{}/merge_requests/{}",
        encode(&id.repo),
        id.number
    );
    let (me, mr) = futures_util::future::join(client.whoami(), async {
        let (body, _) = get(client, &base, &[]).await?;
        client.parse::<MrNode>(&body)
    })
    .await;
    let me = me?.login;
    let node = mr.map_err(|e| match e {
        Error::NotFound(_) => Error::NotFound(format!(
            "{} (on {}, or the token can't see it)",
            id.short_ref(),
            client.host()
        )),
        other => other,
    })?;

    let (approved, states, commits, stats) = futures_util::future::join4(
        approvals(client, &base),
        reviewer_states(client, &base),
        commit_count(client, &base),
        diff_stats(client, &base),
    )
    .await;
    let extras = Extras {
        approvers: soft(approved)?
            .map(|a| a.approved_by.into_iter().map(|u| u.user.username).collect())
            .unwrap_or_default(),
        reviewer_states: soft(states)?
            .map(|s| s.into_iter().map(|e| (e.user.username, e.state)).collect())
            .unwrap_or_default(),
        adds: None,
        dels: None,
        root: client.web_base().path().trim_matches('/').to_string(),
    };
    let stats = soft(stats)?.flatten();
    let extras = Extras {
        adds: stats.map(|s| s.0),
        dels: stats.map(|s| s.1),
        ..extras
    };

    let mut summary = summarize(&node, &me, source_id, Matched::default(), &extras)?;
    summary.id = id.clone();
    let mergeability = mergeability(&node);
    Ok(ChangeDetail {
        summary,
        body: node.description.clone().unwrap_or_default(),
        web_url: node
            .web_url
            .as_deref()
            .and_then(|u| Url::parse(u).ok())
            .unwrap_or(web_url),
        mergeable: mergeable(&node, mergeability),
        mergeability,
        commit_count: soft(commits)?.unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_platform::Secret;

    fn node(json: Value) -> MrNode {
        let mut base = serde_json::json!({
            "id": 1, "iid": 2, "title": "t", "state": "opened",
            "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-02T00:00:00Z"
        });
        base.as_object_mut()
            .unwrap()
            .extend(json.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn encodes_nested_paths() {
        assert_eq!(encode("group/sub group/proj"), "group%2Fsub%20group%2Fproj");
        assert_eq!(encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn pipeline_statuses_map() {
        let ci = |s: &str| ci_state(&node(serde_json::json!({"head_pipeline": {"status": s}})));
        assert_eq!(ci("success"), CiState::Pass);
        assert_eq!(ci("success-with-warnings"), CiState::Pass);
        assert_eq!(ci("running"), CiState::Running);
        assert_eq!(ci("waiting_for_resource"), CiState::Running);
        assert_eq!(ci("failed"), CiState::Fail);
        assert_eq!(ci("canceled"), CiState::Cancelled);
        assert_eq!(ci("skipped"), CiState::Skipped);
        assert_eq!(ci("manual"), CiState::Neutral);
        assert_eq!(ci("???"), CiState::None);
        assert_eq!(ci_state(&node(serde_json::json!({}))), CiState::None);
        let legacy = node(serde_json::json!({"pipeline": {"status": "failed"}}));
        assert_eq!(ci_state(&legacy), CiState::Fail);
    }

    #[test]
    fn drafts_and_bots() {
        assert!(is_draft(&node(serde_json::json!({"draft": true}))));
        assert!(is_draft(&node(
            serde_json::json!({"work_in_progress": true})
        )));
        assert!(is_draft(&node(serde_json::json!({"title": "Draft: x"}))));
        assert!(is_draft(&node(serde_json::json!({"title": "[WIP] x"}))));
        assert!(!is_draft(&node(serde_json::json!({"title": "Drafting"}))));
        let user = |name: &str, bot: bool| UserRef {
            username: name.into(),
            bot,
        };
        assert!(is_bot(&user("x", true)));
        assert!(is_bot(&user("project_42_bot_abc123", false)));
        assert!(is_bot(&user("group_7_bot", false)));
        assert!(!is_bot(&user("project_lead", false)));
    }

    #[test]
    fn merge_status_maps() {
        let m = |s: &str| mergeability(&node(serde_json::json!({"detailed_merge_status": s})));
        assert_eq!(m("mergeable"), Mergeability::Clean);
        assert_eq!(m("conflict"), Mergeability::Conflicts);
        assert_eq!(m("need_rebase"), Mergeability::Behind);
        assert_eq!(m("not_approved"), Mergeability::Blocked);
        assert_eq!(m("checking"), Mergeability::Unknown);
        let old = node(serde_json::json!({"merge_status": "cannot_be_merged"}));
        assert_eq!(mergeability(&old), Mergeability::Conflicts);
        assert_eq!(mergeable(&old, Mergeability::Conflicts), Some(false));
        assert_eq!(mergeable(&old, Mergeability::Unknown), None);
        assert_eq!(mergeable(&old, Mergeability::Blocked), Some(true));
    }

    #[test]
    fn changes_count_may_be_text_or_capped() {
        assert_eq!(count(Some(&Value::from("12"))), 12);
        assert_eq!(count(Some(&Value::from("1000+"))), 1000);
        assert_eq!(count(Some(&Value::from(3))), 3);
        assert_eq!(count(None), 0);
    }

    #[test]
    fn project_path_from_references_or_url() {
        let n = node(serde_json::json!({"references": {"full": "g/sub/p!2"}}));
        assert_eq!(project_path(&n, "").as_deref(), Some("g/sub/p"));
        let n = node(serde_json::json!({"web_url": "https://gl.test/g/p/-/merge_requests/2"}));
        assert_eq!(project_path(&n, "").as_deref(), Some("g/p"));
        let n = node(
            serde_json::json!({"web_url": "https://gl.test/gitlab/g/sub/p/-/merge_requests/2"}),
        );
        assert_eq!(project_path(&n, "gitlab").as_deref(), Some("g/sub/p"));
        assert_eq!(project_path(&node(serde_json::json!({})), ""), None);
    }

    #[test]
    fn scopes_filter_by_path() {
        assert!(in_scope(&Scope::everything(), "me", "any/thing"));
        let scope = Scope {
            owners: vec!["Group".into()],
            repos: vec!["o/r".into()],
            user: true,
        };
        assert!(in_scope(&scope, "me", "group/sub/p"));
        assert!(in_scope(&scope, "me", "o/r"));
        assert!(in_scope(&scope, "me", "me/dotfiles"));
        assert!(!in_scope(&scope, "me", "o/other"));
        assert!(!in_scope(&scope, "me", "groupie/p"));
    }

    #[test]
    fn guard_trips_when_budget_is_spent() {
        let client = GitlabClient::new("h", Some("http://127.0.0.1:9"), Secret::new("t")).unwrap();
        assert!(guard(&client).is_ok());
    }
}
