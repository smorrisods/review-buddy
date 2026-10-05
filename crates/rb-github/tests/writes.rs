use rb_core::{
    ChangeId, DraftComment, Error, ForgeKind, Provider, ReviewDraft, Side, SourceId, ThreadId,
    Verdict,
};
use rb_github::{GithubClient, GithubProvider};
use rb_platform::Secret;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

const TOKEN: &str = "ghp_supersecret";

fn provider(server: &MockServer) -> GithubProvider {
    let client = GithubClient::new("ghe.test", Some(&server.uri()), Secret::new(TOKEN)).unwrap();
    GithubProvider::new(client)
}

fn change() -> ChangeId {
    ChangeId {
        source_id: SourceId::new("work"),
        kind: ForgeKind::GitHub,
        repo: "acme/widgets".into(),
        number: 7,
    }
}

/// Matches a GraphQL call whose query contains `needle`.
struct Op(&'static str);

impl Match for Op {
    fn matches(&self, r: &Request) -> bool {
        serde_json::from_slice::<Value>(&r.body)
            .is_ok_and(|b| b["query"].as_str().is_some_and(|q| q.contains(self.0)))
    }
}

fn gql(data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "data": data }))
}

fn gql_err(kind: &str, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "data": null,
        "errors": [{ "type": kind, "message": message }]
    }))
}

async fn mount(server: &MockServer, op: &'static str, resp: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Op(op))
        .respond_with(resp)
        .mount(server)
        .await;
}

fn no_pending() -> ResponseTemplate {
    gql(json!({"repository": {"pullRequest": {"id": "PR_1", "reviews": {"nodes": []}}}}))
}

fn pending_with(comments: Value) -> ResponseTemplate {
    gql(
        json!({"repository": {"pullRequest": {"id": "PR_1", "reviews": {"nodes": [
            {"id": "PRR_old", "comments": {"nodes": comments}}
        ]}}}}),
    )
}

fn created() -> ResponseTemplate {
    gql(json!({"addPullRequestReview": {"pullRequestReview": {"id": "PRR_new"}}}))
}

fn thread_added() -> ResponseTemplate {
    gql(json!({"addPullRequestReviewThread": {"thread": {"id": "PRRT_1"}}}))
}

fn submitted() -> ResponseTemplate {
    gql(json!({"submitPullRequestReview": {"pullRequestReview": {"id": "PRR_new"}}}))
}

fn dc(start: Option<u32>, line: u32, side: Side, body: &str) -> DraftComment {
    DraftComment {
        path: "src/lib.rs".into(),
        side,
        start_line: start,
        line,
        body: body.into(),
    }
}

async fn bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            let text = String::from_utf8_lossy(&r.body).to_string();
            assert!(!text.contains(TOKEN), "token leaked into a request body");
            serde_json::from_str(&text).unwrap()
        })
        .collect()
}

fn inputs_for<'a>(all: &'a [Value], op: &str) -> Vec<&'a Value> {
    all.iter()
        .filter(|b| b["query"].as_str().unwrap().contains(op))
        .map(|b| &b["variables"]["input"])
        .collect()
}

const SUGGESTION: &str = "Use this:\n```suggestion\nlet x = 1;\n```";

#[tokio::test]
async fn submit_creates_pending_adds_threads_and_submits() {
    let server = MockServer::start().await;
    mount(&server, "pullRequest(number", no_pending()).await;
    mount(&server, "addPullRequestReview(", created()).await;
    mount(&server, "addPullRequestReviewThread(", thread_added()).await;
    mount(&server, "submitPullRequestReview(", submitted()).await;
    let draft = ReviewDraft {
        body: "Nice work".into(),
        comments: vec![
            dc(None, 4, Side::New, SUGGESTION),
            dc(Some(10), 12, Side::Old, "range"),
        ],
    };
    provider(&server)
        .submit_review(&change(), &draft, Verdict::Approve)
        .await
        .unwrap();
    let all = bodies(&server).await;
    assert_eq!(all.len(), 5);
    assert_eq!(
        all[0]["variables"],
        json!({"owner": "acme", "name": "widgets", "number": 7})
    );
    assert_eq!(
        inputs_for(&all, "addPullRequestReview(")[0],
        &json!({"pullRequestId": "PR_1"})
    );
    let threads = inputs_for(&all, "addPullRequestReviewThread(");
    assert_eq!(
        threads[0],
        &json!({"pullRequestReviewId": "PRR_new", "path": "src/lib.rs", "line": 4, "side": "RIGHT", "body": SUGGESTION})
    );
    assert_eq!(
        threads[1],
        &json!({"pullRequestReviewId": "PRR_new", "path": "src/lib.rs", "line": 12, "side": "LEFT", "startLine": 10, "startSide": "LEFT", "body": "range"})
    );
    assert_eq!(
        inputs_for(&all, "submitPullRequestReview(")[0],
        &json!({"pullRequestReviewId": "PRR_new", "event": "APPROVE", "body": "Nice work"})
    );
}

