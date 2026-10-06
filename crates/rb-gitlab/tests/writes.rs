use rb_core::{
    ChangeId, DraftComment, Error, ForgeKind, Provider, ReviewDraft, Side, SourceId, ThreadId,
    Verdict,
};
use rb_gitlab::{GitlabClient, GitlabProvider};
use rb_platform::Secret;
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MR: &str = "/projects/platform%2Fflow/merge_requests/11";
const PATCH: &str = "@@ -1,3 +1,4 @@\n ctx\n-old\n+new\n+newer\n ctx\n";

fn provider(server: &MockServer, base_path: &str) -> GitlabProvider {
    let client = GitlabClient::new(
        "gitlab.test",
        Some(&format!("{}{base_path}", server.uri())),
        Secret::new("glpat-stub"),
    )
    .unwrap();
    GitlabProvider::new(client).with_source_id(SourceId::new("lab"))
}

fn id() -> ChangeId {
    ChangeId {
        source_id: SourceId::new("lab"),
        kind: ForgeKind::GitLab,
        repo: "platform/flow".into(),
        number: 11,
    }
}

fn draft(comments: Vec<DraftComment>, body: &str) -> ReviewDraft {
    ReviewDraft {
        body: body.into(),
        comments,
    }
}

fn comment(start: Option<u32>, line: u32, side: Side, body: &str) -> DraftComment {
    DraftComment {
        path: "a.rs".into(),
        side,
        start_line: start,
        line,
        body: body.into(),
    }
}

async fn mount(server: &MockServer, verb: &str, p: &str, response: ResponseTemplate) {
    Mock::given(method(verb))
        .and(path(p.to_string()))
        .respond_with(response)
        .mount(server)
        .await;
}

fn json_ok(status: u16, body: Value) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(body)
}

#[derive(Default)]
struct Stub {
    drafts: Option<Value>,
    approved_by: Option<Value>,
}

async fn serve(server: &MockServer, stub: Stub) {
    let u = |tail: &str| format!("{MR}{tail}");
    mount(
        server,
        "GET",
        "/user",
        json_ok(200, json!({"username": "octo"})),
    )
    .await;
    mount(
        server,
        "GET",
        MR,
        json_ok(
            200,
            json!({"iid": 11, "sha": "head", "diff_refs": {"base_sha": "base", "start_sha": "start", "head_sha": "head"}}),
        ),
    )
    .await;
    mount(
        server,
        "GET",
        &u("/diffs"),
        json_ok(
            200,
            json!([{"old_path": "a.rs", "new_path": "a.rs", "diff": PATCH}]),
        ),
    )
    .await;
    mount(server, "GET", &u("/discussions"), json_ok(200, json!([]))).await;
    mount(
        server,
        "GET",
        &u("/draft_notes"),
        json_ok(200, stub.drafts.unwrap_or_else(|| json!([]))),
    )
    .await;
    mount(
        server,
        "GET",
        &u("/approvals"),
        json_ok(
            200,
            json!({"approved_by": stub.approved_by.unwrap_or_else(|| json!([]))}),
        ),
    )
    .await;
    mount(
        server,
        "POST",
        &u("/draft_notes"),
        json_ok(201, json!({"id": 50})),
    )
    .await;
    mount(
        server,
        "POST",
        &u("/draft_notes/bulk_publish"),
        ResponseTemplate::new(204),
    )
    .await;
    mount(server, "POST", &u("/approve"), json_ok(201, json!({}))).await;
}

/// Non-GET requests as `(method, path, body)`, in order.
async fn writes(server: &MockServer) -> Vec<(String, String, Value)> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() != "GET")
        .map(|r| {
            (
                r.method.to_string(),
                r.url.path().to_string(),
                serde_json::from_slice(&r.body).unwrap_or(Value::Null),
            )
        })
        .collect()
}

