//! Review writes over GraphQL: pending review, line threads, submit, reply and resolve.
//! See "Threads and comments" and "Submitting a review" in docs/integrations.md.
//!
//! A review is submitted as: find or create the viewer's pending review, add each draft
//! comment as a thread on it (skipping any already there, so a retry never duplicates),
//! then submit with the verdict. A failure after the pending review exists leaves it
//! pending on GitHub and says so; nothing is ever deleted.

use rb_core::{
    ChangeId, Comment, CommentId, DraftComment, Error, Result, ReviewDraft, Side, ThreadId,
    Timestamp, Verdict,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::files::repo_parts;
use crate::time::parse_rfc3339;
use crate::GithubClient;

/// One GitHub call a review submission makes, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedCall {
    /// Reuses your pending review on the change when GitHub already has one.
    OpenPendingReview,
    AddThread {
        path: String,
        line: u32,
        side: &'static str,
        start_line: Option<u32>,
        start_side: Option<&'static str>,
        body: String,
    },
    Submit {
        event: &'static str,
        body: String,
    },
}

/// What submitting a draft will do, for previews and tests. Pure: nothing is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewPlan {
    pub verdict: Verdict,
    pub calls: Vec<PlannedCall>,
}

impl ReviewPlan {
    pub fn comment_count(&self) -> usize {
        self.calls
            .iter()
            .filter(|c| matches!(c, PlannedCall::AddThread { .. }))
            .count()
    }

    /// One calm line for a confirm modal, such as "Approve with 2 comments".
    pub fn summary(&self) -> String {
        let action = match self.verdict {
            Verdict::Approve => "Approve",
            Verdict::RequestChanges => "Request changes",
            Verdict::Comment => "Post review",
        };
        match self.comment_count() {
            0 => action.to_string(),
            1 => format!("{action} with 1 comment"),
            n => format!("{action} with {n} comments"),
        }
    }
}

fn event_name(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approve => "APPROVE",
        Verdict::RequestChanges => "REQUEST_CHANGES",
        Verdict::Comment => "COMMENT",
    }
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Old => "LEFT",
        Side::New => "RIGHT",
    }
}

fn invalid(what: &str) -> Error {
    Error::Api(format!("{what}. Nothing was sent"))
}

/// Checks a draft and lists the calls that would submit it.
///
/// "Post now" for a single comment is a draft with that one comment and `Verdict::Comment`.
pub fn plan_review(review: &ReviewDraft, verdict: Verdict) -> Result<ReviewPlan> {
    let body = review.body.trim();
    if verdict == Verdict::RequestChanges && body.is_empty() {
        return Err(invalid(
            "Requesting changes needs a short summary. Add one and try again",
        ));
    }
    if verdict == Verdict::Comment && body.is_empty() && review.comments.is_empty() {
        return Err(invalid("There's nothing to post yet. Add a comment first"));
    }
    let mut calls = vec![PlannedCall::OpenPendingReview];
    for c in &review.comments {
        calls.push(plan_thread(c)?);
    }
    calls.push(PlannedCall::Submit {
        event: event_name(verdict),
        body: review.body.clone(),
    });
    Ok(ReviewPlan { verdict, calls })
}

fn plan_thread(c: &DraftComment) -> Result<PlannedCall> {
    if c.body.trim().is_empty() {
        return Err(invalid(&format!(
            "The comment on {}:{} is empty. Write something or remove it",
            c.path, c.line
        )));
    }
    if c.line == 0 || c.start_line.is_some_and(|s| s == 0 || s > c.line) {
        return Err(invalid(&format!(
            "The comment on {} has an odd line range. Reselect the lines and try again",
            c.path
        )));
    }
    let start_line = c.start_line.filter(|s| *s != c.line);
    Ok(PlannedCall::AddThread {
        path: c.path.clone(),
        line: c.line,
        side: side_name(c.side),
        start_line,
        start_side: start_line.map(|_| side_name(c.side)),
        body: c.body.clone(),
    })
}

const FIND_PENDING: &str = r"
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      id
      reviews(states: [PENDING], first: 5) {
        nodes {
          id
          comments(first: 100) { nodes { path line body } }
        }
      }
    }
  }
}
";

