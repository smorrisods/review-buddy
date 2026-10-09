use rb_core::{
    CiState, Error, Etag, ForgeKind, Mergeability, MyReview, MyRole, OpenThreads, Provider,
    ReviewerState, Scope, SourceId,
};
use rb_github::{GithubClient, GithubProvider};
use rb_platform::Secret;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

fn fixture(name: &str) -> Value {
    let file = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

/// Matches a GraphQL call by a fragment of `variables.q` and the `after` cursor.
struct Search {
    term: &'static str,
    after: Option<&'static str>,
    scope: Option<&'static str>,
}

impl Match for Search {
    fn matches(&self, request: &Request) -> bool {
        let Ok(body) = serde_json::from_slice::<Value>(&request.body) else {
            return false;
        };
        let vars = &body["variables"];
        let q = vars["q"].as_str().unwrap_or_default();
        q.contains(self.term)
            && vars["after"].as_str() == self.after
            && self.scope.is_none_or(|s| q.ends_with(s))
    }
}

async fn serve(server: &MockServer, term: &'static str, after: Option<&'static str>, name: &str) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Search {
            term,
            after,
            scope: None,
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture(name)))
        .mount(server)
        .await;
}

async fn serve_all(server: &MockServer) {
    serve(server, "review-requested:@me", None, "review_requested_p1").await;
    serve(
        server,
        "review-requested:@me",
        Some("cursor-1"),
        "review_requested_p2",
    )
    .await;
    serve(server, "reviewed-by:@me", None, "reviewed_by").await;
    serve(server, "assignee:@me", None, "assignee").await;
    serve(server, "author:@me", None, "author").await;
    serve(server, "mentions:@me", None, "mentions").await;
}

fn provider(server: &MockServer, base_path: &str) -> GithubProvider {
    let client = GithubClient::new(
        "ghe.test",
        Some(&format!("{}{base_path}", server.uri())),
        Secret::new("ghp_token"),
    )
    .unwrap();
    GithubProvider::new(client).with_source_id(SourceId::new("work"))
}

fn org_scope() -> Scope {
    Scope {
        owners: vec!["acme".into()],
        ..Scope::default()
    }
}

#[tokio::test]
async fn merges_five_searches_paginates_and_dedupes() {
    let server = MockServer::start().await;
    serve_all(&server).await;
    let p = provider(&server, "");
    let page = p.list_changes(&org_scope(), None).await.unwrap();

    let numbers: Vec<u64> = page.items.iter().map(|c| c.id.number).collect();
    assert_eq!(numbers, vec![101, 102, 103, 104, 105]);
    assert!(!page.not_modified);
    assert!(page.etag.unwrap().as_str().starts_with("gql-"));
    // 6 calls: page 2 of review-requested plus one each for the other four.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 6);
    let q = requests[0].body_json::<Value>().unwrap()["variables"]["q"].clone();
    assert_eq!(
        q,
        "is:pr is:open archived:false review-requested:@me org:acme"
    );
}

#[tokio::test]
async fn maps_summary_fields_and_roles() {
    let server = MockServer::start().await;
    serve_all(&server).await;
    let p = provider(&server, "");
    let page = p.list_changes(&org_scope(), None).await.unwrap();
    let by = |n: u64| page.items.iter().find(|c| c.id.number == n).unwrap();

    let a = by(101);
    assert_eq!(a.id.source_id, SourceId::new("work"));
    assert_eq!(a.id.kind, ForgeKind::GitHub);
    assert_eq!(a.id.repo, "acme/web");
    assert_eq!(a.my_role, MyRole::Reviewing);
    assert_eq!(a.my_review, MyReview::None);
    assert_eq!(a.ci, CiState::Pass);
    assert_eq!((a.adds, a.dels, a.files), (1010, 101, 2));
    assert_eq!((a.branch.as_str(), a.base.as_str()), ("feat/101", "main"));
    assert_eq!(a.head_sha, "aaaaaaa");
    assert_eq!(a.labels, vec!["backend"]);
    assert!(a.i_commented);
    let states: Vec<_> = a
        .reviewers
        .iter()
        .map(|r| (r.login.as_str(), r.state))
        .collect();
    assert_eq!(
        states,
        vec![
            ("octo", ReviewerState::Requested),
            ("web-team", ReviewerState::Requested),
            ("tess", ReviewerState::Approved),
        ]
    );
    assert!(a.updated_at > a.created_at);

    let bot = by(102);
    assert!(bot.author_is_bot);
    assert_eq!(bot.ci, CiState::Running);

    let draft = by(103);
    assert!(draft.draft);
    assert_eq!(draft.my_role, MyRole::Authored);
    assert_eq!(draft.ci, CiState::None);
    assert!(draft.has_new_activity, "mira spoke last on my change");
    assert_eq!(draft.reviewers[0].state, ReviewerState::ChangesRequested);

    let d = by(104);
    assert_eq!(d.my_role, MyRole::Reviewing);
    assert_eq!(d.my_review, MyReview::Approved);
    assert_eq!(d.my_reviewed_sha.as_deref(), Some("d0d0d0d"));
    assert!(d.has_new_activity, "pushed since my review");
    assert_eq!(d.ci, CiState::Fail);

    let e = by(105);
    assert_eq!(e.my_role, MyRole::Assigned);
    assert_eq!(e.ci, CiState::Fail);
    assert!(!e.author_is_bot);
}

