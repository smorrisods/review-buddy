use rb_core::{ChangeId, CiState, Error, FileStatus, ForgeKind, Provider, Side, SourceId};
use rb_github::{GithubClient, GithubProvider};
use rb_platform::Secret;
use serde_json::Value;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

fn fixture(name: &str) -> Value {
    let file = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

fn provider(server: &MockServer) -> GithubProvider {
    let client =
        GithubClient::new("ghe.test", Some(&server.uri()), Secret::new("ghp_token")).unwrap();
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

fn json(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(fixture(name))
}

fn paged(server: &MockServer, name: &str, rest_path: &str, next: u32) -> ResponseTemplate {
    json(name).insert_header(
        "link",
        format!(
            "<{}{rest_path}?per_page=100&page={next}>; rel=\"next\"",
            server.uri()
        )
        .as_str(),
    )
}

/// Matches a GraphQL call by one variable's value (`null` when absent).
struct Var(&'static str, Value);

impl Match for Var {
    fn matches(&self, request: &Request) -> bool {
        serde_json::from_slice::<Value>(&request.body)
            .is_ok_and(|b| b["variables"].get(self.0).unwrap_or(&Value::Null) == &self.1)
    }
}

async fn serve_files(server: &MockServer) {
    let p = "/repos/acme/widgets/pulls/7/files";
    Mock::given(method("GET"))
        .and(path(p))
        .and(query_param_is_missing("page"))
        .respond_with(paged(server, "files_p1", p, 2))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(p))
        .and(query_param("page", "2"))
        .respond_with(json("files_p2"))
        .mount(server)
        .await;
}

#[tokio::test]
async fn files_paginate_and_map_status_and_patch() {
    let server = MockServer::start().await;
    serve_files(&server).await;
    let files = provider(&server).files(&change()).await.unwrap();

    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "src/lib.rs",
            "assets/logo.png",
            "src/new_name.rs",
            "src/gone.rs"
        ]
    );
    assert_eq!((files[0].adds, files[0].dels), (2, 1));
    assert!(files[0]
        .patch
        .as_deref()
        .unwrap()
        .starts_with("@@ -1,3 +1,4 @@"));
    assert_eq!(files[0].status, FileStatus::Modified);

    assert_eq!(files[1].status, FileStatus::Added);
    assert_eq!(files[1].patch, None);

    assert_eq!(files[2].status, FileStatus::Renamed);
    assert_eq!(files[2].old_path.as_deref(), Some("src/old_name.rs"));
    assert_eq!(files[3].status, FileStatus::Removed);
    assert_eq!(files[3].dels, 4);
}

#[tokio::test]
async fn files_send_auth_and_per_page() {
    let server = MockServer::start().await;
    serve_files(&server).await;
    provider(&server).files(&change()).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].url.query(), Some("per_page=100"));
    assert_eq!(
        requests[0]
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap(),
        "Bearer ghp_token"
    );
}

#[tokio::test]
async fn files_refuse_a_next_link_to_another_host() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/7/files"))
        .respond_with(
            json("files_p1")
                .insert_header("link", "<https://evil.example/files?page=2>; rel=\"next\""),
        )
        .mount(&server)
        .await;
    let err = provider(&server).files(&change()).await.unwrap_err();
    assert!(matches!(err, Error::Api(_)), "{err:?}");
}

#[tokio::test]
async fn files_missing_pull_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(serde_json::json!({"message": "Not Found"})),
        )
        .mount(&server)
        .await;
    let err = provider(&server).files(&change()).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn files_rate_limit_and_sso_failures() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", "4102444800")
                .set_body_json(serde_json::json!({"message": "API rate limit exceeded"})),
        )
        .mount(&server)
        .await;
    let err = provider(&server).files(&change()).await.unwrap_err();
    assert!(matches!(err, Error::RateLimited { .. }), "{err:?}");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header(
                    "x-github-sso",
                    "required; url=https://github.com/orgs/acme/sso?authorization_request=abc",
                )
                .set_body_json(serde_json::json!({"message": "Resource protected by organization SAML enforcement"})),
        )
        .mount(&server)
        .await;
    let Error::Forbidden { reason, .. } = provider(&server).files(&change()).await.unwrap_err()
    else {
        panic!("expected Forbidden");
    };
    assert!(reason.contains("authorization_request=abc"));
}

