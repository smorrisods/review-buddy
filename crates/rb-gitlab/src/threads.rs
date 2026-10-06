//! Discussions and the viewer's draft notes. See "Threads and comments" in docs/integrations.md.
//!
//! Each discussion becomes a `Thread`: system notes are dropped, diff discussions are anchored by
//! their `position` (a `line_range` gives the range, with the end as `line`), and individual
//! notes without a position are unanchored conversation threads. Draft notes (the pending review)
//! come back as pending comments by the viewer: a reply draft joins its thread, any other draft is
//! a pending thread of its own.

use rb_core::{ChangeId, Comment, CommentId, Error, Result, Side, Thread, Timestamp};
use serde::Deserialize;

use crate::rest::{list, mr_base, thread_id};
use crate::time::parse_rfc3339;
use crate::GitlabClient;

const PER_PAGE: u32 = 100;
const MAX_PAGES: usize = 30;

#[derive(Deserialize, Default, Clone)]
pub(crate) struct Endpoint {
    #[serde(rename = "type")]
    kind: Option<String>,
    old_line: Option<u32>,
    new_line: Option<u32>,
}

#[derive(Deserialize, Default, Clone)]
pub(crate) struct LineRange {
    start: Option<Endpoint>,
    end: Option<Endpoint>,
}

#[derive(Deserialize, Default, Clone)]
pub(crate) struct Position {
    new_path: Option<String>,
    old_path: Option<String>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    line_range: Option<LineRange>,
}

#[derive(Deserialize)]
struct Author {
    #[serde(default)]
    username: String,
}

#[derive(Deserialize)]
pub(crate) struct Note {
    id: u64,
    #[serde(default)]
    pub(crate) body: String,
    author: Option<Author>,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    system: bool,
    #[serde(default)]
    resolvable: bool,
    #[serde(default)]
    resolved: bool,
    position: Option<Position>,
}

#[derive(Deserialize)]
struct Discussion {
    id: String,
    #[serde(default)]
    notes: Vec<Note>,
}

#[derive(Deserialize)]
struct Draft {
    id: u64,
    #[serde(default)]
    note: String,
    discussion_id: Option<String>,
    position: Option<Position>,
}

/// Where a position sits: path, last line, its side, and the range start if it spans lines.
struct Anchor {
    path: Option<String>,
    line: Option<u32>,
    side: Side,
    start_line: Option<u32>,
    start_side: Option<Side>,
}

fn endpoint(e: &Endpoint) -> Option<(u32, Side)> {
    match e.kind.as_deref() {
        Some("old") => e.old_line.map(|l| (l, Side::Old)),
        Some("new") => e.new_line.map(|l| (l, Side::New)),
        _ => e
            .new_line
            .map(|l| (l, Side::New))
            .or_else(|| e.old_line.map(|l| (l, Side::Old))),
    }
}

fn anchor(position: Option<&Position>) -> Anchor {
    let Some(p) = position else {
        return Anchor {
            path: None,
            line: None,
            side: Side::New,
            start_line: None,
            start_side: None,
        };
    };
    let path = p
        .new_path
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| p.old_path.clone().filter(|s| !s.is_empty()));
    let single = p
        .new_line
        .map(|l| (l, Side::New))
        .or_else(|| p.old_line.map(|l| (l, Side::Old)));
    let range = p
        .line_range
        .as_ref()
        .and_then(|r| Some((endpoint(r.start.as_ref()?)?, endpoint(r.end.as_ref()?)?)));
    let (end, start) = match range {
        Some((start, end)) if start != end => (Some(end), Some(start)),
        Some((_, end)) => (Some(end), None),
        None => (single, None),
    };
    Anchor {
        path,
        line: end.map(|e| e.0),
        side: end.map_or(Side::New, |e| e.1),
        start_line: start.map(|s| s.0),
        start_side: start.map(|s| s.1),
    }
}

pub(crate) fn comment(note: &Note) -> Comment {
    Comment {
        id: CommentId::new(note.id.to_string()),
        author: note
            .author
            .as_ref()
            .map_or_else(String::new, |a| a.username.clone()),
        body: note.body.clone(),
        created_at: parse_rfc3339(&note.created_at).unwrap_or(Timestamp(0)),
        pending: false,
    }
}

fn map_discussion(id: &ChangeId, d: &Discussion) -> Option<Thread> {
    let notes: Vec<&Note> = d.notes.iter().filter(|n| !n.system).collect();
    let first = notes.first()?;
    let a = anchor(
        notes
            .iter()
            .find_map(|n| n.position.as_ref())
            .or(first.position.as_ref()),
    );
    let resolvable: Vec<&&Note> = notes.iter().filter(|n| n.resolvable).collect();
    Some(Thread {
        id: thread_id(id, &d.id),
        path: a.path,
        line: a.line,
        side: a.side,
        start_line: a.start_line,
        start_side: a.start_side,
        resolved: !resolvable.is_empty() && resolvable.iter().all(|n| n.resolved),
        outdated: false,
        pending: false,
        comments: notes.into_iter().map(comment).collect(),
    })
}

fn draft_comment(d: &Draft, author: &str) -> Comment {
    Comment {
        id: CommentId::new(format!("draft:{}", d.id)),
        author: author.to_string(),
        body: d.note.clone(),
        created_at: Timestamp(0),
        pending: true,
    }
}

