//! Listing and detail over GraphQL. See "Listing changes" in docs/integrations.md.
//!
//! GraphQL has no ETags, so `list_changes` fingerprints the result from each change's
//! node id and `updatedAt`. When the caller's `since` matches, the page comes back as
//! `not_modified` so nothing downstream is rebuilt. The searches still run, which costs
//! rate limit but not parsing or cache churn.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rb_core::{
    ChangeDetail, ChangeId, ChangeState, ChangeSummary, CiState, Error, Etag, ForgeKind,
    Mergeability, MyReview, MyRole, OpenThreads, Page, Result, Reviewer, ReviewerState, Scope,
    Signals, SourceId,
};
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::graphql::{self, QueryCost};
use crate::time::parse_rfc3339;
use crate::GithubClient;

const PAGE_SIZE: u32 = 50;
const MAX_PAGES: usize = 10;
/// Below this many points left we stop sending requests until the window resets.
const MIN_REMAINING: u64 = 25;

const PR_FIELDS: &str = r"
fragment PrFields on PullRequest {
  id number title isDraft state createdAt updatedAt
  headRefName baseRefName headRefOid baseRefOid
  additions deletions changedFiles
  author { __typename login }
  repository { nameWithOwner }
  labels(first: 20) { nodes { name } }
  assignees(first: 10) { nodes { login } }
  reviewRequests(first: 20) { nodes { requestedReviewer {
    __typename
    ... on User { login }
    ... on Team { slug }
  } } }
  latestReviews(first: 30) { nodes { author { login } state commit { oid } } }
  reviewDecision
  reviewThreads(first: 50) { totalCount nodes { isResolved } }
  comments(last: 30) { totalCount nodes { author { login } } }
  commitCount: commits { totalCount }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
}
";

const SEARCH_QUERY: &str = r"
query($q: String!, $first: Int!, $after: String) {
  rateLimit { cost remaining resetAt }
  viewer { login }
  search(query: $q, type: ISSUE, first: $first, after: $after) {
    pageInfo { hasNextPage endCursor }
    nodes { ... on PullRequest { ...PrFields } }
  }
}
";

const DETAIL_QUERY: &str = r"
query($owner: String!, $name: String!, $number: Int!) {
  rateLimit { cost remaining resetAt }
  viewer { login }
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) { ...PrFields body mergeable mergeStateStatus }
  }
}
";

#[derive(Clone, Copy)]
enum Search {
    ReviewRequested,
    ReviewedBy,
    Assignee,
    Author,
    Mentions,
}

const SEARCHES: [Search; 5] = [
    Search::ReviewRequested,
    Search::ReviewedBy,
    Search::Assignee,
    Search::Author,
    Search::Mentions,
];

impl Search {
    fn terms(self) -> &'static str {
        match self {
            Self::ReviewRequested => "review-requested:@me",
            Self::ReviewedBy => "reviewed-by:@me",
            Self::Assignee => "assignee:@me",
            Self::Author => "author:@me",
            Self::Mentions => "mentions:@me",
        }
    }
}

/// Which of the five searches (or, for a detail fetch, which node fields) tie you to a change.
#[derive(Clone, Copy, Default)]
struct Matched {
    review_requested: bool,
    reviewed_by: bool,
    assignee: bool,
    author: bool,
}

impl Matched {
    fn note(&mut self, search: Search) {
        match search {
            Search::ReviewRequested => self.review_requested = true,
            Search::ReviewedBy => self.reviewed_by = true,
            Search::Assignee => self.assignee = true,
            Search::Author => self.author = true,
            Search::Mentions => {}
        }
    }
}

#[derive(Deserialize)]
struct Viewer {
    login: String,
}

#[derive(Deserialize)]
struct Nodes<T> {
    #[serde(default = "Vec::new")]
    nodes: Vec<Option<T>>,
}

impl<T> Nodes<T> {
    fn iter(&self) -> impl Iterator<Item = &T> {
        self.nodes.iter().flatten()
    }
}