#[tokio::test]
async fn verdicts_map_to_events_and_empty_body_is_omitted() {
    for (verdict, body, event) in [
        (Verdict::Comment, "", "COMMENT"),
        (Verdict::RequestChanges, "Please fix", "REQUEST_CHANGES"),
        (Verdict::Approve, "  ", "APPROVE"),
    ] {
        let server = MockServer::start().await;
        mount(&server, "pullRequest(number", no_pending()).await;
        mount(&server, "addPullRequestReview(", created()).await;
        mount(&server, "addPullRequestReviewThread(", thread_added()).await;
        mount(&server, "submitPullRequestReview(", submitted()).await;
        let draft = ReviewDraft {
            body: body.into(),
            comments: vec![dc(None, 1, Side::New, "c")],
        };
        provider(&server)
            .submit_review(&change(), &draft, verdict)
            .await
            .unwrap();
        let all = bodies(&server).await;
        let input = inputs_for(&all, "submitPullRequestReview(")[0].clone();
        assert_eq!(input["event"], event);
        assert_eq!(input.get("body").is_some(), !body.trim().is_empty());
    }
}

#[tokio::test]
async fn existing_pending_review_is_reused_without_duplicates() {
    let server = MockServer::start().await;
    mount(
        &server,
        "pullRequest(number",
        pending_with(json!([{"path": "src/lib.rs", "line": 4, "body": "already"}])),
    )
    .await;
    mount(&server, "addPullRequestReviewThread(", thread_added()).await;
    mount(&server, "submitPullRequestReview(", submitted()).await;
    let draft = ReviewDraft {
        body: String::new(),
        comments: vec![
            dc(None, 4, Side::New, "already"),
            dc(None, 9, Side::New, "fresh"),
        ],
    };
    provider(&server)
        .submit_review(&change(), &draft, Verdict::Comment)
        .await
        .unwrap();
    let all = bodies(&server).await;
    assert!(inputs_for(&all, "addPullRequestReview(").is_empty());
    let threads = inputs_for(&all, "addPullRequestReviewThread(");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0]["body"], "fresh");
    assert_eq!(threads[0]["pullRequestReviewId"], "PRR_old");
    assert_eq!(
        inputs_for(&all, "submitPullRequestReview(")[0]["pullRequestReviewId"],
        "PRR_old"
    );
}

#[tokio::test]
async fn partial_failure_leaves_review_pending_and_says_so() {
    let server = MockServer::start().await;
    mount(&server, "pullRequest(number", no_pending()).await;
    mount(&server, "addPullRequestReview(", created()).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Op("addPullRequestReviewThread("))
        .respond_with(thread_added())
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount(
        &server,
        "addPullRequestReviewThread(",
        ResponseTemplate::new(502),
    )
    .await;
    let draft = ReviewDraft {
        body: String::new(),
        comments: vec![dc(None, 1, Side::New, "one"), dc(None, 2, Side::New, "two")],
    };
    let err = provider(&server)
        .submit_review(&change(), &draft, Verdict::Comment)
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("saved as pending"), "{text}");
    assert!(text.contains("1 of 2 comments"), "{text}");
    assert!(text.contains("Try again"), "{text}");
    let all = bodies(&server).await;
    assert!(inputs_for(&all, "submitPullRequestReview(").is_empty());
    assert!(!all
        .iter()
        .any(|b| b["query"].as_str().unwrap().contains("delete")));
}

#[tokio::test]
async fn submit_failure_after_comments_leaves_pending() {
    let server = MockServer::start().await;
    mount(&server, "pullRequest(number", no_pending()).await;
    mount(&server, "addPullRequestReview(", created()).await;
    mount(&server, "addPullRequestReviewThread(", thread_added()).await;
    mount(
        &server,
        "submitPullRequestReview(",
        gql_err("FORBIDDEN", "nope"),
    )
    .await;
    let draft = ReviewDraft {
        body: "b".into(),
        comments: vec![dc(None, 1, Side::New, "one")],
    };
    let err = provider(&server)
        .submit_review(&change(), &draft, Verdict::Approve)
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("saved as pending") && text.contains("1 of 1"),
        "{text}"
    );
    assert!(text.contains("can read but not review"), "{text}");
}

