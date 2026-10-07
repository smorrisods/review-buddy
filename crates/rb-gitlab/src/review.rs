//! Review writes over REST: draft notes as the pending review, publish, approve, reply and
//! resolve. See "Threads and comments" and "Submitting a review" in docs/integrations.md.
//!
//! A review is submitted as: read your threads (which include your draft notes), create a draft
//! note for each comment that isn't already there, publish all drafts with `bulk_publish`, then
//! for an approval call `approve` with the head SHA. GitLab has no review summary, so a review
//! body becomes one more draft note without a position. A failure after drafts exist leaves them
//! pending on GitLab and says so; nothing is ever deleted.

use rb_core::{
    ChangeId, Comment, DraftComment, Error, FilePatch, Result, ReviewDraft, Side, Thread, ThreadId,
    Verdict,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::rest::{mr_base, split_thread_id};
use crate::threads::{comment, threads, Note};
use crate::{files, GitlabClient};

/// One GitLab call a review submission makes, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedCall {
    /// A draft note on a line (or range), skipped when you already have the same one.
    CreateDraft {
        path: String,
        line: u32,
        side: &'static str,
        start_line: Option<u32>,
        body: String,
    },
    /// The review summary, as a draft note without a position.
    CreateSummary { body: String },
    /// `bulk_publish`: posts every draft note you have on the change. `reviewer_state` is set
    /// when requesting changes.
    Publish {
        reviewer_state: Option<&'static str>,
    },
    /// `approve`, guarded by the head SHA.
    Approve,
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
            .filter(|c| matches!(c, PlannedCall::CreateDraft { .. }))
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

fn invalid(what: &str) -> Error {
    Error::Api(format!("{what}. Nothing was sent"))
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Old => "old",
        Side::New => "new",
    }
}

/// Checks a draft and lists the calls that would submit it.
pub fn plan_review(review: &ReviewDraft, verdict: Verdict) -> Result<ReviewPlan> {
    let body = review.body.trim();
    if verdict == Verdict::RequestChanges && body.is_empty() {
        return Err(invalid(
            "Requesting changes needs a summary. Say what should change first",
        ));
    }
    if verdict == Verdict::Comment && body.is_empty() && review.comments.is_empty() {
        return Err(invalid("There's nothing to post yet. Add a comment first"));
    }
    let mut calls = Vec::new();
    for c in &review.comments {
        calls.push(plan_draft(c)?);
    }
    if !body.is_empty() {
        calls.push(PlannedCall::CreateSummary {
            body: review.body.clone(),
        });
    }
    if !calls.is_empty() {
        calls.push(PlannedCall::Publish {
            reviewer_state: (verdict == Verdict::RequestChanges).then_some("requested_changes"),
        });
    }
    if verdict == Verdict::Approve {
        calls.push(PlannedCall::Approve);
    }
    Ok(ReviewPlan { verdict, calls })
}

