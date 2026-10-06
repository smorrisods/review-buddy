use rb_core::{ChangeId, CiState, Error, FileStatus, ForgeKind, Provider, Side, SourceId};
use rb_gitlab::{GitlabClient, GitlabProvider};
use rb_platform::Secret;
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MR: &str = "/projects/platform%2Fflow/merge_requests/11";

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

fn id() -> ChangeId {
    ChangeId {
        source_id: SourceId::new("lab"),
        kind: ForgeKind::GitLab,
        repo: "platform/flow".into(),
        number: 11,
    }
}

async fn get(server: &MockServer, p: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(response)
        .mount(server)
        .await;
}

#[tokio::test]
async fn files_paginate_and_map_every_kind() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{MR}/diffs")))
        .and(query_param("per_page", "50"))
        .and(query_param("page", "1"))
        .respond_with(ok("files_p1").insert_header("x-next-page", "2"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{MR}/diffs")))
        .and(query_param("page", "2"))
        .respond_with(ok("files_p2").insert_header("x-next-page", ""))
        .expect(1)
        .mount(&server)
        .await;
    let files = provider(&server, "/gl/api/v4")
        .files(&id())
        .await
        .unwrap_err();
    // The base path isn't mounted, so a self-hosted prefix is honoured and this 404s.
    assert!(matches!(files, Error::NotFound(_)));

    let files = provider(&server, "").files(&id()).await.unwrap();
    let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "a.rs",
            "new_name.rs",
            "added.txt",
            "gone.rs",
            "logo.png",
            "huge.json",
            "lock.lock"
        ]
    );
    assert_eq!((files[0].adds, files[0].dels), (2, 1));
    assert!(files[0].patch.as_deref().unwrap().starts_with("@@ -1,3"));
    assert_eq!(files[1].status, FileStatus::Renamed);
    assert_eq!(files[1].old_path.as_deref(), Some("old_name.rs"));
    assert_eq!(files[2].status, FileStatus::Added);
    assert_eq!(files[3].status, FileStatus::Removed);
    for hidden in &files[4..] {
        assert_eq!(hidden.patch, None, "{}", hidden.path);
    }
}

#[tokio::test]
async fn files_fall_back_to_changes_when_diffs_is_missing() {
    let server = MockServer::start().await;
    get(
        &server,
        &format!("{MR}/diffs"),
        ResponseTemplate::new(404).set_body_json(json!({"message": "404 Not Found"})),
    )
    .await;
    get(
        &server,
        &format!("{MR}/changes"),
        ResponseTemplate::new(200).set_body_json(json!({"changes": fixture("files_p1")})),
    )
    .await;
    let files = provider(&server, "").files(&id()).await.unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[1].path, "new_name.rs");
}