#[tokio::test]
async fn maps_review_state_comment_counts_and_open_threads() {
    let server = MockServer::start().await;
    serve_all(&server).await;
    let page = provider(&server, "")
        .list_changes(&org_scope(), None)
        .await
        .unwrap();
    let by = |n: u64| page.items.iter().find(|c| c.id.number == n).unwrap();

    // Someone asked for a team and an approver weighed in; you are asked too but are left out.
    let a = by(101).signals;
    assert_eq!((a.approvals, a.changes_requested, a.outstanding), (1, 0, 1));
    assert!(a.review_required);
    assert_eq!(a.comments, 7);
    assert_eq!(a.open_threads, OpenThreads::Count(2));

    let draft = by(103).signals;
    assert_eq!((draft.approvals, draft.changes_requested), (0, 1));
    assert!(!draft.review_required);
    assert_eq!(draft.comments, 5);
    assert_eq!(draft.open_threads, OpenThreads::Count(0));

    // Your own approval is `my_review`, not one of other people's approvals; no thread data reads
    // as unknown rather than as zero open.
    let mine = by(104).signals;
    assert_eq!(
        (mine.approvals, mine.changes_requested, mine.outstanding),
        (0, 0, 0)
    );
    assert_eq!(mine.open_threads, OpenThreads::Unknown);
    assert_eq!(mine.comments, 0);
}

#[tokio::test]
async fn the_list_query_asks_for_the_cluster_fields() {
    let server = MockServer::start().await;
    serve_all(&server).await;
    provider(&server, "")
        .list_changes(&org_scope(), None)
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    let query = requests[0].body_json::<Value>().unwrap()["query"]
        .as_str()
        .unwrap()
        .to_string();
    for field in [
        "reviewDecision",
        "totalCommentsCount",
        "reviewThreads(first: 50) { nodes { isResolved } }",
    ] {
        assert!(query.contains(field), "{field} is in the list query");
    }
}

#[tokio::test]
async fn matching_etag_reports_not_modified() {
    let server = MockServer::start().await;
    serve_all(&server).await;
    let p = provider(&server, "");
    let first = p.list_changes(&org_scope(), None).await.unwrap();
    let etag = first.etag.unwrap();
    let again = p
        .list_changes(&org_scope(), Some(etag.clone()))
        .await
        .unwrap();
    assert!(again.not_modified);
    assert!(again.items.is_empty());
    assert_eq!(again.etag, Some(etag));
    let other = p
        .list_changes(&org_scope(), Some(Etag::new("gql-0")))
        .await
        .unwrap();
    assert!(!other.not_modified);
    assert_eq!(other.items.len(), 5);
}

#[tokio::test]
async fn empty_results() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("empty")))
        .mount(&server)
        .await;
    let page = provider(&server, "")
        .list_changes(&Scope::everything(), None)
        .await
        .unwrap();
    assert!(page.items.is_empty());
    assert!(!page.not_modified);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 5);
    let q = requests[0].body_json::<Value>().unwrap()["variables"]["q"].clone();
    assert_eq!(q, "is:pr is:open archived:false review-requested:@me");
}

#[tokio::test]
async fn one_search_set_per_scope_qualifier() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("empty")))
        .mount(&server)
        .await;
    let scope = Scope {
        owners: vec!["acme".into()],
        repos: vec!["o/r".into()],
        user: true,
    };
    provider(&server, "")
        .list_changes(&scope, None)
        .await
        .unwrap();
    let qs: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            r.body_json::<Value>().unwrap()["variables"]["q"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(qs.len(), 15);
    for suffix in ["org:acme", "repo:o/r", "user:@me"] {
        assert_eq!(qs.iter().filter(|q| q.ends_with(suffix)).count(), 5);
    }
}

