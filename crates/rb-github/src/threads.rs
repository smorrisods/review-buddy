//! Review threads over GraphQL plus issue comments over REST.
//! See "Threads and comments" in docs/integrations.md.
//!
//! `reviewThreads` already includes the viewer's pending review, so a draft comment (and
//! any `suggestion` block in it) survives a refresh. `rb_core::Thread` carries one line and
//! side, so a range comment is anchored at its last line and `startLine` isn't kept. An
//! outdated thread falls back to `originalLine` so it keeps an anchor.

use rb_core::{ChangeId, Comment, CommentId, Error, Result, Side, Thread, ThreadId, Timestamp};
use serde::Deserialize;
use serde_json::json;

use crate::files::repo_parts;
use crate::graphql;
use crate::time::parse_rfc3339;
use crate::GithubClient;

const MAX_PAGES: usize = 20;
const ISSUE_COMMENT_PAGES: usize = 20;

const COMMENT_FIELDS: &str = "id author { login } body createdAt state";

fn threads_query() -> String {
    format!(
        r"
query($owner: String!, $name: String!, $number: Int!, $after: String) {{
  rateLimit {{ cost remaining resetAt }}
  repository(owner: $owner, name: $name) {{
    pullRequest(number: $number) {{
      reviewThreads(first: 50, after: $after) {{
        pageInfo {{ hasNextPage endCursor }}
        nodes {{
          id isResolved isOutdated path line originalLine startLine diffSide startDiffSide
          comments(first: 50) {{
            pageInfo {{ hasNextPage endCursor }}
            nodes {{ {COMMENT_FIELDS} }}
          }}
        }}
      }}
    }}
  }}
}}
"
    )
}

fn more_comments_query() -> String {
    format!(
        r"
query($id: ID!, $after: String) {{
  rateLimit {{ cost remaining resetAt }}
  node(id: $id) {{
    ... on PullRequestReviewThread {{
      comments(first: 100, after: $after) {{
        pageInfo {{ hasNextPage endCursor }}
        nodes {{ {COMMENT_FIELDS} }}
      }}
    }}
  }}
}}
"
    )
}

#[derive(Deserialize)]
struct ThreadsData {
    repository: Option<RepoNode>,
}

#[derive(Deserialize)]
struct RepoNode {
    #[serde(rename = "pullRequest")]
    pull_request: Option<PrNode>,
}

#[derive(Deserialize)]
struct PrNode {
    #[serde(rename = "reviewThreads")]
    review_threads: Connection<ThreadNode>,
}

#[derive(Deserialize)]
struct Connection<T> {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<T>,
}

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct ThreadNode {
    id: String,
    #[serde(rename = "isResolved")]
    is_resolved: bool,
    #[serde(rename = "isOutdated")]
    is_outdated: bool,
    path: Option<String>,
    line: Option<u32>,
    #[serde(rename = "originalLine")]
    original_line: Option<u32>,
    #[serde(rename = "startLine")]
    start_line: Option<u32>,
    #[serde(rename = "diffSide")]
    diff_side: Option<String>,
    #[serde(rename = "startDiffSide")]
    start_diff_side: Option<String>,
    comments: Connection<CommentNode>,
}