#[derive(Deserialize)]
struct Actor {
    #[serde(rename = "__typename")]
    typename: Option<String>,
    login: String,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct RequestNode {
    #[serde(rename = "requestedReviewer")]
    requested_reviewer: Option<RequestedReviewer>,
}

#[derive(Deserialize)]
struct RequestedReviewer {
    login: Option<String>,
    slug: Option<String>,
}

#[derive(Deserialize)]
struct ReviewNode {
    author: Option<Login>,
    state: String,
    commit: Option<Oid>,
}

#[derive(Deserialize)]
struct Oid {
    oid: String,
}

#[derive(Deserialize)]
struct CommentNode {
    author: Option<Login>,
}

/// A connection read for the comments' `nodes` and, when the query asked, its `totalCount`.
#[derive(Deserialize)]
struct CommentList {
    #[serde(default = "Vec::new")]
    nodes: Vec<Option<CommentNode>>,
    #[serde(rename = "totalCount")]
    total_count: Option<u32>,
}

impl CommentList {
    fn iter(&self) -> impl Iterator<Item = &CommentNode> {
        self.nodes.iter().flatten()
    }

    /// The forge's count, or the nodes in hand when the response didn't carry one.
    fn total(&self) -> u32 {
        self.total_count
            .unwrap_or_else(|| u32::try_from(self.nodes.len()).unwrap_or(u32::MAX))
    }
}

#[derive(Deserialize)]
struct ThreadList {
    #[serde(rename = "totalCount")]
    total_count: Option<u32>,
    #[serde(default = "Vec::new")]
    nodes: Vec<Option<ThreadNode>>,
}

impl ThreadList {
    fn iter(&self) -> impl Iterator<Item = &ThreadNode> {
        self.nodes.iter().flatten()
    }
}

#[derive(Deserialize)]
struct ThreadNode {
    #[serde(rename = "isResolved")]
    is_resolved: bool,
}

#[derive(Deserialize)]
struct CommitNode {
    commit: CommitInner,
}

#[derive(Deserialize)]
struct CommitInner {
    #[serde(rename = "statusCheckRollup")]
    status_check_rollup: Option<Rollup>,
}

#[derive(Deserialize)]
struct Rollup {
    state: String,
}

#[derive(Deserialize)]
struct RepoName {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrNode {
    id: String,
    number: u64,
    title: String,
    is_draft: bool,
    state: String,
    created_at: String,
    updated_at: String,
    head_ref_name: String,
    base_ref_name: String,
    head_ref_oid: String,
    base_ref_oid: String,
    additions: u32,
    deletions: u32,
    changed_files: u32,
    author: Option<Actor>,
    repository: RepoName,
    labels: Nodes<Named>,
    assignees: Nodes<Login>,
    review_requests: Nodes<RequestNode>,
    latest_reviews: Nodes<ReviewNode>,
    comments: CommentList,
    review_decision: Option<String>,
    review_threads: Option<ThreadList>,
    commits: Nodes<CommitNode>,
    body: Option<String>,
    mergeable: Option<String>,
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: Option<String>,
    #[serde(rename = "commitCount")]
    commit_count: Option<Total>,
}

#[derive(Deserialize)]
struct Total {
    #[serde(rename = "totalCount")]
    total_count: u32,
}

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct SearchBody {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct SearchData {
    viewer: Viewer,
    search: SearchBody,
}

#[derive(Deserialize)]
struct DetailData {
    viewer: Viewer,
    repository: Option<DetailRepo>,
}

#[derive(Deserialize)]
struct DetailRepo {
    #[serde(rename = "pullRequest")]
    pull_request: Option<PrNode>,
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Refuses to send (or continue) when the last reported budget is nearly spent.
fn guard(client: &GithubClient, cost: Option<&QueryCost>) -> Result<()> {
    let now = now_epoch();
    let mut low = client
        .rate_limit()
        .filter(|r| r.remaining < MIN_REMAINING && r.reset > now)
        .map(|r| r.reset - now);
    if let Some(c) = cost.filter(|c| c.remaining < MIN_REMAINING) {
        let reset = parse_rfc3339(&c.reset_at).map_or(0, |t| t.0.max(0) as u64);
        if reset > now {
            low = Some(low.map_or(reset - now, |l| l.max(reset - now)));
        }
    }
    match low {
        Some(secs) => Err(Error::RateLimited {
            host: client.host().to_string(),
            retry_after_secs: Some(secs),
        }),
        None => Ok(()),
    }
}

/// A `RATE_LIMITED` GraphQL error has no reset of its own; borrow the last header's.
fn with_reset(client: &GithubClient, e: Error) -> Error {
    match e {
        Error::RateLimited {
            host,
            retry_after_secs: None,
        } => Error::RateLimited {
            host,
            retry_after_secs: client
                .rate_limit()
                .map(|r| r.reset.saturating_sub(now_epoch())),
        },
        other => other,
    }
}

async fn run<T: serde::de::DeserializeOwned>(
    client: &GithubClient,
    query: &str,
    variables: serde_json::Value,
) -> Result<graphql::Response<T>> {
    guard(client, None)?;
    let query = format!("{query}{PR_FIELDS}");
    let response = graphql::query::<T>(client, &query, variables)
        .await
        .map_err(|e| with_reset(client, e))?;
    guard(client, response.cost.as_ref())?;
    Ok(response)
}

/// One search qualifier per entry; an everything scope has a single empty one.
fn scope_qualifiers(scope: &Scope) -> Vec<String> {
    let mut quals: Vec<String> = scope
        .owners
        .iter()
        .map(|o| format!("org:{o}"))
        .chain(scope.repos.iter().map(|r| format!("repo:{r}")))
        .collect();
    if scope.user {
        quals.push("user:@me".to_string());
    }
    if quals.is_empty() {
        quals.push(String::new());
    }
    quals
}

fn search_string(search: Search, qualifier: &str) -> String {
    let base = format!("is:pr is:open archived:false {}", search.terms());
    if qualifier.is_empty() {
        base
    } else {
        format!("{base} {qualifier}")
    }
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn bad_time(field: &str, value: &str) -> Error {
    Error::Api(format!(
        "GitHub sent an unreadable {field} ({value}). Try again, or run `review-buddy doctor`"
    ))
}

fn ci_state(node: &PrNode) -> CiState {
    let state = node
        .commits
        .iter()
        .last()
        .and_then(|c| c.commit.status_check_rollup.as_ref())
        .map(|r| r.state.as_str());
    match state {
        Some("SUCCESS") => CiState::Pass,
        Some("FAILURE" | "ERROR") => CiState::Fail,
        Some("PENDING" | "EXPECTED") => CiState::Running,
        _ => CiState::None,
    }
}

fn reviewers(node: &PrNode) -> Vec<Reviewer> {
    let mut out: Vec<Reviewer> = Vec::new();
    for r in node.review_requests.iter() {
        let Some(rr) = &r.requested_reviewer else {
            continue;
        };
        if let Some(login) = rr.login.as_ref().or(rr.slug.as_ref()) {
            out.push(Reviewer {
                login: login.clone(),
                state: ReviewerState::Requested,
            });
        }
    }
    for review in node.latest_reviews.iter() {
        let Some(author) = &review.author else {
            continue;
        };
        let state = match review.state.as_str() {
            "APPROVED" => ReviewerState::Approved,
            "CHANGES_REQUESTED" => ReviewerState::ChangesRequested,
            "COMMENTED" => ReviewerState::Commented,
            _ => continue,
        };
        // A fresh re-request outranks the earlier review.
        if !out.iter().any(|r| same(&r.login, &author.login)) {
            out.push(Reviewer {
                login: author.login.clone(),
                state,
            });
        }
    }
    out
}

/// A lower bound on the comments on the change, from what the list already fetches: the general
/// comments plus one for every review thread (a thread holds at least its first comment). The
/// per-thread totals would cost a connection under each thread, so they wait for the change's
/// details. With no review threads the count is exact. Review summary bodies are not counted.
fn comment_total(node: &PrNode) -> (u32, bool) {
    let general = node.comments.total();
    let Some(threads) = &node.review_threads else {
        return (general, false);
    };
    let fetched = u32::try_from(threads.nodes.len()).unwrap_or(u32::MAX);
    let count = threads.total_count.map_or(fetched, |t| t.max(fetched));
    (general.saturating_add(count), count > 0)
}

fn signals(node: &PrNode, me: &str, reviewers: &[Reviewer]) -> Signals {
    let open_threads = node
        .review_threads
        .as_ref()
        .map_or(OpenThreads::Unknown, |t| {
            let open = t.iter().filter(|t| !t.is_resolved).count();
            OpenThreads::Count(u32::try_from(open).unwrap_or(u32::MAX))
        });
    let (comments, comments_floor) = comment_total(node);
    let mut signals = Signals {
        comments,
        comments_floor,
        open_threads,
        ..Signals::from_reviewers(
            reviewers,
            me,
            node.review_decision.as_deref() == Some("REVIEW_REQUIRED"),
        )
    };
    signals.reconcile();
    signals
}

fn summarize(
    node: &PrNode,
    me: &str,
    source_id: &SourceId,
    mut matched: Matched,
) -> Result<ChangeSummary> {
    let author = node
        .author
        .as_ref()
        .map_or("ghost", |a| a.login.as_str())
        .to_string();
    let author_is_bot = node
        .author
        .as_ref()
        .is_some_and(|a| a.typename.as_deref() == Some("Bot") || a.login.ends_with("[bot]"));
    let mine = node
        .latest_reviews
        .iter()
        .find(|r| r.author.as_ref().is_some_and(|a| same(&a.login, me)));
    matched.author |= same(&author, me);
    matched.assignee |= node.assignees.iter().any(|a| same(&a.login, me));
    matched.reviewed_by |= mine.is_some();
    matched.review_requested |= node.review_requests.iter().any(|r| {
        r.requested_reviewer
            .as_ref()
            .and_then(|rr| rr.login.as_deref())
            .is_some_and(|l| same(l, me))
    });

    let my_role = if matched.review_requested || matched.reviewed_by {
        MyRole::Reviewing
    } else if matched.author {
        MyRole::Authored
    } else if matched.assignee {
        MyRole::Assigned
    } else {
        MyRole::Mentioned
    };
    let my_review = match mine.map(|r| r.state.as_str()) {
        Some("APPROVED") => MyReview::Approved,
        Some("CHANGES_REQUESTED") => MyReview::ChangesRequested,
        Some("COMMENTED") => MyReview::Commented,
        _ => MyReview::None,
    };
    let my_reviewed_sha = mine.and_then(|r| r.commit.as_ref().map(|c| c.oid.clone()));

    let i_commented = mine.is_some()
        || matched.reviewed_by
        || node
            .comments
            .iter()
            .any(|c| c.author.as_ref().is_some_and(|a| same(&a.login, me)));
    let pushed_since_review = my_reviewed_sha
        .as_deref()
        .is_some_and(|sha| sha != node.head_ref_oid);
    let others_spoke_last = (matched.author || i_commented)
        && node
            .comments
            .iter()
            .last()
            .and_then(|c| c.author.as_ref())
            .is_some_and(|a| !same(&a.login, me));

    let reviewers = reviewers(node);
    let state = match node.state.as_str() {
        "MERGED" => ChangeState::Merged,
        "CLOSED" => ChangeState::Closed,
        _ => ChangeState::Open,
    };
    let signals = signals(node, me, &reviewers);
    Ok(ChangeSummary {
        id: ChangeId {
            source_id: source_id.clone(),
            kind: ForgeKind::GitHub,
            repo: node.repository.name_with_owner.clone(),
            number: node.number,
        },
        title: node.title.clone(),
        author,
        author_is_bot,
        state,
        draft: node.is_draft,
        created_at: parse_rfc3339(&node.created_at)
            .ok_or_else(|| bad_time("createdAt", &node.created_at))?,
        updated_at: parse_rfc3339(&node.updated_at)
            .ok_or_else(|| bad_time("updatedAt", &node.updated_at))?,
        branch: node.head_ref_name.clone(),
        base: node.base_ref_name.clone(),
        head_sha: node.head_ref_oid.clone(),
        base_sha: node.base_ref_oid.clone(),
        adds: node.additions,
        dels: node.deletions,
        files: node.changed_files,
        ci: ci_state(node),
        labels: node.labels.iter().map(|l| l.name.clone()).collect(),
        reviewers,
        my_role,
        my_review,
        my_reviewed_sha,
        i_commented,
        has_new_activity: pushed_since_review || others_spoke_last,
        signals,
    })
}

/// FNV-1a over `(id, updatedAt)` pairs in a stable order.
fn fingerprint(nodes: &[&PrNode]) -> Etag {
    let mut pairs: Vec<(&str, &str)> = nodes
        .iter()
        .map(|n| (n.id.as_str(), n.updated_at.as_str()))
        .collect();
    pairs.sort_unstable();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (id, updated) in pairs {
        for byte in id.bytes().chain([0]).chain(updated.bytes()).chain([1]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    Etag::new(format!("gql-{hash:016x}"))
}

pub(crate) async fn list_changes(
    client: &GithubClient,
    source_id: &SourceId,
    scope: &Scope,
    since: Option<Etag>,
) -> Result<Page<ChangeSummary>> {
    let mut order: Vec<String> = Vec::new();
    let mut found: HashMap<String, (PrNode, Matched)> = HashMap::new();
    let mut me: Option<String> = None;
    let mut partial: Vec<String> = Vec::new();

    for qualifier in scope_qualifiers(scope) {
        for search in SEARCHES {
            let q = search_string(search, &qualifier);
            let mut after: Option<String> = None;
            for _ in 0..MAX_PAGES {
                let vars = json!({"q": q, "first": PAGE_SIZE, "after": after});
                let response = run::<SearchData>(client, SEARCH_QUERY, vars).await?;
                partial.extend(response.errors.iter().map(|e| e.message.clone()));
                let data = response.data;
                me.get_or_insert(data.viewer.login);
                for value in data.search.nodes {
                    if value.get("id").is_none() {
                        continue;
                    }
                    let node: PrNode = client.parse(value.to_string().as_bytes())?;
                    let entry = found.entry(node.id.clone()).or_insert_with(|| {
                        order.push(node.id.clone());
                        (node, Matched::default())
                    });
                    entry.1.note(search);
                }
                match data.search.page_info {
                    PageInfo {
                        has_next_page: true,
                        end_cursor: Some(cursor),
                    } => after = Some(cursor),
                    _ => break,
                }
            }
        }
    }

    if found.is_empty() {
        if let Some(message) = partial.first() {
            return Err(Error::Forbidden {
                host: client.host().to_string(),
                reason: format!(
                    "{message}. If your organisation uses SSO, authorise the token for it, then try again"
                ),
            });
        }
    }

    let me = me.unwrap_or_default();
    let nodes: Vec<&PrNode> = order
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
        .map(|(node, matched)| summarize(node, &me, source_id, *matched))
        .collect::<Result<Vec<_>>>()?;
    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
    Ok(Page {
        items,
        next_cursor: None,
        etag: Some(etag),
        not_modified: false,
    })
}

pub(crate) async fn change_detail(
    client: &GithubClient,
    source_id: &SourceId,
    id: &ChangeId,
    web_url: Url,
) -> Result<ChangeDetail> {
    let Some((owner, name)) = id.repo.split_once('/') else {
        return Err(Error::NotFound(format!(
            "`{}` isn't an owner/name repository",
            id.repo
        )));
    };
    let vars = json!({"owner": owner, "name": name, "number": id.number});
    let data = run::<DetailData>(client, DETAIL_QUERY, vars).await?.data;
    let Some(node) = data.repository.and_then(|r| r.pull_request) else {
        return Err(Error::NotFound(format!(
            "{} (on {}, or the token can't see it)",
            id.short_ref(),
            client.host()
        )));
    };
    let mut summary = summarize(&node, &data.viewer.login, source_id, Matched::default())?;
    summary.id = id.clone();
    Ok(ChangeDetail {
        summary,
        body: node.body.unwrap_or_default(),
        web_url,
        mergeable: match node.mergeable.as_deref() {
            Some("MERGEABLE") => Some(true),
            Some("CONFLICTING") => Some(false),
            _ => None,
        },
        mergeability: match node.merge_state_status.as_deref() {
            Some("CLEAN" | "HAS_HOOKS") => Mergeability::Clean,
            Some("DIRTY") => Mergeability::Conflicts,
            Some("BLOCKED") => Mergeability::Blocked,
            Some("BEHIND") => Mergeability::Behind,
            Some("UNSTABLE") => Mergeability::Unstable,
            _ => Mergeability::Unknown,
        },
        commit_count: node.commit_count.map_or(0, |c| c.total_count),
    })
}

#[cfg(test)]
mod tests {
    /// Every bounded connection in the list query. GitHub caps a query at 500,000 nodes and
    /// prices it at one point per 100 connection requests, where each connection under each of
    /// the 50 pull requests is one request. A `totalCount` read inside a connection that is
    /// already requested adds no connection, so the thread count is free.
    fn connection_bounds() -> Vec<u32> {
        let mut bounds = Vec::new();
        for line in PR_FIELDS.lines() {
            for part in line.split("(first: ").chain(line.split("(last: ")).skip(1) {
                let n: String = part.chars().take_while(char::is_ascii_digit).collect();
                if let Ok(n) = n.parse() {
                    bounds.push(n);
                }
            }
        }
        bounds
    }

    #[test]
    fn a_list_page_costs_what_it_did_before_the_comment_count() {
        let bounds = connection_bounds();
        // The count-only `commitCount: commits { totalCount }` has no bound but is a connection.
        let connections = bounds.len() as u32 + 1;
        let requests = 1 + PAGE_SIZE * connections;
        let nodes = PAGE_SIZE * (1 + bounds.iter().sum::<u32>() + 1);
        assert_eq!(connections, 8, "seven bounded connections and one count");
        assert_eq!(requests, 401, "connection requests a page");
        assert_eq!(nodes, 8_150, "nodes a page against a limit of 500,000");
        assert!(
            (f64::from(requests) / 100.0 - 4.01).abs() < 1e-9,
            "about 4 points"
        );
        assert!(
            !PR_FIELDS.contains("comments(first: 1)"),
            "no connection is read under each thread"
        );
    }

    fn node_with(general: u32, threads: &[bool]) -> PrNode {
        let file = format!("{}/tests/fixtures/author.json", env!("CARGO_MANIFEST_DIR"));
        let mut doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        let mut pr = doc["data"]["search"]["nodes"][0].take();
        pr["comments"] = serde_json::json!({"totalCount": general, "nodes": []});
        let nodes: Vec<_> = threads
            .iter()
            .map(|open| serde_json::json!({"isResolved": !open}))
            .collect();
        pr["reviewThreads"] = serde_json::json!({"totalCount": nodes.len(), "nodes": nodes});
        serde_json::from_value(pr).unwrap()
    }

    #[test]
    fn five_open_threads_and_no_general_comments_read_at_least_five() {
        let s = signals(&node_with(0, &[true; 5]), "octo", &[]);
        assert_eq!(s.open_threads, OpenThreads::Count(5));
        assert!(s.comments >= 5 && s.comments_floor, "{s:?}");
    }

    #[test]
    fn only_general_comments_are_an_exact_count() {
        let s = signals(&node_with(4, &[]), "octo", &[]);
        assert_eq!((s.comments, s.comments_floor), (4, false));
        let s = signals(&node_with(4, &[false, true]), "octo", &[]);
        assert_eq!((s.comments, s.comments_floor), (6, true));
    }

    #[test]
    fn thread_bounds_are_the_widest_connection() {
        assert_eq!(connection_bounds().into_iter().max(), Some(50));
    }

    use super::*;

    #[test]
    fn qualifiers_cover_scope_kinds() {
        assert_eq!(scope_qualifiers(&Scope::everything()), vec![String::new()]);
        let scope = Scope {
            owners: vec!["liminal-hq".into()],
            repos: vec!["o/r".into()],
            user: true,
        };
        assert_eq!(
            scope_qualifiers(&scope),
            vec!["org:liminal-hq", "repo:o/r", "user:@me"]
        );
    }

    #[test]
    fn search_strings_are_open_prs_with_scope() {
        assert_eq!(
            search_string(Search::Mentions, "org:x"),
            "is:pr is:open archived:false mentions:@me org:x"
        );
        assert_eq!(
            search_string(Search::Author, ""),
            "is:pr is:open archived:false author:@me"
        );
    }

    #[test]
    fn guard_trips_when_budget_is_spent() {
        let client = GithubClient::new(
            "h",
            Some("http://127.0.0.1:9"),
            rb_platform::Secret::new("t"),
        )
        .unwrap();
        assert!(guard(&client, None).is_ok());
        let cost = QueryCost {
            cost: 1,
            remaining: 3,
            reset_at: "2999-01-01T00:00:00Z".into(),
        };
        assert!(matches!(
            guard(&client, Some(&cost)),
            Err(Error::RateLimited {
                retry_after_secs: Some(_),
                ..
            })
        ));
        let spent = QueryCost {
            remaining: 3,
            reset_at: "2001-01-01T00:00:00Z".into(),
            ..cost
        };
        assert!(guard(&client, Some(&spent)).is_ok());
    }
}