#[tokio::test]
async fn enterprise_base_url_uses_api_graphql() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("empty")))
        .expect(5)
        .mount(&server)
        .await;
    provider(&server, "/api/v3")
        .list_changes(&Scope::everything(), None)
        .await
        .unwrap();
}

#[tokio::test]
async fn sso_http_error_names_the_authorisation_url() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header(
                    "x-github-sso",
                    "required; url=https://ghe.test/orgs/acme/sso",
                )
                .set_body_json(
                    json!({"message": "Resource protected by organization SAML enforcement."}),
                ),
        )
        .mount(&server)
        .await;
    let err = provider(&server, "")
        .list_changes(&org_scope(), None)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::Forbidden { reason, .. } if reason.contains("https://ghe.test/orgs/acme/sso"))
    );
}

#[tokio::test]
async fn partial_sso_errors_without_data_become_forbidden() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("sso_partial")))
        .mount(&server)
        .await;
    let err = provider(&server, "")
        .list_changes(&org_scope(), None)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::Forbidden { reason, .. } if reason.contains("SAML") && reason.contains("authorise"))
    );
}

#[tokio::test]
async fn partial_errors_with_results_are_tolerated() {
    let server = MockServer::start().await;
    let mut body = fixture("mentions");
    body["errors"] = json!([{"type": "FORBIDDEN", "message": "SAML"}]);
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let page = provider(&server, "")
        .list_changes(&org_scope(), None)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2);
}

#[tokio::test]
async fn rate_limit_response_surfaces_reset_time() {
    let server = MockServer::start().await;
    let reset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 600;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", reset.to_string().as_str())
                .set_body_json(json!({"message": "API rate limit exceeded"})),
        )
        .mount(&server)
        .await;
    let err = provider(&server, "")
        .list_changes(&org_scope(), None)
        .await
        .unwrap_err();
    let Error::RateLimited {
        retry_after_secs: Some(secs),
        ..
    } = err
    else {
        panic!("expected a rate limit error, got {err:?}");
    };
    assert!((590..=600).contains(&secs));
}

#[tokio::test]
async fn low_budget_stops_further_searches() {
    let server = MockServer::start().await;
    let reset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 300;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ratelimit-limit", "5000")
                .insert_header("x-ratelimit-remaining", "5")
                .insert_header("x-ratelimit-reset", reset.to_string().as_str())
                .set_body_json(fixture("empty")),
        )
        .mount(&server)
        .await;
    let p = provider(&server, "");
    let err = p.list_changes(&org_scope(), None).await.unwrap_err();
    assert!(matches!(err, Error::RateLimited { retry_after_secs: Some(s), .. } if s > 200));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    // The next call is refused without touching the network.
    assert!(p.list_changes(&org_scope(), None).await.is_err());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn detail_maps_body_reviewers_and_mergeability() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("detail")))
        .mount(&server)
        .await;
    let p = provider(&server, "");
    let id = rb_core::ChangeId {
        source_id: SourceId::new("work"),
        kind: ForgeKind::GitHub,
        repo: "acme/web".into(),
        number: 101,
    };
    let d = p.change_detail(&id).await.unwrap();
    assert_eq!(d.summary.id, id);
    assert_eq!(d.summary.title, "Add retry to sync");
    assert!(d.body.starts_with("Retries the sync call"));
    assert_eq!(d.mergeable, Some(true));
    assert_eq!(d.mergeability, Mergeability::Clean);
    assert_eq!(d.commit_count, 4);
    assert_eq!(d.web_url.as_str(), "https://ghe.test/acme/web/pull/101");
    assert_eq!(d.summary.my_role, MyRole::Reviewing);
    assert_eq!(d.summary.reviewers.len(), 3);
    assert_eq!(d.summary.head_sha, "aaaaaaa");
    assert_eq!(d.summary.base_sha, "bbbbbbb");

    let sent = server.received_requests().await.unwrap()[0]
        .body_json::<Value>()
        .unwrap();
    assert_eq!(
        sent["variables"],
        json!({"owner": "acme", "name": "web", "number": 101})
    );
}

#[tokio::test]
async fn detail_of_a_missing_change_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("detail_missing")))
        .mount(&server)
        .await;
    let id = rb_core::ChangeId {
        source_id: SourceId::new("work"),
        kind: ForgeKind::GitHub,
        repo: "acme/gone".into(),
        number: 1,
    };
    let err = provider(&server, "").change_detail(&id).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(m) if m.contains("acme/gone#1")));
}

#[tokio::test]
async fn capabilities_are_full() {
    let server = MockServer::start().await;
    assert_eq!(
        provider(&server, "").capabilities(),
        rb_core::Capabilities::all()
    );
}