#[tokio::test]
async fn validation_errors_send_nothing() {
    let server = MockServer::start().await;
    let p = provider(&server);
    let empty = ReviewDraft::default();
    let e = p
        .submit_review(&change(), &empty, Verdict::RequestChanges)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("short summary"));
    let e = p
        .submit_review(&change(), &empty, Verdict::Comment)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("nothing to post"));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn post_now_is_a_single_comment_review() {
    let server = MockServer::start().await;
    mount(&server, "pullRequest(number", no_pending()).await;
    mount(&server, "addPullRequestReview(", created()).await;
    mount(&server, "addPullRequestReviewThread(", thread_added()).await;
    mount(&server, "submitPullRequestReview(", submitted()).await;
    let draft = ReviewDraft {
        body: String::new(),
        comments: vec![dc(None, 3, Side::New, "quick note")],
    };
    provider(&server)
        .submit_review(&change(), &draft, Verdict::Comment)
        .await
        .unwrap();
    let all = bodies(&server).await;
    assert_eq!(inputs_for(&all, "addPullRequestReviewThread(").len(), 1);
    assert_eq!(
        inputs_for(&all, "submitPullRequestReview(")[0],
        &json!({"pullRequestReviewId": "PRR_new", "event": "COMMENT"})
    );
}

#[tokio::test]
async fn reply_posts_on_the_thread_node_id() {
    let server = MockServer::start().await;
    mount(
        &server,
        "addPullRequestReviewThreadReply(",
        gql(json!({"addPullRequestReviewThreadReply": {"comment": {
            "id": "PRRC_9", "author": {"login": "octo"}, "body": "thanks",
            "createdAt": "2026-01-31T14:05:09Z", "state": "SUBMITTED"
        }}})),
    )
    .await;
    let c = provider(&server)
        .reply(&ThreadId::new("PRRT_5"), "thanks")
        .await
        .unwrap();
    assert_eq!(
        (c.id.as_str(), c.author.as_str(), c.pending),
        ("PRRC_9", "octo", false)
    );
    let all = bodies(&server).await;
    assert_eq!(
        all[0]["variables"]["input"],
        json!({"pullRequestReviewThreadId": "PRRT_5", "body": "thanks"})
    );
    let e = provider(&server)
        .reply(&ThreadId::new("PRRT_5"), "  ")
        .await
        .unwrap_err();
    assert!(e.to_string().contains("empty"));
}

#[tokio::test]
async fn resolve_and_unresolve() {
    let server = MockServer::start().await;
    mount(
        &server,
        ": ResolveReviewThreadInput",
        gql(json!({"resolveReviewThread": {"thread": {"id": "T", "isResolved": true}}})),
    )
    .await;
    mount(
        &server,
        "UnresolveReviewThreadInput",
        gql(json!({"unresolveReviewThread": {"thread": {"id": "T", "isResolved": false}}})),
    )
    .await;
    let p = provider(&server);
    p.resolve(&ThreadId::new("T"), true).await.unwrap();
    p.resolve(&ThreadId::new("T"), false).await.unwrap();
    let all = bodies(&server).await;
    assert_eq!(all[0]["variables"]["input"], json!({"threadId": "T"}));
    assert!(all[0]["query"]
        .as_str()
        .unwrap()
        .contains("($input: ResolveReviewThreadInput!)"));
    assert!(all[1]["query"]
        .as_str()
        .unwrap()
        .contains("UnresolveReviewThreadInput"));
}

async fn reply_error(resp: ResponseTemplate) -> Error {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(resp)
        .mount(&server)
        .await;
    provider(&server)
        .reply(&ThreadId::new("T"), "hi")
        .await
        .unwrap_err()
}