fn merge_drafts(id: &ChangeId, threads: &mut Vec<Thread>, drafts: &[Draft], author: &str) {
    for d in drafts {
        let comment = draft_comment(d, author);
        let discussion = d.discussion_id.as_deref().filter(|s| !s.is_empty());
        if let Some(thread) =
            discussion.and_then(|disc| threads.iter_mut().find(|t| t.id == thread_id(id, disc)))
        {
            thread.comments.push(comment);
            continue;
        }
        let a = anchor(d.position.as_ref());
        threads.push(Thread {
            id: thread_id(id, &format!("draft:{}", d.id)),
            path: a.path,
            line: a.line,
            side: a.side,
            start_line: a.start_line,
            start_side: a.start_side,
            resolved: false,
            outdated: false,
            pending: true,
            comments: vec![comment],
        });
    }
}

/// The viewer's draft notes. Before GitLab 15.9 there are none, and the endpoint answers 404.
async fn drafts(client: &GitlabClient, base: &str) -> Result<Vec<Draft>> {
    match list::<Draft>(client, &format!("{base}/draft_notes"), PER_PAGE, MAX_PAGES).await {
        Ok((drafts, _)) => Ok(drafts),
        Err(Error::NotFound(_)) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

pub(crate) async fn threads(client: &GitlabClient, id: &ChangeId) -> Result<Vec<Thread>> {
    let base = mr_base(id);
    let (discussions, truncated) =
        list::<Discussion>(client, &format!("{base}/discussions"), PER_PAGE, MAX_PAGES).await?;
    if truncated {
        return Err(Error::Api(format!(
            "{} has more discussions than Review Buddy will load. Open it in the browser to see the rest",
            id.short_ref()
        )));
    }
    let mut out: Vec<Thread> = discussions
        .iter()
        .filter_map(|d| map_discussion(id, d))
        .collect();
    let drafts = drafts(client, &base).await?;
    if !drafts.is_empty() {
        let me = client
            .whoami()
            .await
            .map_or_else(|_| "you".into(), |u| u.login);
        merge_drafts(id, &mut out, &drafts, &me);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::{ForgeKind, SourceId};
    use serde_json::json;

    fn cid() -> ChangeId {
        ChangeId {
            source_id: SourceId::new("lab"),
            kind: ForgeKind::GitLab,
            repo: "g/p".into(),
            number: 3,
        }
    }

    fn disc(v: serde_json::Value) -> Discussion {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn single_line_and_range_anchors() {
        let t = map_discussion(
            &cid(),
            &disc(json!({"id": "d1", "notes": [{
                "id": 1, "body": "hm", "author": {"username": "ann"},
                "created_at": "2026-01-01T00:00:00.000Z", "resolvable": true, "resolved": true,
                "position": {"new_path": "a.rs", "old_path": "a.rs", "new_line": 9, "old_line": null,
                    "line_range": {"start": {"type": "new", "new_line": 7, "old_line": null},
                                   "end": {"type": "new", "new_line": 9, "old_line": null}}}
            }]})),
        )
        .unwrap();
        assert_eq!(
            (t.line, t.start_line, t.side),
            (Some(9), Some(7), Side::New)
        );
        assert_eq!(t.start_side, Some(Side::New));
        assert!(t.resolved && !t.pending);
        assert_eq!(t.id.as_str(), "g/p!3!d1");

        let t = map_discussion(
            &cid(),
            &disc(json!({"id": "d2", "notes": [{
                "id": 2, "body": "gone", "position": {"new_path": "a.rs", "old_path": "a.rs", "old_line": 4}
            }]})),
        )
        .unwrap();
        assert_eq!((t.line, t.side, t.start_line), (Some(4), Side::Old, None));
        assert!(!t.resolved);
    }

    #[test]
    fn system_notes_skipped_and_plain_notes_unanchored() {
        let only_system =
            disc(json!({"id": "s", "notes": [{"id": 1, "system": true, "body": "merged"}]}));
        assert!(map_discussion(&cid(), &only_system).is_none());
        let t = map_discussion(
            &cid(),
            &disc(json!({"id": "c", "individual_note": true, "notes": [
                {"id": 1, "system": true, "body": "x"},
                {"id": 2, "body": "hello", "author": {"username": "bo"}}
            ]})),
        )
        .unwrap();
        assert_eq!((t.path, t.line, t.comments.len()), (None, None, 1));
    }

    #[test]
    fn drafts_join_threads_or_stand_alone() {
        let mut threads = vec![map_discussion(
            &cid(),
            &disc(json!({"id": "d1", "notes": [{"id": 1, "body": "q", "position": {"new_path": "a.rs", "new_line": 2}}]})),
        )
        .unwrap()];
        let drafts: Vec<Draft> = serde_json::from_value(json!([
            {"id": 8, "note": "reply", "discussion_id": "d1", "position": null},
            {"id": 9, "note": "new", "discussion_id": null,
             "position": {"new_path": "b.rs", "new_line": 5}}
        ]))
        .unwrap();
        merge_drafts(&cid(), &mut threads, &drafts, "me");
        assert_eq!(threads.len(), 2);
        assert!(threads[0].comments[1].pending && !threads[0].pending);
        assert!(threads[1].pending);
        assert_eq!(threads[1].path.as_deref(), Some("b.rs"));
        assert_eq!(threads[1].comments[0].author, "me");
    }
}