async fn serve_threads(server: &MockServer) {
    let gql = || Mock::given(method("POST")).and(path("/graphql"));
    gql()
        .and(Var("id", Value::from("PRT_1")))
        .respond_with(json("thread_comments_more"))
        .mount(server)
        .await;
    gql()
        .and(Var("number", Value::from(7)))
        .and(Var("after", Value::Null))
        .respond_with(json("threads_p1"))
        .mount(server)
        .await;
    gql()
        .and(Var("after", Value::from("t1")))
        .respond_with(json("threads_p2"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/issues/7/comments"))
        .respond_with(json("issue_comments"))
        .mount(server)
        .await;
}

#[tokio::test]
async fn threads_page_through_threads_and_comments() {
    let server = MockServer::start().await;
    serve_threads(&server).await;
    let threads = provider(&server).threads(&change()).await.unwrap();

    let ids: Vec<&str> = threads.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "PRT_1",
            "PRT_2",
            "PRT_3",
            "PRT_4",
            "issue-comment:9001",
            "issue-comment:9002"
        ]
    );

    let multi = &threads[0];
    assert_eq!(
        (multi.path.as_deref(), multi.line, multi.side),
        (Some("src/lib.rs"), Some(2), Side::Old)
    );
    let bodies: Vec<&str> = multi.comments.iter().map(|c| c.body.as_str()).collect();
    assert_eq!(bodies, ["Why drop this?", "It moved.", "Thanks."]);
    assert_eq!(multi.comments[0].author, "alice");
    assert!(multi.comments[1].created_at > multi.comments[0].created_at);
    assert!(!multi.resolved && !multi.outdated);
}

#[tokio::test]
async fn threads_map_resolved_outdated_and_pending_suggestion() {
    let server = MockServer::start().await;
    serve_threads(&server).await;
    let threads = provider(&server).threads(&change()).await.unwrap();

    let resolved = &threads[1];
    assert!(resolved.resolved && !resolved.outdated);
    assert_eq!((resolved.line, resolved.side), (Some(3), Side::New));

    let outdated = &threads[2];
    assert!(outdated.outdated);
    assert_eq!(outdated.line, Some(9));
    assert_eq!(outdated.comments[0].author, "ghost");

    let pending = &threads[3];
    assert_eq!((pending.line, pending.side), (Some(4), Side::New));
    assert!(pending.comments[0]
        .body
        .contains("```suggestion\nfn c() -> u8 { 2 }\n```"));
}

#[tokio::test]
async fn issue_comments_are_unanchored() {
    let server = MockServer::start().await;
    serve_threads(&server).await;
    let threads = provider(&server).threads(&change()).await.unwrap();
    let convo = &threads[4];
    assert_eq!((convo.path.clone(), convo.line), (None, None));
    assert_eq!(convo.comments.len(), 1);
    assert_eq!(convo.comments[0].author, "carol");
    assert_eq!(threads[5].comments[0].author, "ghost");
}

#[tokio::test]
async fn threads_for_an_unreadable_repo_are_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(json("detail_missing"))
        .mount(&server)
        .await;
    let err = provider(&server).threads(&change()).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

async fn serve_checks(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/7"))
        .respond_with(json("pull_head"))
        .mount(server)
        .await;
    let p = "/repos/acme/widgets/commits/abc123/check-runs";
    Mock::given(method("GET"))
        .and(path(p))
        .and(query_param_is_missing("page"))
        .respond_with(paged(server, "check_runs_p1", p, 2))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(p))
        .and(query_param("page", "2"))
        .respond_with(json("check_runs_p2"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/commits/abc123/status"))
        .respond_with(json("commit_status"))
        .mount(server)
        .await;
}

#[tokio::test]
async fn checks_merge_runs_and_legacy_statuses() {
    let server = MockServer::start().await;
    serve_checks(&server).await;
    let checks = provider(&server).checks(&change()).await.unwrap();

    let got: Vec<(&str, CiState)> = checks.iter().map(|c| (c.name.as_str(), c.state)).collect();
    assert_eq!(
        got,
        [
            ("build", CiState::Pass),
            ("lint", CiState::Fail),
            ("e2e", CiState::Running),
            ("docs", CiState::Pass),
            ("deploy/preview", CiState::Running),
            ("license/cla", CiState::Pass),
        ]
    );
    let url = |i: usize| checks[i].url.as_ref().map(|u| u.as_str().to_string());
    assert_eq!(url(0).as_deref(), Some("https://ci.example/build"));
    assert_eq!(
        url(1).as_deref(),
        Some("https://github.com/acme/widgets/runs/2")
    );
    assert_eq!(url(3), None);
    assert_eq!(url(4).as_deref(), Some("https://legacy.example/deploy"));
}

#[tokio::test]
async fn checks_surface_failures() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(serde_json::json!({"message": "Not Found"})),
        )
        .mount(&server)
        .await;
    let err = provider(&server).checks(&change()).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}
