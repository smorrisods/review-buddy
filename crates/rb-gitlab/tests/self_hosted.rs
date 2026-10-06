//! Self-hosted GitLab layouts: a relative URL root, a port, a trailing slash and plain http.

use rb_core::{ChangeId, ForgeKind, Provider, Scope, SourceId};
use rb_gitlab::{GitlabClient, GitlabProvider};
use rb_platform::Secret;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer, api_path: &str) -> GitlabClient {
    GitlabClient::new(
        "git.corp.test",
        Some(&format!("{}{api_path}", server.uri())),
        Secret::new("glpat-token"),
    )
    .unwrap()
}

#[tokio::test]
async fn requests_follow_a_relative_root_with_a_trailing_slash() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gitlab/api/v4/user"))
        .and(header("private-token", "glpat-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"username": "octo"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gitlab/api/v4/merge_requests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(3)
        .mount(&server)
        .await;
    Mock::given(path("/gitlab/api/v4/todos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    let c = client(&server, "/gitlab/api/v4/");
    assert_eq!(c.whoami().await.unwrap().login, "octo");
    let provider = GitlabProvider::new(c).with_source_id(SourceId::new("lab"));
    provider
        .list_changes(&Scope::everything(), None)
        .await
        .unwrap();
}

#[tokio::test]
async fn list_strips_the_relative_root_when_the_project_comes_from_the_web_url() {
    let server = MockServer::start().await;
    let node = |id: u64, iid: u64, project: &str| {
        json!({
            "id": id, "iid": iid, "project_id": 1, "title": "t", "state": "opened",
            "author": {"username": "someone"}, "reviewers": [{"username": "octo"}],
            "created_at": "2026-10-01T10:00:00Z", "updated_at": "2026-10-02T10:00:00Z",
            "web_url": format!("{}/gitlab/{project}/-/merge_requests/{iid}", server.uri()),
        })
    };
    Mock::given(path("/gitlab/api/v4/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"username": "octo"})))
        .mount(&server)
        .await;
    Mock::given(path("/gitlab/api/v4/merge_requests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([node(
            1,
            5,
            "plat/infra/tf"
        )])))
        .mount(&server)
        .await;
    Mock::given(path("/gitlab/api/v4/todos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    let provider =
        GitlabProvider::new(client(&server, "/gitlab/api/v4")).with_source_id(SourceId::new("lab"));
    let page = provider
        .list_changes(&Scope::everything(), None)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id.repo, "plat/infra/tf");
}

#[tokio::test]
async fn probe_reads_the_version_and_server_clock() {
    let server = MockServer::start().await;
    Mock::given(path("/gitlab/api/v4/version"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("date", "Tue, 14 Nov 2023 22:13:20 GMT")
                .set_body_json(json!({"version": "16.11.2-ee", "revision": "abc123def"})),
        )
        .mount(&server)
        .await;
    let info = client(&server, "/gitlab/api/v4").probe().await.unwrap();
    assert_eq!(info.version, "16.11.2-ee");
    assert_eq!(info.revision.as_deref(), Some("abc123def"));
    assert_eq!(info.server_time, Some(1_700_000_000));
}

#[test]
fn web_urls_keep_the_root_port_and_subgroups() {
    let id = ChangeId {
        source_id: SourceId::new("lab"),
        kind: ForgeKind::GitLab,
        repo: "plat/infra/tf".into(),
        number: 12,
    };
    let url = |host: &str, api: Option<&str>| {
        let c = GitlabClient::new(host, api, Secret::new("t")).unwrap();
        GitlabProvider::new(c).web_url(&id).to_string()
    };
    assert_eq!(
        url("gitlab.com", None),
        "https://gitlab.com/plat/infra/tf/-/merge_requests/12"
    );
    assert_eq!(
        url("git.corp.test:8929", None),
        "https://git.corp.test:8929/plat/infra/tf/-/merge_requests/12"
    );
    assert_eq!(
        url("x", Some("https://corp.test/gitlab/api/v4/")),
        "https://corp.test/gitlab/plat/infra/tf/-/merge_requests/12"
    );
    assert_eq!(
        url("x", Some("http://127.0.0.1:8080/api/v4")),
        "http://127.0.0.1:8080/plat/infra/tf/-/merge_requests/12"
    );
}

#[tokio::test]
async fn an_unreachable_host_gets_a_calm_network_error() {
    let c = GitlabClient::new(
        "gl.test",
        Some("http://127.0.0.1:9/api/v4"),
        Secret::new("t"),
    )
    .unwrap();
    let err = c.whoami().await.unwrap_err().to_string();
    assert!(err.contains("couldn't reach gl.test"), "{err}");
    assert!(err.contains("api_url"), "{err}");
}
