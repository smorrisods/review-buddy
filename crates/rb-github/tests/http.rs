use rb_core::{Error, ForgeKind, Provider};
use rb_github::{graphql, GithubClient, GithubProvider};
use rb_platform::Secret;
use serde::Deserialize;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "ghp_supersecrettoken";

fn client(server: &MockServer, base_path: &str) -> GithubClient {
    GithubClient::new(
        "ghe.test",
        Some(&format!("{}{base_path}", server.uri())),
        Secret::new(TOKEN),
    )
    .unwrap()
}

fn user_response() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("x-oauth-scopes", "repo, read:org")
        .insert_header("x-ratelimit-limit", "5000")
        .insert_header("x-ratelimit-remaining", "4990")
        .insert_header("x-ratelimit-reset", "1700000000")
        .set_body_json(json!({"login": "octo", "name": "Octo Cat"}))
}

#[tokio::test]
async fn whoami_sends_auth_and_tracks_rate_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .and(header("accept", "application/vnd.github+json"))
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
        (5000, 4990, 1_700_000_000)
    );
}

#[tokio::test]
async fn provider_whoami_and_unsupported() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(user_response())
        .mount(&server)
        .await;
    let p = GithubProvider::new(client(&server, ""));
    assert_eq!(p.kind(), ForgeKind::GitHub);
    assert_eq!(p.whoami().await.unwrap().login, "octo");
    let id = rb_core::ChangeId {
        source_id: rb_core::SourceId::new("work"),
        kind: ForgeKind::GitHub,
        repo: "o/r".into(),
        number: 7,
    };
    assert!(matches!(
        p.change_detail(&id).await,
        Err(Error::Unsupported(_))
    ));
    assert_eq!(p.web_url(&id).as_str(), "https://ghe.test/o/r/pull/7");
}

#[tokio::test]
async fn test_token_reports_scopes_limits_and_sso() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            user_response().insert_header("x-github-sso", "partial-results; organizations=12,34"),
        )
        .mount(&server)
        .await;
    Mock::given(path("/rate_limit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": {
                "core": {"limit": 5000, "remaining": 4989, "reset": 1700000100},
                "graphql": {"limit": 5000, "remaining": 4000, "reset": 1700000200}
            }
        })))
        .mount(&server)
        .await;
    let report = client(&server, "").test_token().await.unwrap();
    assert_eq!(report.login, "octo");
    assert_eq!(report.scopes, ["repo", "read:org"]);
    assert_eq!(report.core.remaining, 4989);
    assert_eq!(report.graphql.unwrap().remaining, 4000);
    assert!(report.sso_hint.unwrap().contains("SSO"));
}

#[tokio::test]
async fn unauthorized_asks_to_sign_in_without_leaking_token() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"message": "Bad credentials"})),
        )
        .mount(&server)
        .await;
    let e = client(&server, "").whoami().await.unwrap_err();
    assert_eq!(
        e,
        Error::Unauthorized {
            host: "ghe.test".into()
        }
    );
    assert!(!format!("{e} {e:?}").contains(TOKEN));
}

#[tokio::test]
async fn sso_403_points_at_the_authorisation_url() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header(
                    "x-github-sso",
                    "required; url=https://github.com/orgs/acme/sso?authorization_request=abc",
                )
                .set_body_json(
                    json!({"message": "Resource protected by organization SAML enforcement."}),
                ),
        )
        .mount(&server)
        .await;
    let e = client(&server, "").whoami().await.unwrap_err();
    let Error::Forbidden { reason, .. } = e else {
        panic!("expected Forbidden, got {e:?}");
    };
    assert!(reason.contains("https://github.com/orgs/acme/sso?authorization_request=abc"));
    assert!(reason.contains("Authorise"));
}

#[tokio::test]
async fn primary_rate_limit_403_reports_reset() {
    let server = MockServer::start().await;
    let reset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 600;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-limit", "5000")
                .insert_header("x-ratelimit-reset", reset.to_string().as_str())
                .set_body_json(json!({"message": "API rate limit exceeded"})),
        )
        .mount(&server)
        .await;
    let c = client(&server, "");
    let Error::RateLimited {
        retry_after_secs, ..
    } = c.whoami().await.unwrap_err()
    else {
        panic!("expected RateLimited");
    };
    let secs = retry_after_secs.unwrap();
    assert!((590..=600).contains(&secs), "{secs}");
    assert_eq!(c.rate_limit().unwrap().remaining, 0);
}

