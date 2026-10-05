use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{json, Value};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "ghp_integrationsecret987";

struct Home(tempfile::TempDir);

impl Home {
    fn new(config: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), config).unwrap();
        Self(dir)
    }

    fn rb(&self, args: &[&str]) -> Command {
        let root = self.0.path();
        let mut cmd = Command::cargo_bin("review-buddy").unwrap();
        cmd.args(args)
            .arg("--config")
            .arg(root.join("config.toml"))
            .env("HOME", root)
            .env("XDG_CONFIG_HOME", root.join("c"))
            .env("XDG_DATA_HOME", root.join("d"))
            .env("XDG_CACHE_HOME", root.join("k"))
            .env("XDG_STATE_HOME", root.join("s"))
            .env("NO_COLOR", "1")
            .env_remove("RB_TEST_TOKEN");
        cmd
    }
}

fn config(api_url: &str) -> String {
    format!(
        "[[source]]\nname = \"work\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{api_url}\"\nauth = \"env:RB_TEST_TOKEN\"\n\n\
         [[source]]\nname = \"gl\"\nkind = \"gitlab\"\nhost = \"gitlab.test\"\nauth = \"env:RB_TEST_TOKEN\"\n\n\
         [[source]]\nname = \"off\"\nkind = \"github\"\nhost = \"off.test\"\nenabled = false\nin_all = false\n"
    )
}

async fn mock_github() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-oauth-scopes", "repo, read:org")
                .set_body_json(json!({"login": "smorris", "name": null})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/rate_limit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": {"core": {"limit": 5000, "remaining": 4900, "reset": 1700000000}}
        })))
        .mount(&server)
        .await;
    server
}

fn run(mut cmd: Command) -> assert_cmd::assert::Assert {
    cmd.assert()
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_status_signs_in_and_never_prints_the_token() {
    let server = mock_github().await;
    let home = Home::new(&config(&server.uri()));
    let mut cmd = home.rb(&["auth", "status"]);
    cmd.env("RB_TEST_TOKEN", SECRET);
    let out = tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains(
        "✓ ghe.test (work)  signed in as smorris via env:RB_TEST_TOKEN · scopes repo, read:org"
    ));
    assert!(stdout.contains("gitlab.test (gl)  not checked yet"));
    assert!(!stdout.contains("off.test"));
    let all = format!("{stdout}{}", String::from_utf8_lossy(&out.stderr));
    assert!(!all.contains(SECRET));
}

#[tokio::test(flavor = "multi_thread")]
async fn doctor_and_json_output_never_contain_the_token() {
    let server = mock_github().await;
    let home = Home::new(&config(&server.uri()));
    for args in [
        &["doctor"][..],
        &["doctor", "--json"],
        &["auth", "status", "--json"],
    ] {
        let mut cmd = home.rb(args);
        cmd.env("RB_TEST_TOKEN", SECRET);
        let out = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert!(out.status.success(), "{args:?}");
        let all = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!all.contains(SECRET), "{args:?}");
        if args.contains(&"doctor") && !args.contains(&"--json") {
            for needle in [
                "Auth",
                "Rate limits",
                "API versions",
                "Paths",
                "core 4900/5000",
                "2022-11-28",
            ] {
                assert!(all.contains(needle), "{needle}\n{all}");
            }
        }
    }
}

#[test]
fn a_source_that_cannot_sign_in_exits_4_with_the_fix() {
    let home = Home::new(&config("http://127.0.0.1:9"));
    run(home.rb(&["auth", "status"]))
        .code(4)
        .stdout(predicate::str::contains(
            "✕ ghe.test (work)  couldn't read a token",
        ))
        .stdout(predicate::str::contains("· set RB_TEST_TOKEN to a token"))
        .stderr(predicate::str::contains("1 source can't sign in"))
        .stderr(predicate::str::contains("set RB_TEST_TOKEN"));
    run(home.rb(&["doctor"])).code(4);
}

#[test]
fn auth_status_json_lists_fields_and_rows() {
    let home = Home::new(&config("http://127.0.0.1:9"));
    run(home.rb(&["auth", "status", "--json", "state,host"]))
        .code(4)
        .stdout(predicate::str::contains("\"state\":\"failed\""));
    run(home.rb(&["auth", "status", "--json"])).stdout(predicate::str::contains("method"));
}

#[test]
fn source_list_reads_config_only_and_lists_every_source() {
    let home = Home::new(&config("http://127.0.0.1:9"));
    let out = home.rb(&["source", "list"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let rows: Vec<Vec<&str>> = text.lines().map(|l| l.split('\t').collect()).collect();
    assert_eq!(rows.len(), 3, "{text}");
    assert!(rows.iter().any(|r| r[0] == "off" && r[3] == "no"));
    assert!(rows
        .iter()
        .any(|r| r[0] == "work" && r[5] == "env:RB_TEST_TOKEN"));

    let out = home
        .rb(&["source", "list", "--json", "name,enabled"])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value.as_array().unwrap().len(), 3);
    assert_eq!(value[2]["enabled"], false);
}

#[test]
fn demo_reports_demo_sources_without_network_or_keyring() {
    let dir = tempfile::tempdir().unwrap();
    for args in [&["auth", "status"][..], &["doctor"]] {
        Command::cargo_bin("review-buddy")
            .unwrap()
            .args(["--demo", "--frozen-time", "2026-10-05T10:00"])
            .args(args)
            .env("HOME", dir.path())
            .env("NO_COLOR", "1")
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "signed in as smorris via demo (demo)",
            ));
    }
    Command::cargo_bin("review-buddy")
        .unwrap()
        .args(["--demo", "source", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("demo"));
}
