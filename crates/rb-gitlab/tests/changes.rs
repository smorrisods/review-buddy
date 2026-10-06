use rb_core::{
    ChangeId, ChangeState, CiState, Error, ForgeKind, Mergeability, MyReview, MyRole, Provider,
    ReviewerState, Scope, SourceId,
};
use rb_gitlab::{GitlabClient, GitlabProvider};
use rb_platform::Secret;
use serde_json::Value;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Value {
    let file = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

fn ok(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(fixture(name))
}

fn provider(server: &MockServer, base_path: &str) -> GitlabProvider {
    let client = GitlabClient::new(
        "gitlab.test",
        Some(&format!("{}{base_path}", server.uri())),
        Secret::new("glpat-stub"),
    )
    .unwrap();
    GitlabProvider::new(client).with_source_id(SourceId::new("lab"))
}

async fn mount_user(server: &MockServer, base: &str) {
    Mock::given(method("GET"))
        .and(path(format!("{base}/user")))
        .respond_with(ok("user"))
        .mount(server)
        .await;
}

async fn mount_list(server: &MockServer, list_path: &str, param: &str, fixture: &str) {
    Mock::given(method("GET"))
        .and(path(list_path))
        .and(query_param(param, "octo"))
        .and(query_param("state", "opened"))
        .and(query_param("scope", "all"))
        .respond_with(ok(fixture))
        .mount(server)
        .await;
}

async fn mount_todos(server: &MockServer, base: &str, fixture: &str) {
    Mock::given(method("GET"))
        .and(path(format!("{base}/todos")))
        .and(query_param("action", "mentioned"))
        .and(query_param("type", "MergeRequest"))
        .respond_with(ok(fixture))
        .mount(server)
        .await;
}

async fn mount_everywhere(server: &MockServer, base: &str) {
    mount_user(server, base).await;
    let list = format!("{base}/merge_requests");
    Mock::given(method("GET"))
        .and(path(list.as_str()))
        .and(query_param("reviewer_username", "octo"))
        .and(query_param("page", "1"))
        .respond_with(ok("reviewer_p1").insert_header("x-next-page", "2"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(list.as_str()))
        .and(query_param("reviewer_username", "octo"))
        .and(query_param("page", "2"))
        .respond_with(ok("reviewer_p2").insert_header("x-next-page", ""))
        .mount(server)
        .await;
    mount_list(server, &list, "assignee_username", "assignee").await;
    mount_list(server, &list, "author_username", "author").await;
    mount_todos(server, base, "todos").await;
}

#[tokio::test]
async fn everything_scope_merges_pages_queries_and_mentions_without_duplicates() {
    let server = MockServer::start().await;
    mount_everywhere(&server, "").await;
    let page = provider(&server, "")
        .list_changes(&Scope::everything(), None)
        .await
        .unwrap();

    let refs: Vec<String> = page.items.iter().map(|c| c.id.short_ref()).collect();
    assert_eq!(
        refs,
        [
            "platform/flow!11",
            "platform/sub/api!12",
            "platform/flow!13",
            "octo/dotfiles!14",
            "other/thing!15"
        ]
    );
    assert!(page.etag.is_some() && !page.not_modified);

    let a = &page.items[0];
    assert_eq!(a.id.kind, ForgeKind::GitLab);
    assert_eq!(a.id.source_id, SourceId::new("lab"));
    assert_eq!(a.title, "Add retry to sync");
    assert_eq!(a.author, "mira");
    assert_eq!(a.my_role, MyRole::Reviewing);
    assert_eq!(a.state, ChangeState::Open);
    assert_eq!(a.branch, "feat/11");
    assert_eq!(a.base, "main");
    assert_eq!(a.head_sha, "11".repeat(20));
    assert_eq!(a.labels, ["sync", "needs-review"]);
    assert_eq!(a.ci, CiState::None);
    assert_eq!(a.my_review, MyReview::None);
    assert_eq!(a.reviewers.len(), 2);
    assert!(a
        .reviewers
        .iter()
        .all(|r| r.state == ReviewerState::Requested));

    let draft = &page.items[1];
    assert!(draft.draft);
    assert_eq!(draft.my_role, MyRole::Reviewing);

    let bot = &page.items[2];
    assert!(bot.author_is_bot);
    assert!(!a.author_is_bot);
    assert_eq!(bot.my_role, MyRole::Assigned);

    assert_eq!(page.items[3].my_role, MyRole::Authored);
    assert_eq!(page.items[4].my_role, MyRole::Mentioned);
}

#[tokio::test]
async fn an_unchanged_result_comes_back_not_modified() {
    let server = MockServer::start().await;
    mount_everywhere(&server, "").await;
    let p = provider(&server, "");
    let first = p.list_changes(&Scope::everything(), None).await.unwrap();
    let again = p
        .list_changes(&Scope::everything(), first.etag.clone())
        .await
        .unwrap();
    assert!(again.not_modified);
    assert!(again.items.is_empty());
    assert_eq!(again.etag, first.etag);
}

#[tokio::test]
async fn group_scope_encodes_nested_paths_and_includes_subgroups() {
    let server = MockServer::start().await;
    mount_user(&server, "").await;
    let list = "/groups/platform%2Fsub/merge_requests";
    Mock::given(method("GET"))
        .and(path(list))
        .and(query_param("reviewer_username", "octo"))
        .and(query_param("include_subgroups", "true"))
        .respond_with(ok("reviewer_p2"))
        .mount(&server)
        .await;
    mount_list(&server, list, "assignee_username", "author_group").await;
    mount_list(&server, list, "author_username", "author_group").await;
    mount_todos(&server, "", "todos").await;
    let scope = Scope {
        owners: vec!["platform/sub".into()],
        ..Scope::default()
    };
    let page = provider(&server, "")
        .list_changes(&scope, None)
        .await
        .unwrap();
    let refs: Vec<String> = page.items.iter().map(|c| c.id.short_ref()).collect();
    assert_eq!(refs, ["platform/sub/api!12"]);
}

#[tokio::test]
async fn project_scope_uses_the_encoded_project_path() {
    let server = MockServer::start().await;
    mount_user(&server, "").await;
    let list = "/projects/platform%2Fflow/merge_requests";
    mount_list(&server, list, "reviewer_username", "reviewer_p2").await;
    mount_list(&server, list, "assignee_username", "assignee").await;
    mount_list(&server, list, "author_username", "author_group").await;
    mount_todos(&server, "", "todos").await;
    let scope = Scope {
        repos: vec!["platform/flow".into()],
        ..Scope::default()
    };
    let page = provider(&server, "")
        .list_changes(&scope, None)
        .await
        .unwrap();
    let refs: Vec<String> = page.items.iter().map(|c| c.id.short_ref()).collect();
    assert_eq!(
        refs,
        [
            "platform/flow!11",
            "platform/sub/api!12",
            "platform/flow!13"
        ]
    );
}

#[tokio::test]
async fn user_scope_keeps_only_your_own_namespace() {
    let server = MockServer::start().await;
    mount_everywhere(&server, "").await;
    let scope = Scope {
        user: true,
        ..Scope::default()
    };
    let page = provider(&server, "")
        .list_changes(&scope, None)
        .await
        .unwrap();
    let refs: Vec<String> = page.items.iter().map(|c| c.id.short_ref()).collect();
    assert_eq!(refs, ["octo/dotfiles!14"]);
}

#[tokio::test]
async fn empty_results_are_an_empty_page() {
    let server = MockServer::start().await;
    mount_user(&server, "").await;
    for param in ["reviewer_username", "assignee_username", "author_username"] {
        mount_list(&server, "/merge_requests", param, "author_group").await;
    }
    mount_todos(&server, "", "author_group").await;
    let page = provider(&server, "")
        .list_changes(&Scope::everything(), None)
        .await
        .unwrap();
    assert!(page.items.is_empty());
    assert!(!page.not_modified);
}

#[tokio::test]
async fn self_hosted_base_paths_work() {
    let server = MockServer::start().await;
    mount_everywhere(&server, "/gitlab/api/v4").await;
    let page = provider(&server, "/gitlab/api/v4")
        .list_changes(&Scope::everything(), None)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 5);
}

#[tokio::test]
async fn auth_and_rate_limit_failures_map_to_next_steps() {
    for (status, body, check) in [
        (
            401,
            "error_401",
            (|e: &Error| matches!(e, Error::Unauthorized { .. })) as fn(&Error) -> bool,
        ),
        (
            403,
            "error_403",
            |e| matches!(e, Error::Forbidden { reason, .. } if reason.contains("api, read_api")),
        ),
        (429, "error_429", |e| {
            matches!(
                e,
                Error::RateLimited {
                    retry_after_secs: Some(30),
                    ..
                }
            )
        }),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("retry-after", "30")
                    .set_body_json(fixture(body)),
            )
            .mount(&server)
            .await;
        let err = provider(&server, "")
            .list_changes(&Scope::everything(), None)
            .await
            .unwrap_err();
        assert!(check(&err), "{status}: {err:?}");
    }
}