#[tokio::test]
async fn secondary_rate_limit_429_uses_retry_after() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "42"))
        .mount(&server)
        .await;
    let e = client(&server, "").whoami().await.unwrap_err();
    assert_eq!(
        e,
        Error::RateLimited {
            host: "ghe.test".into(),
            retry_after_secs: Some(42)
        }
    );
}

#[tokio::test]
async fn not_found_and_conflict() {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .mount(&server)
        .await;
    Mock::given(path("/rate_limit"))
        .respond_with(ResponseTemplate::new(409))
        .mount(&server)
        .await;
    let c = client(&server, "");
    assert!(matches!(c.whoami().await, Err(Error::NotFound(_))));
    assert!(matches!(c.test_token().await, Err(Error::NotFound(_))));
}

#[tokio::test]
async fn network_error_is_calm_and_clean() {
    let c = GithubClient::new("ghe.test", Some("http://127.0.0.1:1"), Secret::new(TOKEN)).unwrap();
    let e = c.whoami().await.unwrap_err();
    let Error::Network { host, reason } = &e else {
        panic!("expected Network, got {e:?}");
    };
    assert_eq!(host, "ghe.test");
    assert!(!reason.contains("127.0.0.1"));
    assert!(!format!("{e} {e:?}").contains(TOKEN));
}

#[tokio::test]
async fn enterprise_base_path_is_honoured() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v3/user"))
        .respond_with(user_response())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/graphql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"viewer": {"login": "octo"}}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server, "/api/v3");
    c.whoami().await.unwrap();
    let r: graphql::Response<Viewer> = graphql::query(&c, "{ viewer { login } }", json!({}))
        .await
        .unwrap();
    assert_eq!(r.data.viewer.login, "octo");
}

#[derive(Deserialize)]
struct Viewer {
    viewer: Login,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

const VIEWER_QUERY: &str = "query { viewer { login } rateLimit { cost remaining resetAt } }";

#[tokio::test]
async fn graphql_success_with_cost() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "viewer": {"login": "octo"},
                "rateLimit": {"cost": 1, "remaining": 4999, "resetAt": "2026-01-01T00:00:00Z"}
            }
        })))
        .mount(&server)
        .await;
    let r: graphql::Response<Viewer> =
        graphql::query(&client(&server, ""), VIEWER_QUERY, json!({}))
            .await
            .unwrap();
    assert_eq!(r.data.viewer.login, "octo");
    let cost = r.cost.unwrap();
    assert_eq!((cost.cost, cost.remaining), (1, 4999));
    assert!(r.errors.is_empty());
}

#[tokio::test]
async fn graphql_error_array_maps() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": null,
            "errors": [{"type": "NOT_FOUND", "message": "Could not resolve to a Repository"}]
        })))
        .mount(&server)
        .await;
    let e = graphql::query::<Viewer>(&client(&server, ""), "query { x }", json!({}))
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(matches!(&e, Error::NotFound(m) if m.contains("Could not resolve")));
}

#[tokio::test]
async fn graphql_rate_limited_and_generic_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errors": [{"message": "Field 'nope' doesn't exist"}]
        })))
        .mount(&server)
        .await;
    let c = client(&server, "");
    let e = graphql::query::<Viewer>(&c, "q", json!({}))
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(matches!(e, Error::RateLimited { .. }));
    let e = graphql::query::<Viewer>(&c, "q", json!({}))
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(matches!(e, Error::Api(m) if m.contains("doesn't exist")));
}

#[tokio::test]
async fn graphql_partial_data_keeps_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"viewer": {"login": "octo"}},
            "errors": [{"type": "FORBIDDEN", "message": "hidden repo"}]
        })))
        .mount(&server)
        .await;
    let r = graphql::query::<Viewer>(&client(&server, ""), "q", json!({}))
        .await
        .unwrap();
    assert_eq!(r.errors.len(), 1);
}

#[tokio::test]
async fn graphql_http_errors_use_the_shared_mapping() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let e = graphql::query::<Viewer>(&client(&server, ""), "q", json!({}))
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(matches!(e, Error::Unauthorized { .. }));
}
