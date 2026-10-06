//! The recorded-shape fixtures that no other test file exercises: `/version` on both editions,
//! `/personal_access_tokens/self`, the `/changes` fallback, the file shapes in `/diffs`, and the
//! 404 error body.

use rb_core::{ChangeId, Error, FileStatus, ForgeKind, Provider, SourceId};
use rb_gitlab::{GitlabClient, GitlabProvider};
use rb_platform::Secret;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MR: &str = "/projects/platform%2Fflow/merge_requests/11";

fn fixture(name: &str) -> Value {
    let file = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

async fn get(server: &MockServer, p: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(response)
        .mount(server)
        .await;
}

fn client(server: &MockServer) -> GitlabClient {
    GitlabClient::new(
        "gitlab.test",
        Some(&server.uri()),
        Secret::new("glpat-stub"),
    )
    .unwrap()
}

fn provider(server: &MockServer) -> GitlabProvider {
    GitlabProvider::new(client(server)).with_source_id(SourceId::new("lab"))
}

fn id() -> ChangeId {
    ChangeId {
        source_id: SourceId::new("lab"),
        kind: ForgeKind::GitLab,
        repo: "platform/flow".into(),
        number: 11,
    }
}

fn ok(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(fixture(name))
}

async fn mount_user(server: &MockServer) {
    get(server, "/user", ok("user")).await;
}

#[tokio::test]
async fn an_enterprise_version_enables_request_changes_and_a_community_one_explains() {
    let server = MockServer::start().await;
    mount_user(&server).await;
    get(&server, "/version", ok("version")).await;
    get(
        &server,
        "/personal_access_tokens/self",
        ok("personal_access_token_self"),
    )
    .await;
    let out = provider(&server).probe().await.unwrap();
    assert!(out.complete);
    assert_eq!(out.version.as_deref(), Some("17.5.1"));
    assert!(out.capabilities.request_changes && out.capabilities.rerun_failed);

    let server = MockServer::start().await;
    mount_user(&server).await;
    get(&server, "/version", ok("version_ce")).await;
    get(
        &server,
        "/personal_access_tokens/self",
        ok("personal_access_token_self"),
    )
    .await;
    let out = provider(&server).probe().await.unwrap();
    assert_eq!(out.version.as_deref(), Some("16.11.2"));
    assert!(!out.capabilities.request_changes);
    assert!(out.reason(rb_core::FeatureAction::RequestChanges).is_some());
}

#[tokio::test]
async fn the_token_description_gives_scopes_and_expiry() {
    let server = MockServer::start().await;
    mount_user(&server).await;
    get(
        &server,
        "/personal_access_tokens/self",
        ok("personal_access_token_self"),
    )
    .await;
    let report = client(&server).test_token().await.unwrap();
    assert_eq!(report.login, "octo");
    assert_eq!(report.scopes, ["api", "read_user"]);
    assert_eq!(report.expires.as_deref(), Some("2027-01-12"));
}

#[tokio::test]
async fn files_come_from_the_changes_response_when_diffs_is_missing() {
    let server = MockServer::start().await;
    get(
        &server,
        &format!("{MR}/diffs"),
        ResponseTemplate::new(404).set_body_json(fixture("error_404")),
    )
    .await;
    get(&server, &format!("{MR}/changes"), ok("mr_changes")).await;
    let files = provider(&server).files(&id()).await.unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[1].path, "new_name.rs");
    assert_eq!(files[1].status, FileStatus::Renamed);
}

#[tokio::test]
async fn diff_entries_map_every_shape() {
    let server = MockServer::start().await;
    get(&server, &format!("{MR}/diffs"), ok("diffs_shapes")).await;
    let files = provider(&server).files(&id()).await.unwrap();
    let status = |path: &str| files.iter().find(|f| f.path == path).map(|f| f.status);
    assert_eq!(status("src/lib.rs"), Some(FileStatus::Modified));
    assert_eq!(status("gone.txt"), Some(FileStatus::Removed));
    assert_eq!(status("added.txt"), Some(FileStatus::Added));
    assert_eq!(status("new/name.txt"), Some(FileStatus::Renamed));
    assert_eq!(status("docs/b.md"), Some(FileStatus::Renamed));
    let modified = files.iter().find(|f| f.path == "src/lib.rs").unwrap();
    assert!(modified.patch.is_some());
}

#[tokio::test]
async fn a_404_body_becomes_not_found() {
    let server = MockServer::start().await;
    get(
        &server,
        &format!("{MR}/diffs"),
        ResponseTemplate::new(404).set_body_json(json!(fixture("error_404"))),
    )
    .await;
    get(
        &server,
        &format!("{MR}/changes"),
        ResponseTemplate::new(404).set_body_json(fixture("error_404")),
    )
    .await;
    let err = provider(&server).files(&id()).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}