const CREATE_REVIEW: &str = r"
mutation($input: AddPullRequestReviewInput!) {
  addPullRequestReview(input: $input) { pullRequestReview { id } }
}
";

const ADD_THREAD: &str = r"
mutation($input: AddPullRequestReviewThreadInput!) {
  addPullRequestReviewThread(input: $input) { thread { id } }
}
";

const SUBMIT: &str = r"
mutation($input: SubmitPullRequestReviewInput!) {
  submitPullRequestReview(input: $input) { pullRequestReview { id } }
}
";

const REPLY: &str = r"
mutation($input: AddPullRequestReviewThreadReplyInput!) {
  addPullRequestReviewThreadReply(input: $input) {
    comment { id author { login } body createdAt state }
  }
}
";

const RESOLVE: &str = r"
mutation($input: ResolveReviewThreadInput!) {
  resolveReviewThread(input: $input) { thread { id isResolved } }
}
";

const UNRESOLVE: &str = r"
mutation($input: UnresolveReviewThreadInput!) {
  unresolveReviewThread(input: $input) { thread { id isResolved } }
}
";

#[derive(Deserialize)]
struct FindData {
    repository: Option<FindRepo>,
}

#[derive(Deserialize)]
struct FindRepo {
    #[serde(rename = "pullRequest")]
    pull_request: Option<FindPr>,
}

#[derive(Deserialize)]
struct FindPr {
    id: String,
    reviews: Nodes<PendingReview>,
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<T>,
}

#[derive(Deserialize)]
struct PendingReview {
    id: String,
    comments: Nodes<ExistingComment>,
}

#[derive(Deserialize)]
struct ExistingComment {
    path: String,
    line: Option<u32>,
    body: String,
}

/// Sends a mutation. Any GraphQL `errors` entry fails the call, even next to partial data.
async fn mutate(client: &GithubClient, query: &str, input: Value) -> Result<Value> {
    let body = json!({ "query": query, "variables": { "input": input } });
    send_graphql(client, body).await
}

async fn send_graphql(client: &GithubClient, body: Value) -> Result<Value> {
    let raw = client
        .send(client.graphql_request().json(&body))
        .await
        .map_err(write_error)?;
    let envelope: Value = client.parse(&raw.body)?;
    if let Some(first) = envelope
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|e| e.first())
    {
        return Err(graphql_write_error(client.host(), first));
    }
    match envelope.get("data") {
        Some(data) if !data.is_null() => Ok(data.clone()),
        _ => Err(Error::Api(format!(
            "{} returned no data. Try again",
            client.host()
        ))),
    }
}

fn line_moved() -> Error {
    Error::Conflict(
        "that line moved or isn't part of the diff any more. Refresh the diff and place the comment again"
            .to_string(),
    )
}

fn no_review_access(host: &str) -> Error {
    Error::Forbidden {
        host: host.to_string(),
        reason: "your token can read but not review here. Add the `repo` scope (or, for a fine-grained token, pull request write access) and try again".to_string(),
    }
}

fn gone() -> Error {
    Error::NotFound(
        "that pull request or repository is gone, or your token can't see it. Check the address and your access"
            .to_string(),
    )
}

/// Rewrites generic read-oriented errors with copy that fits a write.
fn write_error(e: Error) -> Error {
    match e {
        Error::Forbidden { host, reason } if !reason.contains("SSO") => no_review_access(&host),
        Error::NotFound(_) => gone(),
        Error::Api(msg) if msg.contains("answered 422") => unprocessable(msg),
        other => other,
    }
}

fn unprocessable(msg: String) -> Error {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("line") || lower.contains("diff") || lower.contains("position") {
        line_moved()
    } else {
        let detail = msg.split_once(": ").map_or(msg.as_str(), |(_, d)| d);
        Error::Api(format!(
            "GitHub couldn't accept that ({detail}). Check the text and try again"
        ))
    }
}

