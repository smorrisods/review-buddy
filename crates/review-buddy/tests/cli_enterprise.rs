//! GitHub Enterprise and self-hosted GitLab end to end: a path prefix, a relative URL root,
//! ports, trailing slashes and plain http, all against wiremock stubs.
#![cfg(feature = "live")]

#[path = "support/cli.rs"]
mod sandbox;

use sandbox::Sandbox;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "ghp_enterprisesecret321";
const OLD_DATE: &str = "Tue, 14 Nov 2023 22:13:20 GMT";

fn empty_graphql() -> Value {
    let file = format!(
        "{}/../rb-github/tests/fixtures/empty.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

async fn ghe(server: &MockServer, date: Option<&str>) {
    Mock::given(path("/ghe/api/v3/user"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-oauth-scopes", "repo, read:org")
                .set_body_json(json!({"login": "smorris", "name": null})),
        )
        .mount(server)
        .await;
    Mock::given(path("/ghe/api/v3/rate_limit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": {
                "core": {"limit": 5000, "remaining": 4900, "reset": 1700000000},
                "graphql": {"limit": 5000, "remaining": 4999, "reset": 1700000000}
            }
        })))
        .mount(server)
        .await;
    let mut meta = ResponseTemplate::new(200)
        .insert_header("x-github-enterprise-version", "3.12.4")
        .set_body_json(json!({}));
    if let Some(date) = date {
        meta = meta.insert_header("date", date);
    }
    Mock::given(path("/ghe/api/v3/meta"))
        .respond_with(meta)
        .mount(server)
        .await;
}

async fn gitlab(server: &MockServer) {
    Mock::given(path("/gitlab/api/v4/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"username": "gluser"})))
        .mount(server)
        .await;
    Mock::given(path("/gitlab/api/v4/personal_access_tokens/self"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"scopes": ["api", "read_user"], "expires_at": "2027-01-12"})),
        )
        .mount(server)
        .await;
    Mock::given(path("/gitlab/api/v4/version"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"version": "16.11.2-ee", "revision": "abc123def"})),
        )
        .mount(server)
        .await;
}

fn config(gh: &MockServer, gl: &MockServer) -> String {
    format!(
        "[[source]]\nname = \"work\"\nkind = \"github\"\nhost = \"ghe.corp.test:8443\"\napi_url = \"{}/ghe/api/v3/\"\nauth = \"env:RB_TEST_TOKEN\"\n\n\
         [[source]]\nname = \"lab\"\nkind = \"gitlab\"\nhost = \"git.corp.test\"\napi_url = \"{}/gitlab/api/v4/\"\nauth = \"env:RB_TEST_TOKEN\"\n",
        gh.uri(),
        gl.uri()
    )
}

async fn run(sandbox: &Sandbox, config: &str, args: &[&str]) -> (String, String, i32) {
    let file = sandbox.write_config(config);
    let mut cmd = sandbox.cmd();
    cmd.arg("--config")
        .arg(file)
        .args(args)
        .env("RB_TEST_TOKEN", SECRET);
    let out = tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn doctor_reports_versions_endpoints_and_clock_for_both_forges() {
    let (gh, gl) = (MockServer::start().await, MockServer::start().await);
    ghe(&gh, Some(OLD_DATE)).await;
    gitlab(&gl).await;
    let sandbox = Sandbox::new();
    let (out, err, code) = run(&sandbox, &config(&gh, &gl), &["doctor"]).await;
    assert_eq!(code, 0, "{out}{err}");
    assert!(!format!("{out}{err}").contains(SECRET));
    for needle in [
        "GitHub Enterprise Server 3.12.4 · REST 2022-11-28",
        "GitLab 16.11.2-ee (abc123def) · REST v4",
        &format!(
            "REST {0}/ghe/api/v3 · GraphQL {0}/ghe/api/graphql · web {0}/ghe",
            gh.uri()
        ),
        &format!("REST {0}/gitlab/api/v4 · web {0}/gitlab", gl.uri()),
        "core 4900/5000",
        "graphql 4999/5000",
        "the server's clock is",
        "behind this machine",
        "in step with this machine",
        "Paths",
    ] {
        assert!(out.contains(needle), "{needle}\n{out}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn doctor_json_carries_versions_endpoints_and_skew() {
    let (gh, gl) = (MockServer::start().await, MockServer::start().await);
    ghe(&gh, Some(OLD_DATE)).await;
    gitlab(&gl).await;
    let sandbox = Sandbox::new();
    let (out, _, code) = run(
        &sandbox,
        &config(&gh, &gl),
        &["doctor", "--json", "apiVersions,endpoints,clock,paths"],
    )
    .await;
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    let api = v["apiVersions"].as_array().unwrap();
    assert_eq!(api[0]["server"], "3.12.4");
    assert_eq!(api[0]["product"], "GitHub Enterprise Server");
    assert_eq!(api[1]["server"], "16.11.2-ee");
    assert_eq!(api[1]["revision"], "abc123def");
    let endpoints = v["endpoints"].as_array().unwrap();
    assert_eq!(
        endpoints[0]["graphql"],
        format!("{}/ghe/api/graphql", gh.uri())
    );
    assert_eq!(endpoints[1]["web"], format!("{}/gitlab", gl.uri()));
    let clock = v["clock"].as_array().unwrap();
    assert_eq!(clock[0]["warning"], true);
    assert_eq!(clock[1]["warning"], false);
    assert!(v["paths"]["configFiles"].is_array());
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_reads_through_the_prefix_and_relative_root() {
    let (gh, gl) = (MockServer::start().await, MockServer::start().await);
    Mock::given(path("/ghe/api/v3/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"login": "smorris"})))
        .mount(&gh)
        .await;
    Mock::given(method("POST"))
        .and(path("/ghe/api/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(empty_graphql()))
        .expect(5)
        .mount(&gh)
        .await;
    gitlab(&gl).await;
    Mock::given(path("/gitlab/api/v4/merge_requests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(3)
        .mount(&gl)
        .await;
    Mock::given(path("/gitlab/api/v4/todos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&gl)
        .await;
    let sandbox = Sandbox::new();
    let (out, err, code) = run(&sandbox, &config(&gh, &gl), &["queue", "--json", "ref"]).await;
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_hangs_up_on_tls_gets_a_calm_next_step() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(4) {
            drop(stream);
        }
    });
    let config = format!(
        "[[source]]\nname = \"work\"\nkind = \"github\"\nhost = \"ghe.corp.test\"\napi_url = \"https://127.0.0.1:{port}/api/v3\"\nauth = \"env:RB_TEST_TOKEN\"\n"
    );
    let sandbox = Sandbox::new();
    let (out, err, code) = run(&sandbox, &config, &["doctor"]).await;
    assert_eq!(code, 4, "{out}{err}");
    assert!(out.contains("couldn't reach ghe.corp.test"), "{out}");
    assert!(
        out.contains("check the host name, `api_url` and your VPN")
            || out.contains("publicly trusted certificate"),
        "{out}"
    );
}