#[tokio::test]
async fn http_errors_map_with_next_steps() {
    assert!(matches!(
        reply_error(ResponseTemplate::new(401)).await,
        Error::Unauthorized { .. }
    ));
    let e = reply_error(ResponseTemplate::new(403).set_body_json(json!({"message": "x"}))).await;
    assert!(e.to_string().contains("can read but not review") && e.to_string().contains("`repo`"));
    let e = reply_error(
        ResponseTemplate::new(403)
            .insert_header("x-github-sso", "required; url=https://sso.test/x"),
    )
    .await;
    assert!(e.to_string().contains("https://sso.test/x"));
    assert!(matches!(
        reply_error(ResponseTemplate::new(404)).await,
        Error::NotFound(m) if m.contains("gone")
    ));
    let e = reply_error(
        ResponseTemplate::new(422)
            .set_body_json(json!({"message": "Line must be part of the diff"})),
    )
    .await;
    assert!(e.to_string().contains("Refresh the diff"), "{e}");
    assert!(matches!(
        reply_error(ResponseTemplate::new(409)).await,
        Error::Conflict(_)
    ));
    assert!(matches!(
        reply_error(ResponseTemplate::new(429).insert_header("retry-after", "30")).await,
        Error::RateLimited {
            retry_after_secs: Some(30),
            ..
        }
    ));
}

#[tokio::test]
async fn graphql_errors_map_with_next_steps() {
    let e = reply_error(gql_err("NOT_FOUND", "Could not resolve")).await;
    assert!(matches!(e, Error::NotFound(_)));
    let e = reply_error(gql_err("INSUFFICIENT_SCOPES", "scopes")).await;
    assert!(e.to_string().contains("`repo`"));
    let e = reply_error(gql_err("RATE_LIMITED", "slow")).await;
    assert!(matches!(e, Error::RateLimited { .. }));
    let e = reply_error(gql_err("UNPROCESSABLE", "Line must be part of the diff")).await;
    assert!(e.to_string().contains("Refresh the diff"));
    let e = reply_error(gql_err(
        "UNPROCESSABLE",
        "Can not approve your own pull request",
    ))
    .await;
    assert!(matches!(e, Error::Conflict(m) if m.contains("comment instead")));
    assert!(!e_text(&reply_error(gql_err("X", "boom")).await).contains(TOKEN));
}

fn e_text(e: &Error) -> String {
    e.to_string()
}

#[tokio::test]
async fn missing_pull_request_is_not_found() {
    let server = MockServer::start().await;
    mount(
        &server,
        "pullRequest(number",
        gql(json!({"repository": null})),
    )
    .await;
    let draft = ReviewDraft {
        body: "x".into(),
        comments: vec![],
    };
    let e = provider(&server)
        .submit_review(&change(), &draft, Verdict::Comment)
        .await
        .unwrap_err();
    assert!(matches!(e, Error::NotFound(_)));
}

#[tokio::test]
async fn merge_and_rerun_stay_unsupported() {
    let server = MockServer::start().await;
    let p = provider(&server);
    let opts = rb_core::MergeOpts {
        method: rb_core::MergeMethod::Merge,
        delete_branch: false,
    };
    assert!(matches!(
        p.merge(&change(), &opts).await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        p.rerun_failed(&change()).await,
        Err(Error::Unsupported(_))
    ));
}

/// Opt-in live smoke test. Never point this at a real project: it posts a review.
///
/// Create a throwaway repo with an open pull request, then run:
/// `REVIEW_BUDDY_LIVE_WRITE_REPO=you/throwaway REVIEW_BUDDY_LIVE_WRITE_PR=1 GITHUB_TOKEN=... \
///  cargo test -p rb-github --test writes -- --ignored live_smoke`
#[tokio::test]
#[ignore = "writes to a live repository; set REVIEW_BUDDY_LIVE_WRITE_REPO to a throwaway repo"]
async fn live_smoke() {
    let Ok(repo) = std::env::var("REVIEW_BUDDY_LIVE_WRITE_REPO") else {
        eprintln!("REVIEW_BUDDY_LIVE_WRITE_REPO isn't set; skipping");
        return;
    };
    let number = std::env::var("REVIEW_BUDDY_LIVE_WRITE_PR")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);
    let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN for the throwaway repo");
    let client = GithubClient::new("github.com", None, Secret::new(token)).unwrap();
    let provider = GithubProvider::new(client);
    let id = ChangeId {
        source_id: SourceId::new("live"),
        kind: ForgeKind::GitHub,
        repo,
        number,
    };
    let draft = ReviewDraft {
        body: "Review Buddy live smoke test".into(),
        comments: vec![],
    };
    provider
        .submit_review(&id, &draft, Verdict::Comment)
        .await
        .unwrap();
}