fn code(path: &str, old: u32, new: u32) -> String {
    use sha1::{Digest, Sha1};
    let hex: String = Sha1::digest(path.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{hex}_{old}_{new}")
}

#[tokio::test]
async fn a_comment_then_approve_sends_the_exact_requests() {
    let server = MockServer::start().await;
    serve(&server, Stub::default()).await;
    let review = draft(vec![comment(None, 3, Side::New, "Looks right")], "");
    provider(&server, "")
        .submit_review(&id(), &review, Verdict::Approve)
        .await
        .unwrap();
    let draft_path = format!("{MR}/draft_notes");
    assert_eq!(
        writes(&server).await,
        vec![
            (
                "POST".to_string(),
                draft_path.clone(),
                json!({"note": "Looks right", "position": {
                    "position_type": "text", "base_sha": "base", "start_sha": "start",
                    "head_sha": "head", "new_path": "a.rs", "old_path": "a.rs", "new_line": 3
                }})
            ),
            (
                "POST".to_string(),
                format!("{draft_path}/bulk_publish"),
                json!({})
            ),
            (
                "POST".to_string(),
                format!("{MR}/approve"),
                json!({"sha": "head"})
            ),
        ]
    );
}

#[tokio::test]
async fn range_and_old_side_positions_carry_line_codes_and_both_numbers() {
    let server = MockServer::start().await;
    serve(&server, Stub::default()).await;
    let review = draft(
        vec![
            comment(Some(2), 3, Side::New, "Range"),
            comment(None, 2, Side::Old, "Removed"),
            comment(None, 1, Side::New, "Context"),
        ],
        "Summary",
    );
    provider(&server, "")
        .submit_review(&id(), &review, Verdict::Comment)
        .await
        .unwrap();
    let sent = writes(&server).await;
    let positions: Vec<&Value> = sent.iter().map(|w| &w.2["position"]).collect();
    assert_eq!(
        positions[0]["line_range"],
        json!({
            "start": {"line_code": code("a.rs", 3, 2), "type": "new", "new_line": 2},
            "end": {"line_code": code("a.rs", 3, 3), "type": "new", "new_line": 3}
        })
    );
    assert_eq!(positions[0]["new_line"], 3);
    assert_eq!(positions[1]["old_line"], 2);
    assert!(positions[1].get("new_line").is_none());
    assert_eq!(positions[2]["old_line"], 1);
    assert_eq!(positions[2]["new_line"], 1);
    assert_eq!(sent[3].2, json!({"note": "Summary"}));
    assert!(sent[4].1.ends_with("/bulk_publish"));
    assert_eq!(sent.len(), 5, "a comment-only review never approves");
}

#[tokio::test]
async fn existing_drafts_are_reused_and_published_not_duplicated() {
    let server = MockServer::start().await;
    serve(
        &server,
        Stub {
            drafts: Some(
                json!([{"id": 9, "note": "Looks right", "discussion_id": null,
                "position": {"new_path": "a.rs", "old_path": "a.rs", "new_line": 3}}]),
            ),
            ..Stub::default()
        },
    )
    .await;
    let review = draft(
        vec![
            comment(None, 3, Side::New, "Looks right"),
            comment(None, 2, Side::New, "Also this"),
        ],
        "",
    );
    provider(&server, "")
        .submit_review(&id(), &review, Verdict::Comment)
        .await
        .unwrap();
    let sent = writes(&server).await;
    let tails: Vec<_> = sent
        .iter()
        .map(|w| w.1.rsplit('/').next().unwrap().to_string())
        .collect();
    assert_eq!(tails, ["draft_notes", "bulk_publish"]);
    assert_eq!(sent[0].2["note"], "Also this");
}

#[tokio::test]
async fn pending_drafts_are_published_even_when_nothing_new_is_added() {
    let server = MockServer::start().await;
    serve(
        &server,
        Stub {
            drafts: Some(
                json!([{"id": 9, "note": "old thought", "discussion_id": null, "position": null}]),
            ),
            ..Stub::default()
        },
    )
    .await;
    provider(&server, "")
        .submit_review(&id(), &draft(vec![], ""), Verdict::Approve)
        .await
        .unwrap();
    let names: Vec<_> = writes(&server).await.into_iter().map(|w| w.1).collect();
    assert_eq!(
        names,
        [
            format!("{MR}/draft_notes/bulk_publish"),
            format!("{MR}/approve")
        ]
    );
}

#[tokio::test]
async fn an_approval_you_already_gave_is_left_alone() {
    let server = MockServer::start().await;
    serve(
        &server,
        Stub {
            approved_by: Some(json!([{"user": {"username": "octo"}}])),
            ..Stub::default()
        },
    )
    .await;
    provider(&server, "")
        .submit_review(&id(), &draft(vec![], ""), Verdict::Approve)
        .await
        .unwrap();
    assert!(writes(&server).await.is_empty());
}

#[tokio::test]
async fn a_failure_after_some_drafts_leaves_them_pending_and_says_so() {
    let server = MockServer::start().await;
    serve(&server, Stub::default()).await;
    Mock::given(method("POST"))
        .and(path(format!("{MR}/draft_notes")))
        .and(body_partial_json(json!({"note": "two"})))
        .respond_with(json_ok(500, json!({"message": "boom"})))
        .with_priority(1)
        .mount(&server)
        .await;
    let review = draft(
        vec![
            comment(None, 3, Side::New, "one"),
            comment(None, 2, Side::New, "two"),
        ],
        "",
    );
    let err = provider(&server, "")
        .submit_review(&id(), &review, Verdict::Approve)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("saved as pending on GitLab (1 of 2 comments added)"),
        "{err}"
    );
    assert!(err.contains("nothing was lost"), "{err}");
    let sent = writes(&server).await;
    assert!(sent.iter().all(|w| w.0 == "POST"), "nothing deleted");
    assert!(sent
        .iter()
        .all(|w| !w.1.ends_with("bulk_publish") && !w.1.ends_with("approve")));
}