#[tokio::test]
async fn files_report_a_missing_merge_request() {
    let server = MockServer::start().await;
    let err = provider(&server, "").files(&id()).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn threads_map_discussions_and_drafts() {
    let server = MockServer::start().await;
    get(
        &server,
        "/user",
        ResponseTemplate::new(200).set_body_json(json!({"username": "octo"})),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(format!("{MR}/discussions")))
        .and(query_param("per_page", "100"))
        .respond_with(ok("discussions"))
        .mount(&server)
        .await;
    get(&server, &format!("{MR}/draft_notes"), ok("draft_notes")).await;
    let threads = provider(&server, "").threads(&id()).await.unwrap();
    assert_eq!(threads.len(), 4, "{threads:#?}");

    let range = &threads[0];
    assert_eq!(range.id.as_str(), "platform/flow!11!d_range");
    assert_eq!(range.path.as_deref(), Some("a.rs"));
    assert_eq!((range.line, range.start_line), (Some(4), Some(2)));
    assert_eq!((range.side, range.start_side), (Side::New, Some(Side::New)));
    assert!(range.resolved && !range.pending);
    assert_eq!(range.comments.len(), 2);
    assert_eq!(range.comments[0].author, "ann");
    assert_eq!(range.comments[0].created_at.0, 1_769_940_000);

    let old = &threads[1];
    assert_eq!(
        (old.line, old.side, old.resolved),
        (Some(2), Side::Old, false)
    );
    assert_eq!(old.comments.len(), 2, "the reply draft joins its thread");
    assert!(old.comments[1].pending && old.comments[1].author == "octo");
    assert!(!old.pending);

    let note = &threads[2];
    assert_eq!(
        (note.path.clone(), note.line, note.resolved),
        (None, None, false)
    );
    assert_eq!(note.comments[0].body, "Overall looks fine");

    let draft = &threads[3];
    assert!(draft.pending && draft.comments[0].pending);
    assert_eq!((draft.path.as_deref(), draft.line), (Some("b.rs"), Some(9)));
}

#[tokio::test]
async fn threads_without_draft_notes_support_still_load() {
    let server = MockServer::start().await;
    get(&server, &format!("{MR}/discussions"), ok("discussions")).await;
    get(
        &server,
        &format!("{MR}/draft_notes"),
        ResponseTemplate::new(404),
    )
    .await;
    assert_eq!(provider(&server, "").threads(&id()).await.unwrap().len(), 3);
}

async fn mount_pipeline(server: &MockServer, base: &str) {
    get(
        server,
        &format!("{base}{MR}"),
        ResponseTemplate::new(200).set_body_json(json!({"iid": 11, "head_pipeline": {"id": 77}})),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "{base}/projects/platform%2Fflow/pipelines/77/jobs"
        )))
        .and(header("private-token", "glpat-stub"))
        .respond_with(ok("pipeline_jobs"))
        .mount(server)
        .await;
}

#[tokio::test]
async fn checks_map_jobs_and_bridges_on_a_self_hosted_base() {
    let server = MockServer::start().await;
    mount_pipeline(&server, "/gl/api/v4").await;
    get(
        &server,
        "/gl/api/v4/projects/platform%2Fflow/pipelines/77/bridges",
        ok("pipeline_bridges"),
    )
    .await;
    let checks = provider(&server, "/gl/api/v4").checks(&id()).await.unwrap();
    let got: Vec<_> = checks.iter().map(|c| (c.name.as_str(), c.state)).collect();
    assert_eq!(
        got,
        [
            ("build / build", CiState::Pass),
            ("test / unit", CiState::Running),
            ("test / lint", CiState::Neutral),
            ("test / e2e", CiState::Fail),
            ("deploy / deploy", CiState::Skipped),
            ("deploy / docs", CiState::Cancelled),
            ("deploy / perf", CiState::Running),
            ("deploy / pkg", CiState::Skipped),
            ("deploy / trigger-deploy", CiState::Pass),
        ]
    );
    assert_eq!(checks[0].duration_secs(), Some(120));
    assert_eq!(checks[0].required, Some(true));
    assert_eq!(checks[2].required, Some(false));
    assert_eq!(
        checks[0].url.as_ref().map(url::Url::as_str),
        Some("https://gitlab.test/platform/flow/-/jobs/1")
    );
}

#[tokio::test]
async fn checks_are_empty_without_a_pipeline_and_survive_missing_bridges() {
    let server = MockServer::start().await;
    get(
        &server,
        MR,
        ResponseTemplate::new(200).set_body_json(json!({"iid": 11, "head_pipeline": null})),
    )
    .await;
    assert!(provider(&server, "")
        .checks(&id())
        .await
        .unwrap()
        .is_empty());

    let server = MockServer::start().await;
    mount_pipeline(&server, "").await;
    get(
        &server,
        "/projects/platform%2Fflow/pipelines/77/bridges",
        ResponseTemplate::new(403),
    )
    .await;
    assert_eq!(provider(&server, "").checks(&id()).await.unwrap().len(), 8);
}

#[test]
fn capabilities_are_honest() {
    let server_less =
        GitlabProvider::new(GitlabClient::new("gitlab.test", None, Secret::new("t")).unwrap());
    let caps = server_less.capabilities();
    assert!(!caps.request_changes && !caps.viewed_files && !caps.rerun_failed);
    assert!(caps.range_comments && caps.resolve_threads);
}