fn plan_draft(c: &DraftComment) -> Result<PlannedCall> {
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
    Ok(PlannedCall::CreateDraft {
        path: c.path.clone(),
        line: c.line,
        side: side_name(c.side),
        start_line: c.start_line.filter(|s| *s != c.line),
        body: c.body.clone(),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Add,
    Del,
    Context,
}

/// One diff line with GitLab's line counters: `old` and `new` are the numbers the line has, or
/// would have next to it, on each side (the same pair GitLab puts in a `line_code`).
#[derive(Clone, Copy)]
struct Row {
    kind: Kind,
    old: u32,
    new: u32,
}

fn hunk_start(header: &str) -> Option<(u32, u32)> {
    let rest = header.strip_prefix("@@ -")?;
    let number = |s: &str| -> Option<u32> {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        s[..end].parse().ok()
    };
    let old = number(rest)?;
    let new = number(&rest[rest.find(" +")? + 2..])?;
    Some((old, new))
}

fn rows(patch: &str) -> Vec<Row> {
    let (mut old, mut new) = (0, 0);
    let mut out = Vec::new();
    for line in patch.lines() {
        if let Some(start) = hunk_start(line) {
            (old, new) = start;
            continue;
        }
        let kind = match line.as_bytes().first() {
            Some(b'+') => Kind::Add,
            Some(b'-') => Kind::Del,
            Some(b'\\') => continue,
            _ => Kind::Context,
        };
        out.push(Row { kind, old, new });
        if kind != Kind::Add {
            old += 1;
        }
        if kind != Kind::Del {
            new += 1;
        }
    }
    out
}

fn find_row(rows: &[Row], line: u32, side: Side) -> Option<Row> {
    rows.iter().copied().find(|r| match side {
        Side::New => r.kind != Kind::Del && r.new == line,
        Side::Old => r.kind != Kind::Add && r.old == line,
    })
}

fn line_code(path: &str, row: Row) -> String {
    use sha1::{Digest, Sha1};
    let hash: String = Sha1::digest(path.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{hash}_{}_{}", row.old, row.new)
}

struct Refs {
    base_sha: String,
    start_sha: String,
    head_sha: String,
}

fn endpoint_json(path: &str, row: Row) -> Value {
    let mut v = json!({ "line_code": line_code(path, row) });
    match row.kind {
        Kind::Add => {
            v["type"] = json!("new");
            v["new_line"] = json!(row.new);
        }
        Kind::Del => {
            v["type"] = json!("old");
            v["old_line"] = json!(row.old);
        }
        Kind::Context => {
            v["old_line"] = json!(row.old);
            v["new_line"] = json!(row.new);
        }
    }
    v
}

fn set_lines(position: &mut Value, row: Row) {
    if row.kind != Kind::Add {
        position["old_line"] = json!(row.old);
    }
    if row.kind != Kind::Del {
        position["new_line"] = json!(row.new);
    }
}

/// The `position` for a draft note. Unchanged lines carry both line numbers, as GitLab needs.
/// A line the diff doesn't show is sent as asked and GitLab decides.
fn position(refs: &Refs, file: Option<&FilePatch>, c: &DraftComment) -> Value {
    let new_path = c.path.clone();
    let old_path = file
        .and_then(|f| f.old_path.clone())
        .unwrap_or_else(|| c.path.clone());
    let mut p = json!({
        "position_type": "text",
        "base_sha": refs.base_sha,
        "start_sha": refs.start_sha,
        "head_sha": refs.head_sha,
        "new_path": new_path,
        "old_path": old_path,
    });
    let table = file
        .and_then(|f| f.patch.as_deref())
        .map(rows)
        .unwrap_or_default();
    let end = find_row(&table, c.line, c.side);
    match end {
        Some(row) => set_lines(&mut p, row),
        None => match c.side {
            Side::New => p["new_line"] = json!(c.line),
            Side::Old => p["old_line"] = json!(c.line),
        },
    }
    let start_line = c.start_line.filter(|s| *s != c.line);
    if let (Some(start), Some(end)) = (start_line.and_then(|s| find_row(&table, s, c.side)), end) {
        p["line_range"] = json!({
            "start": endpoint_json(&c.path, start),
            "end": endpoint_json(&c.path, end),
        });
    }
    p
}

fn already_there(existing: &[Thread], me: &str, call: &PlannedCall) -> bool {
    let mine = |t: &Thread, body: &str| {
        t.comments
            .iter()
            .any(|c| c.author == me && c.body.trim() == body.trim())
    };
    match call {
        PlannedCall::CreateDraft {
            path,
            line,
            side,
            body,
            ..
        } => existing.iter().any(|t| {
            t.path.as_deref() == Some(path)
                && t.line == Some(*line)
                && side_name(t.side) == *side
                && mine(t, body)
        }),
        PlannedCall::CreateSummary { body } => {
            existing.iter().any(|t| t.path.is_none() && mine(t, body))
        }
        _ => false,
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
        reason: "your token can read but not review here. Create one with the `api` scope and sign in again".to_string(),
    }
}

fn gone() -> Error {
    Error::NotFound(
        "that merge request or project is gone, or your token can't see it. Check the address and your access"
            .to_string(),
    )
}

/// Rewrites generic read-oriented errors with copy that fits a write.
fn write_error(e: Error) -> Error {
    match e {
        Error::Forbidden { host, reason } if !reason.contains("missing a scope") => no_review_access(&host),
        Error::NotFound(_) => gone(),
        Error::Conflict(msg) if msg.to_ascii_lowercase().contains("sha") => Error::Conflict(
            "the merge request has new commits since you opened it. Refresh, look over what changed, and try again"
                .to_string(),
        ),
        Error::Api(msg) if msg.contains("answered 422") || msg.contains("answered 400") => {
            unprocessable(&msg)
        }
        other => other,
    }
}

fn unprocessable(msg: &str) -> Error {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("line") || lower.contains("diff") || lower.contains("position") {
        line_moved()
    } else {
        let detail = msg.split_once(": ").map_or(msg, |(_, d)| d);
        Error::Api(format!(
            "GitLab couldn't accept that ({detail}). Check the text and try again"
        ))
    }
}

fn left_pending(cause: &Error, added: usize, total: usize) -> Error {
    let cause = match cause {
        Error::RateLimited {
            host,
            retry_after_secs: Some(secs),
        } => format!("rate limited by {host}, try again in {secs} seconds"),
        other => other.to_string(),
    };
    Error::Api(format!(
        "{cause}. Your review is saved as pending on GitLab ({added} of {total} comments added) and nothing was lost. Try again to pick up where it stopped, or open the merge request on GitLab to finish it there"
    ))
}

fn published_not_approved(cause: &Error) -> Error {
    Error::Api(format!(
        "{cause}. Your comments were already posted, but the approval didn't go through. Approve again from the merge request once it's sorted out"
    ))
}

#[derive(Deserialize)]
struct MrRefs {
    sha: Option<String>,
    diff_refs: Option<DiffRefs>,
}

#[derive(Deserialize)]
struct DiffRefs {
    base_sha: Option<String>,
    start_sha: Option<String>,
    head_sha: Option<String>,
}

async fn mr_refs(client: &GitlabClient, base: &str) -> Result<Refs> {
    let mr: MrRefs = client.get_json(base).await.map_err(write_error)?;
    let r = mr.diff_refs;
    let head = r
        .as_ref()
        .and_then(|r| r.head_sha.clone())
        .or(mr.sha)
        .ok_or_else(|| Error::Api("GitLab didn't say which commit this is. Try again".into()))?;
    let base_sha = r
        .as_ref()
        .and_then(|r| r.base_sha.clone())
        .unwrap_or_default();
    let start_sha = r
        .and_then(|r| r.start_sha)
        .unwrap_or_else(|| base_sha.clone());
    Ok(Refs {
        base_sha,
        start_sha,
        head_sha: head,
    })
}

#[derive(Deserialize)]
struct Approvals {
    #[serde(default)]
    approved_by: Vec<Approver>,
}

#[derive(Deserialize)]
struct Approver {
    user: ApproverUser,
}

#[derive(Deserialize)]
struct ApproverUser {
    #[serde(default)]
    username: String,
}

async fn approve(client: &GitlabClient, base: &str, me: &str, head_sha: &str) -> Result<()> {
    let state: Approvals = client
        .get_json(&format!("{base}/approvals"))
        .await
        .map_err(write_error)?;
    if state.approved_by.iter().any(|a| a.user.username == me) {
        return Ok(());
    }
    client
        .write_json(
            Method::POST,
            &format!("{base}/approve"),
            &json!({ "sha": head_sha }),
        )
        .await
        .map(|_| ())
        .map_err(|e| match e {
            Error::Unauthorized { host } => Error::Forbidden {
                host,
                reason: "GitLab won't let this account approve here. Authors can't always approve their own work, and some projects limit who may. Leave a comment instead".to_string(),
            },
            other => write_error(other),
        })
}

fn is_pending(t: &Thread) -> bool {
    t.pending || t.comments.iter().any(|c| c.pending)
}

pub(crate) async fn submit_review(
    client: &GitlabClient,
    id: &ChangeId,
    review: &ReviewDraft,
    verdict: Verdict,
) -> Result<()> {
    let plan = plan_review(review, verdict)?;
    let base = mr_base(id);
    let refs = mr_refs(client, &base).await?;
    let me = client.whoami().await.map_err(write_error)?.login;
    let existing = threads(client, id).await.map_err(write_error)?;
    let total = plan.comment_count();

    let needs_files = !review.comments.is_empty();
    let patches = if needs_files {
        files::files(client, id).await.map_err(write_error)?
    } else {
        Vec::new()
    };

    let mut added = 0;
    let mut created = 0;
    let mut comments = review.comments.iter();
    for call in &plan.calls {
        let body = match call {
            PlannedCall::CreateDraft { .. } => {
                let c = comments.next().expect("one draft call per comment");
                if already_there(&existing, &me, call) {
                    added += 1;
                    continue;
                }
                let file = patches.iter().find(|f| f.path == c.path);
                json!({ "note": c.body, "position": position(&refs, file, c) })
            }
            PlannedCall::CreateSummary { body } => {
                if already_there(&existing, &me, call) {
                    continue;
                }
                json!({ "note": body })
            }
            _ => continue,
        };
        client
            .write_json(Method::POST, &format!("{base}/draft_notes"), &body)
            .await
            .map_err(write_error)
            .map_err(|e| left_pending(&e, added, total))?;
        created += 1;
        if matches!(call, PlannedCall::CreateDraft { .. }) {
            added += 1;
        }
    }

    let reviewer_state = (verdict == Verdict::RequestChanges).then_some("requested_changes");
    if created > 0 || reviewer_state.is_some() || existing.iter().any(is_pending) {
        let publish = match reviewer_state {
            Some(state) => json!({ "reviewer_state": state }),
            None => json!({}),
        };
        client
            .write_json(
                Method::POST,
                &format!("{base}/draft_notes/bulk_publish"),
                &publish,
            )
            .await
            .map_err(write_error)
            .map_err(|e| left_pending(&e, added, total))?;
    }
    if verdict == Verdict::Approve {
        approve(client, &base, &me, &refs.head_sha)
            .await
            .map_err(|e| published_not_approved(&e))?;
    }
    Ok(())
}

/// Replies on a discussion. A reply on a thread you're drafting waits for the review to publish.
pub(crate) async fn reply(client: &GitlabClient, thread: &ThreadId, body: &str) -> Result<Comment> {
    if body.trim().is_empty() {
        return Err(invalid("The reply is empty. Write something first"));
    }
    let (base, discussion) = split_thread_id(thread)?;
    if discussion.starts_with("draft:") {
        return Err(Error::Conflict(
            "that comment is still pending in your review. Submit the review first, then reply"
                .to_string(),
        ));
    }
    let raw = client
        .write_json(
            Method::POST,
            &format!("{base}/discussions/{discussion}/notes"),
            &json!({ "body": body }),
        )
        .await
        .map_err(write_error)?;
    let note: Note = client.parse(&raw).map_err(|_| {
        Error::Api("GitLab's reply came back unreadable. Refresh to check it posted".to_string())
    })?;
    Ok(comment(&note))
}

/// The draft note a pending thread stands for, as `(merge request path, note id)`.
fn draft_note(thread: &ThreadId) -> Result<(String, String)> {
    let (base, discussion) = split_thread_id(thread)?;
    match discussion.strip_prefix("draft:") {
        Some(note) if !note.is_empty() => Ok((base, note.to_string())),
        _ => Err(Error::Conflict(
            "that comment is already published, so it can't be edited here. Open it on GitLab"
                .to_string(),
        )),
    }
}

/// Rewrites one of your draft notes.
pub(crate) async fn update_draft(
    client: &GitlabClient,
    thread: &ThreadId,
    body: &str,
) -> Result<()> {
    if body.trim().is_empty() {
        return Err(invalid("The comment is empty. Write something first"));
    }
    let (base, note) = draft_note(thread)?;
    client
        .write_json(
            Method::PUT,
            &format!("{base}/draft_notes/{note}"),
            &json!({ "note": body }),
        )
        .await
        .map(|_| ())
        .map_err(write_error)
}

/// Removes one of your draft notes.
pub(crate) async fn delete_draft(client: &GitlabClient, thread: &ThreadId) -> Result<()> {
    let (base, note) = draft_note(thread)?;
    client
        .write_json(
            Method::DELETE,
            &format!("{base}/draft_notes/{note}"),
            &json!({}),
        )
        .await
        .map(|_| ())
        .map_err(write_error)
}

pub(crate) async fn resolve(
    client: &GitlabClient,
    thread: &ThreadId,
    resolved: bool,
) -> Result<()> {
    let (base, discussion) = split_thread_id(thread)?;
    if discussion.starts_with("draft:") {
        return Err(Error::Conflict(
            "that comment is still pending in your review. Submit the review first".to_string(),
        ));
    }
    client
        .write_json(
            Method::PUT,
            &format!("{base}/discussions/{discussion}"),
            &json!({ "resolved": resolved }),
        )
        .await
        .map(|_| ())
        .map_err(write_error)
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
        assert!(matches!(
            plan.calls.as_slice(),
            [
                PlannedCall::CreateDraft {
                    start_line: None,
                    ..
                },
                PlannedCall::CreateDraft {
                    start_line: Some(6),
                    ..
                },
                PlannedCall::CreateSummary { .. },
                PlannedCall::Publish {
                    reviewer_state: None
                },
                PlannedCall::Approve
            ]
        ));
        assert_eq!(plan.summary(), "Approve with 2 comments");
        let plan = plan_review(&draft("", vec![]), Verdict::Approve).unwrap();
        assert_eq!(plan.calls, vec![PlannedCall::Approve]);
        assert_eq!(plan.summary(), "Approve");
    }

    #[test]
    fn validation_is_calm() {
        assert!(plan_review(&draft(" ", vec![]), Verdict::RequestChanges).is_err());
        let plan = plan_review(&draft("fix this", vec![]), Verdict::RequestChanges).unwrap();
        assert_eq!(
            plan.calls.last(),
            Some(&PlannedCall::Publish {
                reviewer_state: Some("requested_changes")
            })
        );
        assert_eq!(plan.summary(), "Request changes");
        assert!(plan_review(&draft("", vec![]), Verdict::Comment).is_err());
        let bad = |c| plan_review(&draft("x", vec![c]), Verdict::Comment).is_err();
        assert!(bad(comment(None, 3, " ")));
        assert!(bad(comment(Some(9), 3, "b")));
        assert!(bad(comment(None, 0, "b")));
        let plan = plan_review(&draft("", vec![comment(None, 3, "a")]), Verdict::Comment).unwrap();
        assert_eq!(plan.summary(), "Post review with 1 comment");
    }

    const PATCH: &str =
        "@@ -10,4 +10,5 @@\n ctx\n-gone\n+new1\n+new2\n tail\n\\ No newline at end of file\n";

    #[test]
    fn counters_follow_gitlab_line_codes() {
        let t = rows(PATCH);
        let at = |line, side| find_row(&t, line, side).map(|r| (r.old, r.new));
        assert_eq!(at(10, Side::New), Some((10, 10)));
        assert_eq!(at(11, Side::Old), Some((11, 11)));
        assert_eq!(at(11, Side::New), Some((12, 11)));
        assert_eq!(at(12, Side::New), Some((12, 12)));
        assert_eq!(at(13, Side::New), Some((12, 13)));
        assert_eq!(at(99, Side::New), None);
    }

    #[test]
    fn positions_carry_both_lines_for_context_and_a_range() {
        let refs = Refs {
            base_sha: "b".into(),
            start_sha: "s".into(),
            head_sha: "h".into(),
        };
        let file = FilePatch {
            path: "src/a.rs".into(),
            old_path: Some("src/old.rs".into()),
            status: rb_core::FileStatus::Renamed,
            adds: 2,
            dels: 1,
            patch: Some(PATCH.into()),
        };
        let p = position(&refs, Some(&file), &comment(None, 10, "x"));
        assert_eq!(p["new_line"], 10);
        assert_eq!(p["old_line"], 10);
        assert_eq!(p["old_path"], "src/old.rs");
        let p = position(&refs, Some(&file), &comment(None, 12, "x"));
        assert_eq!(p["new_line"], 12);
        assert!(p.get("old_line").is_none());
        let p = position(&refs, Some(&file), &comment(Some(11), 12, "x"));
        assert_eq!(p["line_range"]["start"]["type"], "new");
        assert_eq!(p["line_range"]["end"]["new_line"], 12);
    }
}
