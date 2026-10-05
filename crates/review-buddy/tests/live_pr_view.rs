//! `pr view` against a stubbed GitHub, through the real binary and the live provider factory.
#![cfg(feature = "live")]

use assert_cmd::Command;
use serde_json::Value;
use wiremock::matchers::{method, path, query_param_is_missing};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

const TOKEN: &str = "ghp_stub_token_value";

fn fixture(name: &str) -> Value {
    let file = format!(
        "{}/../rb-github/tests/fixtures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

fn json(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(fixture(name))
}

struct Body(&'static str);

impl Match for Body {
    fn matches(&self, request: &Request) -> bool {
        String::from_utf8_lossy(&request.body).contains(self.0)
    }
}

struct Var(&'static str, Value);

impl Match for Var {
    fn matches(&self, request: &Request) -> bool {
        serde_json::from_slice::<Value>(&request.body)
            .is_ok_and(|b| b["variables"].get(self.0).unwrap_or(&Value::Null) == &self.1)
    }
}

async fn serve(server: &MockServer) {
    let gql = || Mock::given(method("POST")).and(path("/graphql"));
    gql()
        .and(Body("mergeable"))
        .respond_with(json("detail"))
        .mount(server)
        .await;
    gql()
        .and(Var("id", Value::from("PRT_1")))
        .respond_with(json("thread_comments_more"))
        .mount(server)
        .await;
    gql()
        .and(Var("number", Value::from(7)))
        .and(Var("after", Value::Null))
        .respond_with(json("threads_p1"))
        .mount(server)
        .await;
    gql()
        .and(Var("after", Value::from("t1")))
        .respond_with(json("threads_p2"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/issues/7/comments"))
        .respond_with(json("issue_comments"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/7"))
        .respond_with(json("pull_head"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/commits/abc123/check-runs"))
        .and(query_param_is_missing("page"))
        .respond_with(json("check_runs_p2"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/commits/abc123/status"))
        .respond_with(json("commit_status"))
        .mount(server)
        .await;
}

fn run(server: &MockServer, home: &std::path::Path, args: &[&str]) -> std::process::Output {
    let config = home.join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[[source]]\nname = \"work\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{}\"\nauth = \"env:RB_STUB_TOKEN\"\n",
            server.uri()
        ),
    )
    .unwrap();
    let mut cmd = Command::cargo_bin("review-buddy").unwrap();
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("xdg-config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("RB_STUB_TOKEN", TOKEN)
        .arg("--config")
        .arg(&config)
        .args(["-R", "acme/widgets"])
        .args(args);
    cmd.output().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn pr_view_reads_a_change_from_a_stubbed_github() {
    let server = MockServer::start().await;
    serve(&server).await;
    let home = tempfile::tempdir().unwrap();
    let out = run(&server, home.path(), &["pr", "view", "7", "--comments"]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("acme/widgets#7"), "{text}");
    assert!(text.contains("wants"), "{text}");
    assert!(text.contains("Checks"), "{text}");
    assert!(text.contains("Comments ("), "{text}");
    assert!(!text.contains(TOKEN));
}

#[tokio::test(flavor = "multi_thread")]
async fn cheap_json_fields_skip_the_check_and_thread_requests() {
    let server = MockServer::start().await;
    serve(&server).await;
    let home = tempfile::tempdir().unwrap();
    let out = run(
        &server,
        home.path(),
        &["pr", "view", "7", "--json", "number,ref,url"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["number"], 7);
    assert_eq!(value["ref"], "acme/widgets#7");

    let paths: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect();
    assert!(
        !paths
            .iter()
            .any(|p| p.contains("check-runs") || p.contains("/status")),
        "{paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.contains("issues/7/comments")),
        "{paths:?}"
    );
}
