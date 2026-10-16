//! Viewed marks: reading `viewerViewedState` across pages, and the two mutations with the
//! pull request's node id looked up first.

use rb_core::{ChangeId, Error, ForgeKind, Provider, SourceId};
use rb_github::{GithubClient, GithubProvider};
use rb_platform::Secret;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

fn provider(server: &MockServer) -> GithubProvider {
    let client = GithubClient::new("ghe.test", Some(&server.uri()), Secret::new("ghp_x")).unwrap();
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

struct Op(&'static str);

impl Match for Op {
    fn matches(&self, r: &Request) -> bool {
        serde_json::from_slice::<Value>(&r.body)
            .is_ok_and(|b| b["query"].as_str().is_some_and(|q| q.contains(self.0)))
    }
}

/// Matches a call whose variables hold `value` at `pointer`.
struct Var(&'static str, Value);

impl Match for Var {
    fn matches(&self, r: &Request) -> bool {
        serde_json::from_slice::<Value>(&r.body)
            .is_ok_and(|b| b["variables"].pointer(self.0) == Some(&self.1))
    }
}

fn gql(data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "data": data }))
}

fn page(files: &[(&str, &str)], next: Option<&str>) -> ResponseTemplate {
    let nodes: Vec<Value> = files
        .iter()
        .map(|(p, s)| json!({ "path": p, "viewerViewedState": s }))
        .collect();
    gql(json!({
        "repository": { "pullRequest": { "files": {
            "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
            "nodes": nodes
        } } }
    }))
}

async fn mount(server: &MockServer, matcher: impl Match + 'static, resp: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(matcher)
        .respond_with(resp)
        .mount(server)
        .await;
}

#[tokio::test]
async fn reads_viewed_files_across_pages_and_leaves_out_dismissed_ones() {
    let server = MockServer::start().await;
    mount(
        &server,
        Var("/after", Value::Null),
        page(&[("a.rs", "VIEWED"), ("b.rs", "UNVIEWED")], Some("c1")),
    )
    .await;
    mount(
        &server,
        Var("/after", json!("c1")),
        page(&[("c.rs", "DISMISSED"), ("d.png", "VIEWED")], None),
    )
    .await;
    let viewed = provider(&server).viewed_files(&change()).await.unwrap();
    assert_eq!(viewed, ["a.rs", "d.png"]);
}

#[tokio::test]
async fn marking_looks_up_the_pull_request_then_sends_its_id_and_the_path() {
    let server = MockServer::start().await;
    mount(
        &server,
        Op("pullRequest(number: $number) { id }"),
        gql(json!({ "repository": { "pullRequest": { "id": "PR_node" } } })),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Op("markFileAsViewed"))
        .and(Var("/input/pullRequestId", json!("PR_node")))
        .and(Var("/input/path", json!("src/a b.rs")))
        .respond_with(gql(
            json!({ "markFileAsViewed": { "pullRequest": { "id": "PR_node" } } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    provider(&server)
        .set_file_viewed(&change(), "src/a b.rs", true)
        .await
        .unwrap();
}

#[tokio::test]
async fn unmarking_uses_the_other_mutation() {
    let server = MockServer::start().await;
    mount(
        &server,
        Op("pullRequest(number: $number) { id }"),
        gql(json!({ "repository": { "pullRequest": { "id": "PR_node" } } })),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Op("unmarkFileAsViewed"))
        .respond_with(gql(
            json!({ "unmarkFileAsViewed": { "pullRequest": { "id": "PR_node" } } }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    provider(&server)
        .set_file_viewed(&change(), "a.rs", false)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_missing_pull_request_says_so() {
    let server = MockServer::start().await;
    mount(
        &server,
        Op("pullRequest(number: $number) { id }"),
        gql(json!({ "repository": { "pullRequest": null } })),
    )
    .await;
    let err = provider(&server)
        .set_file_viewed(&change(), "a.rs", true)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn a_rejected_mutation_is_an_error_not_a_silent_success() {
    let server = MockServer::start().await;
    mount(
        &server,
        Op("pullRequest(number: $number) { id }"),
        gql(json!({ "repository": { "pullRequest": { "id": "PR_node" } } })),
    )
    .await;
    mount(
        &server,
        Op("markFileAsViewed"),
        ResponseTemplate::new(200).set_body_json(json!({
            "data": null,
            "errors": [{ "type": "FORBIDDEN", "message": "no" }]
        })),
    )
    .await;
    let err = provider(&server)
        .set_file_viewed(&change(), "a.rs", true)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Forbidden { .. }), "{err:?}");
}
