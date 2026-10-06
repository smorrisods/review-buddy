//! GitHub Enterprise Server layouts: a path prefix, a port, a trailing slash and plain http.

use rb_core::{ChangeId, ForgeKind, Provider, Scope, SourceId};
use rb_github::{GithubClient, GithubProvider};
use rb_platform::Secret;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer, api_path: &str) -> GithubClient {
    GithubClient::new(
        "ghe.corp.test",
        Some(&format!("{}{api_path}", server.uri())),
        Secret::new("ghp_token"),
    )
    .unwrap()
}

fn empty() -> serde_json::Value {
    let file = format!("{}/tests/fixtures/empty.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

#[tokio::test]
async fn rest_and_graphql_follow_a_path_prefix_with_a_trailing_slash() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ghe/api/v3/user"))
        .and(header("authorization", "Bearer ghp_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"login": "octo"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/ghe/api/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty()))
        .expect(5)
        .mount(&server)
        .await;
    let c = client(&server, "/ghe/api/v3/");
    assert_eq!(c.whoami().await.unwrap().login, "octo");
    let provider = GithubProvider::new(c).with_source_id(SourceId::new("work"));
    provider
        .list_changes(&Scope::everything(), None)
        .await
        .unwrap();
}

#[tokio::test]
async fn probe_reads_the_enterprise_version_and_server_clock() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v3/meta"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-github-enterprise-version", "3.12.4")
                .insert_header("date", "Tue, 14 Nov 2023 22:13:20 GMT")
                .set_body_json(json!({"verifiable_password_authentication": false})),
        )
        .mount(&server)
        .await;
    let info = client(&server, "/api/v3").probe().await.unwrap();
    assert_eq!(info.enterprise_version.as_deref(), Some("3.12.4"));
    assert_eq!(info.server_time, Some(1_700_000_000));
}

#[tokio::test]
async fn probe_on_a_server_without_the_header_has_no_version() {
    let server = MockServer::start().await;
    Mock::given(path("/meta"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;
    let info = client(&server, "").probe().await.unwrap();
    assert_eq!(info.enterprise_version, None);
}

#[test]
fn web_urls_use_the_enterprise_origin_prefix_and_port() {
    let id = ChangeId {
        source_id: SourceId::new("work"),
        kind: ForgeKind::GitHub,
        repo: "acme/widgets".into(),
        number: 7,
    };
    let url = |host: &str, api: Option<&str>| {
        let c = GithubClient::new(host, api, Secret::new("t")).unwrap();
        GithubProvider::new(c).web_url(&id).to_string()
    };
    assert_eq!(
        url("github.com", None),
        "https://github.com/acme/widgets/pull/7"
    );
    assert_eq!(
        url("ghe.corp.test", None),
        "https://ghe.corp.test/acme/widgets/pull/7"
    );
    assert_eq!(
        url("ghe.corp.test:8443", None),
        "https://ghe.corp.test:8443/acme/widgets/pull/7"
    );
    assert_eq!(
        url("x", Some("https://corp.test:8443/ghe/api/v3/")),
        "https://corp.test:8443/ghe/acme/widgets/pull/7"
    );
    assert_eq!(
        url("x", Some("http://127.0.0.1:8080/api/v3")),
        "http://127.0.0.1:8080/acme/widgets/pull/7"
    );
}

#[tokio::test]
async fn an_unreachable_host_gets_a_calm_network_error() {
    let c = GithubClient::new(
        "ghe.test",
        Some("http://127.0.0.1:9/api/v3"),
        Secret::new("t"),
    )
    .unwrap();
    let err = c.whoami().await.unwrap_err().to_string();
    assert!(err.contains("couldn't reach ghe.test"), "{err}");
    assert!(err.contains("api_url"), "{err}");
}