#[tokio::test]
async fn a_spent_rate_limit_stops_before_the_queries() {
    let server = MockServer::start().await;
    let reset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 600;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(
            ok("user")
                .insert_header("ratelimit-remaining", "2")
                .insert_header("ratelimit-reset", reset.to_string().as_str()),
        )
        .mount(&server)
        .await;
    let err = provider(&server, "")
        .list_changes(&Scope::everything(), None)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::RateLimited {
            retry_after_secs: Some(s),
            ..
        } if s > 0 && s <= 600
    ));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

fn change_id() -> ChangeId {
    ChangeId {
        source_id: SourceId::new("lab"),
        kind: ForgeKind::GitLab,
        repo: "platform/flow".into(),
        number: 11,
    }
}

const MR: &str = "/projects/platform%2Fflow/merge_requests/11";

async fn mount_detail(server: &MockServer) {
    mount_user(server, "").await;
    let get = |p: &str| Mock::given(method("GET")).and(path(format!("{MR}{p}")));
    Mock::given(method("GET"))
        .and(path(MR))
        .respond_with(ok("mr_detail"))
        .mount(server)
        .await;
    get("/approvals")
        .respond_with(ok("approvals"))
        .mount(server)
        .await;
    get("/reviewers")
        .respond_with(ok("mr_reviewers"))
        .mount(server)
        .await;
    get("/commits")
        .and(query_param("per_page", "1"))
        .respond_with(ok("commits").insert_header("x-total", "4"))
        .mount(server)
        .await;
    get("/diffs")
        .and(query_param_is_missing("nope"))
        .respond_with(ok("diffs"))
        .mount(server)
        .await;
}