fn graphql_write_error(host: &str, err: &Value) -> Error {
    let message = err
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let lower = message.to_ascii_lowercase();
    match err.get("type").and_then(Value::as_str) {
        Some("NOT_FOUND") => gone(),
        Some("RATE_LIMITED") => Error::RateLimited {
            host: host.to_string(),
            retry_after_secs: None,
        },
        Some("FORBIDDEN") | Some("INSUFFICIENT_SCOPES") => no_review_access(host),
        _ if lower.contains("diff") || lower.contains("line") || lower.contains("position") => {
            line_moved()
        }
        _ if lower.contains("resource not accessible") => no_review_access(host),
        _ if lower.contains("own pull request") => Error::Conflict(
            "GitHub doesn't let you approve or request changes on your own pull request. Post it as a comment instead"
                .to_string(),
        ),
        _ => Error::Api(format!(
            "{host} couldn't accept that: {message}. Check it and try again"
        )),
    }
}

fn id_at<'a>(data: &'a Value, mutation: &str, node: &str) -> Result<&'a str> {
    data[mutation][node]["id"].as_str().ok_or_else(|| {
        Error::Api(format!(
            "GitHub's answer to {mutation} was missing an id. Try again"
        ))
    })
}

struct Pending {
    review_id: String,
    existing: Vec<ExistingComment>,
}

async fn find_or_create_pending(client: &GithubClient, id: &ChangeId) -> Result<Pending> {
    let (owner, name) = repo_parts(id)?;
    let data = send_graphql(
        client,
        json!({
            "query": FIND_PENDING,
            "variables": { "owner": owner, "name": name, "number": id.number },
        }),
    )
    .await?;
    let data: FindData = serde_json::from_value(data)
        .map_err(|_| Error::Api("GitHub sent something unexpected. Try again".to_string()))?;
    let pr = data
        .repository
        .and_then(|r| r.pull_request)
        .ok_or_else(gone)?;
    if let Some(review) = pr.reviews.nodes.into_iter().next() {
        return Ok(Pending {
            review_id: review.id,
            existing: review.comments.nodes,
        });
    }
    let created = mutate(client, CREATE_REVIEW, json!({ "pullRequestId": pr.id })).await?;
    let review_id = id_at(&created, "addPullRequestReview", "pullRequestReview")?.to_string();
    Ok(Pending {
        review_id,
        existing: Vec::new(),
    })
}

fn already_there(existing: &[ExistingComment], call: &PlannedCall) -> bool {
    let PlannedCall::AddThread {
        path, line, body, ..
    } = call
    else {
        return false;
    };
    existing
        .iter()
        .any(|e| e.path == *path && e.line == Some(*line) && e.body == *body)
}

fn thread_input(review_id: &str, call: &PlannedCall) -> Option<Value> {
    let PlannedCall::AddThread {
        path,
        line,
        side,
        start_line,
        start_side,
        body,
    } = call
    else {
        return None;
    };
    let mut input = json!({
        "pullRequestReviewId": review_id,
        "path": path,
        "line": line,
        "side": side,
        "body": body,
    });
    if let (Some(start_line), Some(start_side)) = (start_line, start_side) {
        input["startLine"] = json!(start_line);
        input["startSide"] = json!(start_side);
    }
    Some(input)
}

fn left_pending(cause: &Error, added: usize, total: usize) -> Error {
    Error::Api(format!(
        "{cause}. Your review is saved as pending on GitHub ({added} of {total} comments added) and nothing was lost. Try again to pick up where it stopped, or open the pull request on GitHub to finish it there"
    ))
}

pub(crate) async fn submit_review(
    client: &GithubClient,
    id: &ChangeId,
    review: &ReviewDraft,
    verdict: Verdict,
) -> Result<()> {
    let plan = plan_review(review, verdict)?;
    let pending = find_or_create_pending(client, id).await?;
    let total = plan.comment_count();
    let mut added = 0;
    for call in &plan.calls {
        let Some(input) = thread_input(&pending.review_id, call) else {
            continue;
        };
        if !already_there(&pending.existing, call) {
            mutate(client, ADD_THREAD, input)
                .await
                .map_err(|e| left_pending(&e, added, total))?;
        }
        added += 1;
    }
    let mut input =
        json!({ "pullRequestReviewId": pending.review_id, "event": event_name(verdict) });
    if !review.body.trim().is_empty() {
        input["body"] = json!(review.body);
    }
    mutate(client, SUBMIT, input)
        .await
        .map_err(|e| left_pending(&e, added, total))?;
    Ok(())
}