#[tokio::test]
async fn a_failed_publish_says_drafts_are_pending() {
    let server = MockServer::start().await;
    serve(&server, Stub::default()).await;
    Mock::given(method("POST"))
        .and(path(format!("{MR}/draft_notes/bulk_publish")))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&server)
        .await;
    let review = draft(vec![comment(None, 3, Side::New, "one")], "");
    let err = provider(&server, "")
        .submit_review(&id(), &review, Verdict::Comment)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("saved as pending on GitLab (1 of 1"), "{err}");
}

#[tokio::test]
async fn a_failed_approval_after_publishing_says_the_comments_went_out() {
    let server = MockServer::start().await;
    serve(&server, Stub::default()).await;
    let review = draft(vec![comment(None, 3, Side::New, "one")], "");
    let e = failing(
        &server,
        "/approve",
        json_ok(401, json!({"message": "401 Unauthorized"})),
        &review,
    )
    .await;
    assert!(text(&e).contains("won't let this account approve"), "{e:?}");
    assert!(text(&e).contains("comments were already posted"), "{e:?}");
}

async fn failing(
    server: &MockServer,
    endpoint: &str,
    response: ResponseTemplate,
    review: &ReviewDraft,
) -> Error {
    Mock::given(method("POST"))
        .and(path(format!("{MR}{endpoint}")))
        .respond_with(response)
        .with_priority(1)
        .mount(server)
        .await;
    provider(server, "")
        .submit_review(&id(), review, Verdict::Approve)
        .await
        .unwrap_err()
}

async fn fail_with(endpoint: &str, response: ResponseTemplate) -> Error {
    let server = MockServer::start().await;
    serve(&server, Stub::default()).await;
    let review = draft(vec![comment(None, 3, Side::New, "one")], "");
    failing(&server, endpoint, response, &review).await
}

fn text(e: &Error) -> String {
    format!("{e:?} {e}")
}

#[tokio::test]
async fn write_errors_are_mapped_with_a_next_step() {
    let draft_notes = "/draft_notes";
    let e = fail_with(
        draft_notes,
        json_ok(403, json!({"message": "403 Forbidden"})),
    )
    .await;
    assert!(text(&e).contains("`api` scope"), "{e:?}");

    let e = fail_with(
        draft_notes,
        json_ok(404, json!({"message": "404 Not found"})),
    )
    .await;
    assert!(
        text(&e).contains("gone, or your token can't see it"),
        "{e:?}"
    );

    let e = fail_with(
        draft_notes,
        json_ok(422, json!({"message": "Position is invalid"})),
    )
    .await;
    assert!(text(&e).contains("Refresh the diff"), "{e:?}");

    let bad = json!({"message": "400 Bad request - line_code can't be blank"});
    let e = fail_with(draft_notes, json_ok(400, bad)).await;
    assert!(text(&e).contains("Refresh the diff"), "{e:?}");

    let e = fail_with(
        draft_notes,
        json_ok(422, json!({"message": "Note can't be blank"})),
    )
    .await;
    assert!(text(&e).contains("couldn't accept that"), "{e:?}");

    let stale = json!({"message": "SHA does not match HEAD of source branch"});
    let e = fail_with("/approve", json_ok(409, stale)).await;
    assert!(text(&e).contains("new commits"), "{e:?}");

    let e = fail_with(
        draft_notes,
        ResponseTemplate::new(429).insert_header("retry-after", "42"),
    )
    .await;
    assert!(text(&e).contains("try again in 42 seconds"), "{e:?}");

    let e = fail_with(draft_notes, ResponseTemplate::new(401)).await;
    assert!(text(&e).contains("was rejected"), "{e:?}");
}

