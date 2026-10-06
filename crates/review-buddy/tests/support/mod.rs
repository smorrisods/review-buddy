//! A stub GitHub for the review-write tests: reads served from the `rb-github` fixtures for
//! acme/widgets#7, writes answered the way GitHub's GraphQL does, and a log of what was sent.
#![allow(dead_code)]

use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param_is_missing};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

pub fn fixture(name: &str) -> Value {
    let file = format!(
        "{}/../rb-github/tests/fixtures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

pub fn fixture_response(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(fixture(name))
}

/// Matches a GraphQL call whose query text contains `needle`.
pub struct Op(pub &'static str);

impl Match for Op {
    fn matches(&self, request: &Request) -> bool {
        query_of(request).contains(self.0)
    }
}

pub struct Body(pub &'static str);

impl Match for Body {
    fn matches(&self, request: &Request) -> bool {
        String::from_utf8_lossy(&request.body).contains(self.0)
    }
}

pub struct Var(pub &'static str, pub Value);

impl Match for Var {
    fn matches(&self, request: &Request) -> bool {
        serde_json::from_slice::<Value>(&request.body)
            .is_ok_and(|b| b["variables"].get(self.0).unwrap_or(&Value::Null) == &self.1)
    }
}

fn query_of(request: &Request) -> String {
    serde_json::from_slice::<Value>(&request.body)
        .ok()
        .and_then(|b| b["query"].as_str().map(str::to_string))
        .unwrap_or_default()
}

pub fn gql(data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "data": data }))
}

pub fn gql_error(kind: &str, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "data": null,
        "errors": [{ "type": kind, "message": message }]
    }))
}

fn graphql() -> wiremock::MockBuilder {
    Mock::given(method("POST")).and(path("/graphql"))
}

pub const FIND_PENDING: &str = "reviews(states: [PENDING]";
pub const CREATE_REVIEW: &str = "addPullRequestReview(";
pub const ADD_THREAD: &str = "addPullRequestReviewThread(";
pub const SUBMIT: &str = "submitPullRequestReview(";
pub const REPLY: &str = "addPullRequestReviewThreadReply(";

/// The write operations in the order they were sent, with their variables.
pub async fn writes(server: &MockServer) -> Vec<(&'static str, Value)> {
    let names = [
        (FIND_PENDING, "find pending"),
        (CREATE_REVIEW, "create review"),
        (ADD_THREAD, "add thread"),
        (SUBMIT, "submit"),
        (REPLY, "reply"),
    ];
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter_map(|r| {
            let query = query_of(r);
            let (_, name) = names.iter().find(|(needle, _)| query.contains(needle))?;
            let body: Value = serde_json::from_slice(&r.body).ok()?;
            Some((*name, body["variables"].clone()))
        })
        .collect()
}

pub async fn count_requests(server: &MockServer, needle: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| String::from_utf8_lossy(&r.body).contains(needle))
        .count()
}

/// A search answer whose one change is acme/widgets#7, the change the detail mocks describe.
pub fn widgets_search() -> Value {
    let mut body = fixture("review_requested_p2");
    let node = &mut body["data"]["search"]["nodes"][1];
    node["number"] = json!(7);
    node["title"] = json!("Widgets get springs");
    node["repository"]["nameWithOwner"] = json!("acme/widgets");
    body
}

/// Writes that succeed: no pending review yet, so one is created, then submitted.
pub async fn serve_happy_writes(server: &MockServer) {
    serve_write_chain(server, thread_added()).await;
}

pub fn thread_added() -> ResponseTemplate {
    gql(json!({"addPullRequestReviewThread": {"thread": {"id": "PRRT_new"}}}))
}

/// The write mocks with `thread` as the answer to adding a line comment. Mount before the
/// reads: the first matching mock wins, and the reads' catch-alls would otherwise take these.
pub async fn serve_write_chain(server: &MockServer, thread: ResponseTemplate) {
    graphql()
        .and(Op(FIND_PENDING))
        .respond_with(gql(
            json!({"repository": {"pullRequest": {"id": "PR_1", "reviews": {"nodes": []}}}}),
        ))
        .mount(server)
        .await;
    graphql()
        .and(Op(CREATE_REVIEW))
        .respond_with(gql(
            json!({"addPullRequestReview": {"pullRequestReview": {"id": "PRR_new"}}}),
        ))
        .mount(server)
        .await;
    graphql()
        .and(Op(ADD_THREAD))
        .respond_with(thread)
        .mount(server)
        .await;
    graphql()
        .and(Op(SUBMIT))
        .respond_with(gql(
            json!({"submitPullRequestReview": {"pullRequestReview": {"id": "PRR_new"}}}),
        ))
        .mount(server)
        .await;
}

pub async fn serve_searches(server: &MockServer) {
    graphql()
        .and(Body("\"q\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(widgets_search()))
        .mount(server)
        .await;
}

/// Details, checks, threads and files for acme/widgets#7.
pub async fn serve_reads(server: &MockServer) {
    graphql()
        .and(Body("mergeable"))
        .respond_with(fixture_response("detail"))
        .mount(server)
        .await;
    graphql()
        .and(Var("id", Value::from("PRT_1")))
        .respond_with(fixture_response("thread_comments_more"))
        .mount(server)
        .await;
    graphql()
        .and(Var("number", Value::from(7)))
        .and(Var("after", Value::Null))
        .respond_with(fixture_response("threads_p1"))
        .mount(server)
        .await;
    graphql()
        .and(Var("after", Value::from("t1")))
        .respond_with(fixture_response("threads_p2"))
        .mount(server)
        .await;
    serve_rest_reads(server, fixture("files_p2")).await;
}

/// The REST side of a change: comments, head, checks and the file list.
pub async fn serve_rest_reads(server: &MockServer, files: Value) {
    let get = |p: &str| Mock::given(method("GET")).and(path(p.to_string()));
    get("/repos/acme/widgets/issues/7/comments")
        .respond_with(fixture_response("issue_comments"))
        .mount(server)
        .await;
    get("/repos/acme/widgets/pulls/7")
        .respond_with(fixture_response("pull_head"))
        .mount(server)
        .await;
    get("/repos/acme/widgets/commits/abc123/check-runs")
        .and(query_param_is_missing("page"))
        .respond_with(fixture_response("check_runs_p2"))
        .mount(server)
        .await;
    get("/repos/acme/widgets/commits/abc123/status")
        .respond_with(fixture_response("commit_status"))
        .mount(server)
        .await;
    get("/repos/acme/widgets/pulls/7/files")
        .and(query_param_is_missing("page"))
        .respond_with(ResponseTemplate::new(200).set_body_json(files))
        .mount(server)
        .await;
}