#[derive(Deserialize)]
struct ReplyComment {
    id: String,
    author: Option<Author>,
    body: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    state: Option<String>,
}

#[derive(Deserialize)]
struct Author {
    login: String,
}

/// Replies on a thread. If you have a pending review, GitHub keeps the reply pending with it.
pub(crate) async fn reply(client: &GithubClient, thread: &ThreadId, body: &str) -> Result<Comment> {
    if body.trim().is_empty() {
        return Err(invalid("The reply is empty. Write something first"));
    }
    let data = mutate(
        client,
        REPLY,
        json!({ "pullRequestReviewThreadId": thread.as_str(), "body": body }),
    )
    .await?;
    let node = data["addPullRequestReviewThreadReply"]["comment"].clone();
    let c: ReplyComment = serde_json::from_value(node).map_err(|_| {
        Error::Api("GitHub's reply came back unreadable. Refresh to check it posted".to_string())
    })?;
    Ok(Comment {
        id: CommentId::new(c.id),
        author: c.author.map_or_else(String::new, |a| a.login),
        body: c.body,
        created_at: parse_rfc3339(&c.created_at).unwrap_or(Timestamp(0)),
        pending: c.state.as_deref() == Some("PENDING"),
    })
}

pub(crate) async fn resolve(
    client: &GithubClient,
    thread: &ThreadId,
    resolved: bool,
) -> Result<()> {
    let (query, field) = if resolved {
        (RESOLVE, "resolveReviewThread")
    } else {
        (UNRESOLVE, "unresolveReviewThread")
    };
    let data = mutate(client, query, json!({ "threadId": thread.as_str() })).await?;
    data[field]["thread"]["id"]
        .as_str()
        .map(|_| ())
        .ok_or_else(|| Error::Api("GitHub didn't confirm the change. Refresh to check".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(start: Option<u32>, line: u32, body: &str) -> DraftComment {
        DraftComment {
            path: "src/a.rs".into(),
            side: Side::New,
            start_line: start,
            line,
            body: body.into(),
        }
    }

    fn draft(body: &str, comments: Vec<DraftComment>) -> ReviewDraft {
        ReviewDraft {
            body: body.into(),
            comments,
        }
    }

    #[test]
    fn plan_lists_calls_in_order() {
        let d = draft("ok", vec![comment(None, 4, "a"), comment(Some(6), 9, "b")]);
        let plan = plan_review(&d, Verdict::Approve).unwrap();
        assert_eq!(plan.calls.len(), 4);
        assert_eq!(plan.calls[0], PlannedCall::OpenPendingReview);
        assert!(matches!(
            &plan.calls[2],
            PlannedCall::AddThread {
                start_line: Some(6),
                start_side: Some("RIGHT"),
                ..
            }
        ));
        assert!(matches!(
            &plan.calls[3],
            PlannedCall::Submit {
                event: "APPROVE",
                ..
            }
        ));
        assert_eq!(plan.summary(), "Approve with 2 comments");
    }

    #[test]
    fn single_line_range_collapses_and_old_side_is_left() {
        let mut c = comment(Some(4), 4, "a");
        c.side = Side::Old;
        let plan = plan_review(&draft("", vec![c]), Verdict::Comment).unwrap();
        assert!(matches!(
            &plan.calls[1],
            PlannedCall::AddThread {
                side: "LEFT",
                start_line: None,
                start_side: None,
                ..
            }
        ));
        assert_eq!(plan.summary(), "Post review with 1 comment");
    }

    #[test]
    fn validation_is_calm() {
        let e = plan_review(&draft("  ", vec![]), Verdict::RequestChanges).unwrap_err();
        assert!(e.to_string().contains("short summary"));
        assert!(plan_review(&draft("", vec![]), Verdict::Comment).is_err());
        assert!(plan_review(&draft("", vec![]), Verdict::Approve).is_ok());
        let bad = |c| plan_review(&draft("x", vec![c]), Verdict::Comment).is_err();
        assert!(bad(comment(None, 3, " ")));
        assert!(bad(comment(Some(9), 3, "b")));
        assert!(bad(comment(None, 0, "b")));
    }
}