#[tokio::test]
async fn request_changes_is_unsupported_and_sends_nothing() {
    let server = MockServer::start().await;
    serve(&server, Stub::default()).await;
    let err = provider(&server, "")
        .submit_review(&id(), &draft(vec![], "Please fix"), Verdict::RequestChanges)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)), "{err:?}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_drafts_send_nothing() {
    let server = MockServer::start().await;
    serve(&server, Stub::default()).await;
    let review = draft(vec![comment(None, 3, Side::New, "  ")], "");
    assert!(provider(&server, "")
        .submit_review(&id(), &review, Verdict::Comment)
        .await
        .is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn reply_and_resolve_use_the_discussion_endpoints_on_a_self_hosted_base() {
    let server = MockServer::start().await;
    let base = "/gl/api/v4";
    mount(
        &server,
        "POST",
        &format!("{base}{MR}/discussions/abc/notes"),
        json_ok(
            201,
            json!({"id": 301, "body": "Thanks", "author": {"username": "octo"},
                   "created_at": "2026-02-03T10:00:00.000Z"}),
        ),
    )
    .await;
    mount(
        &server,
        "PUT",
        &format!("{base}{MR}/discussions/abc"),
        json_ok(200, json!({"id": "abc"})),
    )
    .await;
    let p = provider(&server, base);
    let thread = ThreadId::new("platform/flow!11!abc");
    let c = p.reply(&thread, "Thanks").await.unwrap();
    assert_eq!(
        (c.body.as_str(), c.author.as_str(), c.pending),
        ("Thanks", "octo", false)
    );
    p.resolve(&thread, true).await.unwrap();
    p.resolve(&thread, false).await.unwrap();
    let sent = writes(&server).await;
    assert_eq!(sent[0].2, json!({"body": "Thanks"}));
    assert_eq!(sent[1].2, json!({"resolved": true}));
    assert_eq!(sent[2].2, json!({"resolved": false}));

    assert!(p.reply(&thread, "  ").await.is_err());
    assert!(matches!(
        p.reply(&ThreadId::new("platform/flow!11!draft:9"), "x")
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        p.resolve(&ThreadId::new("PRRT_node"), true).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn reply_errors_are_mapped() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/projects/platform%2Fflow/merge_requests/11/discussions/abc/notes",
        json_ok(403, json!({"message": "403 Forbidden"})),
    )
    .await;
    let thread = ThreadId::new("platform/flow!11!abc");
    let e = provider(&server, "").reply(&thread, "x").await.unwrap_err();
    assert!(text(&e).contains("`api` scope"), "{e:?}");
}

/// Posts a comment-only review on a throwaway project. Opt in with
/// `REVIEW_BUDDY_LIVE_WRITE_GITLAB_PROJECT=you/throwaway REVIEW_BUDDY_LIVE_WRITE_GITLAB_MR=1
/// GITLAB_TOKEN=… cargo test -p rb-gitlab --test writes -- --ignored live_smoke`.
#[tokio::test]
#[ignore = "writes to a real GitLab project; needs REVIEW_BUDDY_LIVE_WRITE_GITLAB_PROJECT"]
async fn live_smoke() {
    let Ok(project) = std::env::var("REVIEW_BUDDY_LIVE_WRITE_GITLAB_PROJECT") else {
        eprintln!("set REVIEW_BUDDY_LIVE_WRITE_GITLAB_PROJECT to a throwaway project to run this");
        return;
    };
    let iid: u64 = std::env::var("REVIEW_BUDDY_LIVE_WRITE_GITLAB_MR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let host = std::env::var("REVIEW_BUDDY_LIVE_WRITE_GITLAB_HOST")
        .unwrap_or_else(|_| "gitlab.com".into());
    let token = std::env::var("GITLAB_TOKEN").expect("GITLAB_TOKEN");
    let client = GitlabClient::new(&host, None, Secret::new(token)).unwrap();
    let provider = GitlabProvider::new(client);
    let id = ChangeId {
        source_id: SourceId::new("live"),
        kind: ForgeKind::GitLab,
        repo: project,
        number: iid,
    };
    let review = ReviewDraft {
        body: "Review Buddy live smoke test".into(),
        comments: Vec::new(),
    };
    provider
        .submit_review(&id, &review, Verdict::Comment)
        .await
        .unwrap();
}