#[derive(Deserialize)]
struct CommentNode {
    id: String,
    author: Option<Login>,
    body: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    state: Option<String>,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct MoreData {
    node: Option<MoreNode>,
}

#[derive(Deserialize)]
struct MoreNode {
    comments: Option<Connection<CommentNode>>,
}

#[derive(Deserialize)]
struct IssueComment {
    id: u64,
    user: Option<Login>,
    #[serde(default)]
    body: String,
    created_at: String,
}

fn comment(node: CommentNode) -> Comment {
    Comment {
        id: CommentId::new(node.id),
        author: node.author.map_or_else(|| "ghost".into(), |a| a.login),
        body: node.body,
        created_at: parse_rfc3339(&node.created_at).unwrap_or(Timestamp(0)),
        pending: node.state.as_deref() == Some("PENDING"),
    }
}

fn side(raw: Option<&str>) -> Side {
    match raw {
        Some("LEFT") => Side::Old,
        _ => Side::New,
    }
}

fn thread(node: ThreadNode, comments: Vec<Comment>) -> Thread {
    let start_line = node.start_line.filter(|s| Some(*s) != node.line);
    Thread {
        pending: !comments.is_empty() && comments.iter().all(|c| c.pending),
        start_side: start_line.and(node.start_diff_side.as_deref().map(|s| side(Some(s)))),
        start_line,
        id: ThreadId::new(node.id),
        path: node.path,
        line: node.line.or(node.original_line),
        side: side(node.diff_side.as_deref()),
        resolved: node.is_resolved,
        outdated: node.is_outdated,
        comments,
    }
}

fn not_found(client: &GithubClient, id: &ChangeId) -> Error {
    Error::NotFound(format!(
        "{} (on {}, or the token can't see it)",
        id.short_ref(),
        client.host()
    ))
}

async fn rest_of_comments(
    client: &GithubClient,
    thread_id: &str,
    mut page: PageInfo,
    out: &mut Vec<Comment>,
) -> Result<()> {
    let query = more_comments_query();
    for _ in 0..MAX_PAGES {
        if !page.has_next_page {
            break;
        }
        let vars = json!({"id": thread_id, "after": page.end_cursor});
        let data = graphql::query::<MoreData>(client, &query, vars).await?.data;
        let Some(conn) = data.node.and_then(|n| n.comments) else {
            break;
        };
        out.extend(conn.nodes.into_iter().map(comment));
        page = conn.page_info;
    }
    Ok(())
}

async fn review_threads(client: &GithubClient, id: &ChangeId) -> Result<Vec<Thread>> {
    let (owner, name) = repo_parts(id)?;
    let query = threads_query();
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let vars = json!({"owner": owner, "name": name, "number": id.number, "after": after});
        let data = graphql::query::<ThreadsData>(client, &query, vars)
            .await?
            .data;
        let Some(pr) = data.repository.and_then(|r| r.pull_request) else {
            return Err(not_found(client, id));
        };
        let conn = pr.review_threads;
        for mut node in conn.nodes {
            let comments = std::mem::replace(
                &mut node.comments,
                Connection {
                    page_info: PageInfo {
                        has_next_page: false,
                        end_cursor: None,
                    },
                    nodes: Vec::new(),
                },
            );
            let mut list: Vec<Comment> = comments.nodes.into_iter().map(comment).collect();
            rest_of_comments(client, &node.id, comments.page_info, &mut list).await?;
            out.push(thread(node, list));
        }
        if !conn.page_info.has_next_page {
            break;
        }
        after = conn.page_info.end_cursor;
    }
    Ok(out)
}

async fn issue_comments(client: &GithubClient, id: &ChangeId) -> Result<Vec<Thread>> {
    let (owner, name) = repo_parts(id)?;
    let path = format!(
        "/repos/{owner}/{name}/issues/{}/comments?per_page=100",
        id.number
    );
    let (pages, _) = client.get_pages(&path, ISSUE_COMMENT_PAGES).await?;
    let mut out = Vec::new();
    for raw in pages {
        for c in client.parse::<Vec<IssueComment>>(&raw.body)? {
            out.push(Thread {
                id: ThreadId::new(format!("issue-comment:{}", c.id)),
                path: None,
                line: None,
                side: Side::New,
                start_line: None,
                start_side: None,
                pending: false,
                resolved: false,
                outdated: false,
                comments: vec![Comment {
                    id: CommentId::new(c.id.to_string()),
                    author: c.user.map_or_else(|| "ghost".into(), |u| u.login),
                    body: c.body,
                    created_at: parse_rfc3339(&c.created_at).unwrap_or(Timestamp(0)),
                    pending: false,
                }],
            });
        }
    }
    Ok(out)
}

/// Review threads first, then conversation comments, each in the order GitHub returns them.
pub(crate) async fn threads(client: &GithubClient, id: &ChangeId) -> Result<Vec<Thread>> {
    let mut all = review_threads(client, id).await?;
    all.extend(issue_comments(client, id).await?);
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(side: Option<&str>, line: Option<u32>, original: Option<u32>) -> ThreadNode {
        ThreadNode {
            id: "T".into(),
            is_resolved: true,
            is_outdated: line.is_none(),
            path: Some("a.rs".into()),
            line,
            original_line: original,
            start_line: None,
            diff_side: side.map(String::from),
            start_diff_side: None,
            comments: Connection {
                page_info: PageInfo {
                    has_next_page: false,
                    end_cursor: None,
                },
                nodes: vec![],
            },
        }
    }

    #[test]
    fn anchors_keep_side_and_fall_back_to_original_line() {
        let t = thread(node(Some("LEFT"), Some(4), Some(4)), vec![]);
        assert_eq!((t.side, t.line), (Side::Old, Some(4)));
        let t = thread(node(Some("RIGHT"), None, Some(9)), vec![]);
        assert_eq!(
            (t.side, t.line, t.outdated, t.resolved),
            (Side::New, Some(9), true, true)
        );
    }

    #[test]
    fn range_threads_keep_start_line_and_side() {
        let mut n = node(Some("LEFT"), Some(4), Some(4));
        n.start_line = Some(2);
        n.start_diff_side = Some("LEFT".into());
        let t = thread(n, vec![]);
        assert_eq!((t.start_line, t.start_side), (Some(2), Some(Side::Old)));
        let mut n = node(Some("RIGHT"), Some(4), Some(4));
        n.start_line = Some(4);
        assert_eq!(thread(n, vec![]).start_line, None);
    }
}