#[tokio::test]
async fn detail_fills_in_approvals_pipeline_and_counts() {
    let server = MockServer::start().await;
    mount_detail(&server).await;
    let detail = provider(&server, "")
        .change_detail(&change_id())
        .await
        .unwrap();
    let s = &detail.summary;
    assert_eq!(s.id, change_id());
    assert_eq!(s.ci, CiState::Fail);
    assert_eq!(s.head_sha, "c".repeat(40));
    assert_eq!(s.base_sha, "b".repeat(40));
    assert_eq!((s.adds, s.dels, s.files), (3, 2, 3));
    assert_eq!(s.my_review, MyReview::Approved);
    assert!(s.i_commented);
    assert_eq!(s.my_role, MyRole::Reviewing);
    let state = |name: &str| s.reviewers.iter().find(|r| r.login == name).unwrap().state;
    assert_eq!(state("octo"), ReviewerState::Approved);
    assert_eq!(state("sam"), ReviewerState::Approved);
    assert_eq!(state("lee"), ReviewerState::Requested);
    assert_eq!(detail.body, "Body of Add retry to sync.");
    assert_eq!(detail.commit_count, 4);
    assert_eq!(detail.mergeability, Mergeability::Blocked);
    assert_eq!(detail.mergeable, Some(true));
    assert_eq!(
        detail.web_url.as_str(),
        "https://gitlab.test/platform/flow/-/merge_requests/11"
    );
}

#[tokio::test]
async fn detail_degrades_when_extras_are_unavailable() {
    let server = MockServer::start().await;
    mount_user(&server, "").await;
    Mock::given(method("GET"))
        .and(path(MR))
        .respond_with(ok("mr_detail"))
        .mount(&server)
        .await;
    let detail = provider(&server, "")
        .change_detail(&change_id())
        .await
        .unwrap();
    assert_eq!(detail.commit_count, 0);
    assert_eq!((detail.summary.adds, detail.summary.dels), (0, 0));
    assert_eq!(detail.summary.my_review, MyReview::None);
}

#[tokio::test]
async fn detail_of_a_missing_merge_request_says_where() {
    let server = MockServer::start().await;
    mount_user(&server, "").await;
    let err = provider(&server, "")
        .change_detail(&change_id())
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::NotFound(m) if m.contains("platform/flow!11")),
        "{err:?}"
    );
}
