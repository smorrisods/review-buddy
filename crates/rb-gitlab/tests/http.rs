use rb_core::{ChangeId, Error, ForgeKind, Provider, SourceId};
use rb_gitlab::{GitlabClient, GitlabProvider};
use rb_platform::Secret;
use serde_json::json;
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "glpat-supersecrettoken";

fn client(server: &MockServer, base_path: &str) -> GitlabClient {
    GitlabClient::new(
        "gitlab.test",
        Some(&format!("{}{base_path}", server.uri())),
        Secret::new(TOKEN),
    )
    .unwrap()
}

fn user_response() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("ratelimit-limit", "2000")
        .insert_header("ratelimit-remaining", "1990")
        .insert_header("ratelimit-reset", "1700000000")
        .set_body_json(json!({"username": "octo", "name": "Octo Cat"}))
}

async fn mount_user(server: &MockServer) {
    Mock::given(path("/user"))
        .respond_with(user_response())
        .mount(server)
        .await;
}

#[tokio::test]
async fn whoami_sends_private_token_and_tracks_rate_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("private-token", TOKEN))
        .respond_with(user_response())
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server, "");
    assert!(c.rate_limit().is_none());
    let user = c.whoami().await.unwrap();
    assert_eq!(user.login, "octo");
    assert_eq!(user.name.as_deref(), Some("Octo Cat"));
    let rate = c.rate_limit().unwrap();
    assert_eq!(
        (rate.limit, rate.remaining, rate.reset),
        (2000, 1990, 1_700_000_000)
    );
}

#[tokio::test]
async fn bearer_mode_sends_authorization_only() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(user_response())
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server, "").with_bearer();
    c.whoami().await.unwrap();
    let requests = server.received_requests().await.unwrap();
    assert!(!requests[0].headers.contains_key("private-token"));
}

#[tokio::test]
async fn self_hosted_api_url_with_prefix_and_trailing_slash() {
    let server = MockServer::start().await;
    Mock::given(path("/gitlab/api/v4/user"))
        .respond_with(user_response())
        .expect(1)
        .mount(&server)
        .await;
    let c = GitlabClient::new(
        "gitlab.test",
        Some(&format!("{}/gitlab/api/v4/", server.uri())),
        Secret::new(TOKEN),
    )
    .unwrap();
    c.whoami().await.unwrap();
}

#[tokio::test]
async fn test_token_reports_scopes_and_expiry() {
    let server = MockServer::start().await;
    mount_user(&server).await;
    Mock::given(path("/personal_access_tokens/self"))
        .and(header_exists("private-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 1, "name": "rb", "active": true,
            "scopes": ["api", "read_user"], "expires_at": "2027-01-12"
        })))
        .mount(&server)
        .await;
    let r = client(&server, "").test_token().await.unwrap();
    assert_eq!(r.login, "octo");
    assert_eq!(r.scopes, ["api", "read_user"]);
    assert_eq!(r.expires.as_deref(), Some("2027-01-12"));
    assert_eq!(r.rate.unwrap().remaining, 1990);
}

#[tokio::test]
async fn test_token_without_expiry() {
    let server = MockServer::start().await;
    mount_user(&server).await;
    Mock::given(path("/personal_access_tokens/self"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"scopes": ["read_api"], "expires_at": null})),
        )
        .mount(&server)
        .await;
    let r = client(&server, "").test_token().await.unwrap();
    assert_eq!(r.expires, None);
    assert_eq!(r.scopes, ["read_api"]);
}

#[tokio::test]
async fn test_token_tolerates_tokens_that_cannot_describe_themselves() {
    for status in [404, 403, 401] {
        let server = MockServer::start().await;
        mount_user(&server).await;
        Mock::given(path("/personal_access_tokens/self"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"message": "no"})))
            .mount(&server)
            .await;
        let r = client(&server, "").test_token().await.unwrap();
        assert_eq!(r.login, "octo");
        assert!(r.scopes.is_empty() && r.expires.is_none(), "{status}");
    }
}

#[tokio::test]
async fn unauthorized_asks_to_sign_in_again() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"message": "401 Unauthorized"})),
        )
        .mount(&server)
        .await;
    let e = client(&server, "").whoami().await.unwrap_err();
    assert_eq!(
        e,
        Error::Unauthorized {
            host: "gitlab.test".into()
        }
    );
}

#[tokio::test]
async fn forbidden_names_the_scopes() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error": "insufficient_scope", "scope": "api read_user"})),
        )
        .mount(&server)
        .await;
    let Error::Forbidden { reason, .. } = client(&server, "").whoami().await.unwrap_err() else {
        panic!("expected Forbidden")
    };
    assert!(reason.contains("api, read_user"), "{reason}");
}

#[tokio::test]
async fn rate_limited_reports_retry_after() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "42"))
        .mount(&server)
        .await;
    assert_eq!(
        client(&server, "").whoami().await.unwrap_err(),
        Error::RateLimited {
            host: "gitlab.test".into(),
            retry_after_secs: Some(42)
        }
    );
}

#[tokio::test]
async fn not_found_and_server_errors() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "404 Not Found"})))
        .mount(&server)
        .await;
    assert!(matches!(
        client(&server, "").whoami().await.unwrap_err(),
        Error::NotFound(_)
    ));
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&server)
        .await;
    assert!(matches!(
        client(&server, "").whoami().await.unwrap_err(),
        Error::Api(_)
    ));
}

#[tokio::test]
async fn unreachable_host_is_a_network_error() {
    let c = GitlabClient::new(
        "gitlab.test",
        Some("http://127.0.0.1:1"),
        Secret::new(TOKEN),
    )
    .unwrap();
    let e = c.whoami().await.unwrap_err();
    assert!(matches!(e, Error::Network { .. }));
    assert!(!e.to_string().contains(TOKEN));
}

#[tokio::test]
async fn errors_never_contain_the_token() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
        .mount(&server)
        .await;
    let c = client(&server, "");
    let e = c.whoami().await.unwrap_err();
    assert!(!format!("{e} {e:?} {c:?}").contains("supersecret"));
}

#[tokio::test]
async fn provider_skeleton() {
    let server = MockServer::start().await;
    mount_user(&server).await;
    let p = GitlabProvider::new(client(&server, ""));
    assert_eq!(p.kind(), ForgeKind::GitLab);
    assert_eq!(p.whoami().await.unwrap().login, "octo");
    let id = ChangeId {
        source_id: SourceId::new("lab"),
        kind: ForgeKind::GitLab,
        repo: "grp/sub/proj".into(),
        number: 7,
    };
    assert_eq!(p.checkout_refspec(&id), "merge-requests/7/head");
    assert_eq!(
        p.web_url(&id).as_str(),
        "https://gitlab.test/grp/sub/proj/-/merge_requests/7"
    );
    assert!(matches!(
        p.merge(
            &id,
            &rb_core::MergeOpts {
                method: rb_core::MergeMethod::Merge,
                delete_branch: false
            }
        )
        .await
        .unwrap_err(),
        Error::Unsupported(_)
    ));
    assert!(matches!(
        p.rerun_failed(&id).await.unwrap_err(),
        Error::Unsupported(_)
    ));
    let caps = p.capabilities();
    assert!(!caps.request_changes && !caps.viewed_files && caps.range_comments);
}
