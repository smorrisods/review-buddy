//! The read commands against a stubbed GitLab, through the real binary and the live factory:
//! a subgroup project, a self-hosted instance served below a path, the `mr` alias, byte-exact
//! patches, pipeline jobs and the calm errors for a token that lacks a scope.
#![cfg(feature = "live")]

#[path = "support/cli.rs"]
mod sandbox;

use std::process::Command as Std;

use sandbox::Sandbox;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "glpat_stub_token_value";
const PROJECT: &str = "platform/infra/terraform";
const ENCODED: &str = "platform%2Finfra%2Fterraform";

fn fixture(name: &str) -> Value {
    let file = format!(
        "{}/../rb-gitlab/tests/fixtures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

fn ok(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(fixture(name))
}

fn detail(web: &str) -> ResponseTemplate {
    let mut value = fixture("mr_detail");
    value["iid"] = json!(12);
    value["web_url"] = json!(format!("{web}/{PROJECT}/-/merge_requests/12"));
    ResponseTemplate::new(200).set_body_json(value)
}

async fn serve(server: &MockServer, prefix: &str, web: &str) {
    let get = |p: String| Mock::given(method("GET")).and(path(format!("{prefix}{p}")));
    let mr = format!("/projects/{ENCODED}/merge_requests/12");
    get("/user".into())
        .respond_with(ok("user"))
        .mount(server)
        .await;
    get("/merge_requests".into())
        .respond_with(ok("reviewer_p2"))
        .mount(server)
        .await;
    get("/todos".into())
        .respond_with(ok("todos"))
        .mount(server)
        .await;
    get(mr.clone())
        .respond_with(detail(web))
        .mount(server)
        .await;
    for (tail, fixture) in [
        ("approvals", "approvals"),
        ("reviewers", "mr_reviewers"),
        ("commits", "commits"),
        ("diffs", "diffs_shapes"),
        ("discussions", "discussions"),
    ] {
        get(format!("{mr}/{tail}"))
            .respond_with(ok(fixture))
            .mount(server)
            .await;
    }
    get(format!("{mr}/draft_notes"))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;
    get(format!("/projects/{ENCODED}/pipelines/77/jobs"))
        .respond_with(ok("pipeline_jobs"))
        .mount(server)
        .await;
    get(format!("/projects/{ENCODED}/pipelines/77/bridges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;
}

struct Lab {
    sandbox: Sandbox,
    config: std::path::PathBuf,
}

impl Lab {
    fn new(host: &str, api_url: &str) -> Self {
        let sandbox = Sandbox::new();
        let config = sandbox.write_config(&format!(
            "[[source]]\nname = \"lab\"\nkind = \"gitlab\"\nhost = \"{host}\"\napi_url = \"{api_url}\"\nauth = \"env:RB_STUB_TOKEN\"\n"
        ));
        Self { sandbox, config }
    }

    fn run(&self, args: &[&str]) -> (String, String, i32) {
        let out = self
            .sandbox
            .cmd()
            .env("RB_STUB_TOKEN", TOKEN)
            .arg("--config")
            .arg(&self.config)
            .args(args)
            .output()
            .unwrap();
        let text = |b: Vec<u8>| String::from_utf8(b).unwrap().replace("\r\n", "\n");
        assert!(!text(out.stdout.clone()).contains(TOKEN));
        (
            text(out.stdout),
            text(out.stderr),
            out.status.code().unwrap(),
        )
    }
}

async fn plain_lab() -> (MockServer, Lab) {
    let server = MockServer::start().await;
    serve(&server, "", "https://gitlab.test").await;
    let lab = Lab::new("gitlab.test", &server.uri());
    (server, lab)
}

const SUB: [&str; 4] = ["-s", "lab", "-R", PROJECT];

fn with(rest: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = SUB.iter().map(|s| s.to_string()).collect();
    args.extend(rest.iter().map(|s| s.to_string()));
    args
}

fn run_with(lab: &Lab, rest: &[&str]) -> (String, String, i32) {
    let args = with(rest);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    lab.run(&args)
}

#[tokio::test(flavor = "multi_thread")]
async fn mr_view_shows_a_subgroup_merge_request_with_bang_refs() {
    let (_server, lab) = plain_lab().await;
    for bare in ["12", "!12", "#12"] {
        let (out, err, code) = run_with(&lab, &["mr", "view", bare]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("platform/infra/terraform!12 · open"), "{out}");
        assert!(
            out.contains("Merge     waiting on reviews or checks"),
            "{out}"
        );
        assert!(out.contains("sam (approved)"), "{out}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn comments_show_resolved_state_and_ranges() {
    let (_server, lab) = plain_lab().await;
    let (out, err, code) = run_with(&lab, &["mr", "view", "!12", "--comments"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Comments (4) · 1 unresolved"), "{out}");
    assert!(out.contains("a.rs:2-4 · resolved"), "{out}");
    assert!(out.contains("Why remove this?"), "{out}");
    assert!(out.contains("Conversation"), "{out}");
    let (json_out, _, _) = run_with(&lab, &["mr", "view", "!12", "--json", "comments"]);
    let value: Value = serde_json::from_str(&json_out).unwrap();
    let first = &value["comments"][0];
    assert_eq!(first["path"], "a.rs");
    assert_eq!(
        (first["startLine"].as_u64(), first["line"].as_u64()),
        (Some(2), Some(4))
    );
    assert_eq!(first["resolved"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn json_fields_match_the_other_forge() {
    let (_server, lab) = plain_lab().await;
    let (listed, _, code) = run_with(&lab, &["mr", "view", "!12", "--json"]);
    assert_eq!(code, 0);
    let fields: Vec<&str> = listed.lines().collect();
    assert!(fields.contains(&"headSha") && fields.contains(&"comments"));
    let (all, _, _) = run_with(&lab, &["mr", "view", "!12", "-q", "keys"]);
    let keys: Vec<String> = serde_json::from_str(&all).unwrap();
    for field in &fields {
        assert!(
            keys.iter().any(|k| k == field),
            "{field} missing from {keys:?}"
        );
    }
    let (one, _, _) = run_with(
        &lab,
        &["mr", "view", "!12", "--json", "forge,ref,isDraft,state"],
    );
    let value: Value = serde_json::from_str(&one).unwrap();
    assert_eq!(value["forge"], "gitlab");
    assert_eq!(value["ref"], "platform/infra/terraform!12");
    assert_eq!(value["isDraft"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn mr_list_prints_the_same_columns_as_pr_list() {
    let (_server, lab) = plain_lab().await;
    let (out, err, code) = lab.run(&["mr", "list", "-s", "lab"]);
    assert_eq!(code, 0, "{err}");
    let (again, _, _) = lab.run(&["pr", "list", "-s", "lab"]);
    assert_eq!(out, again);
    let first = out.lines().next().unwrap();
    assert_eq!(first.split('\t').count(), 7, "{first}");
    assert!(
        first.starts_with("lab\tplatform/flow!11\topen\t"),
        "{first}"
    );
    let (queue, _, code) = lab.run(&["queue", "-s", "lab"]);
    assert_eq!(code, 0);
    assert!(queue.contains("platform/flow!11"), "{queue}");
}

#[tokio::test(flavor = "multi_thread")]
async fn mr_checks_lists_jobs_and_treats_allowed_failures_as_neutral() {
    let (_server, lab) = plain_lab().await;
    let (out, err, code) = run_with(&lab, &["mr", "checks", "!12"]);
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(out.contains("pass\tbuild / build\t120\t"), "{out}");
    assert!(out.contains("neutral\ttest / lint\t30\t"), "{out}");
    assert!(out.contains("fail\ttest / e2e\t"), "{out}");
    assert!(err.contains("1 check failed."), "{err}");

    let (required, _, _) = run_with(&lab, &["mr", "checks", "!12", "--required"]);
    assert!(!required.contains("test / lint"), "{required}");
    assert!(required.contains("test / e2e"), "{required}");

    let (json_out, _, _) = run_with(
        &lab,
        &["mr", "checks", "!12", "--json", "name,state,required"],
    );
    let value: Value = serde_json::from_str(&json_out).unwrap();
    let lint = value
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "test / lint")
        .unwrap();
    assert_eq!(
        (lint["state"].as_str(), lint["required"].as_bool()),
        (Some("neutral"), Some(false))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mr_diff_rebuilds_a_patch_git_accepts() {
    let (_server, lab) = plain_lab().await;
    let (patch, err, code) = run_with(&lab, &["mr", "diff", "!12"]);
    assert_eq!(code, 0, "{err}");
    assert!(patch.starts_with(
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ "
    ));
    assert!(patch.contains("diff --git a/gone.txt b/gone.txt\ndeleted file mode 100644\n--- a/gone.txt\n+++ /dev/null\n"));
    assert!(patch.contains("diff --git a/added.txt b/added.txt\nnew file mode 100644\n--- /dev/null\n+++ b/added.txt\n"));
    assert!(patch.contains("diff --git a/old/name.txt b/new/name.txt\nsimilarity index 100%\nrename from old/name.txt\nrename to new/name.txt\ndiff --git"));
    assert!(patch.contains(
        "rename from docs/a.md\nrename to docs/b.md\n--- a/docs/a.md\n+++ b/docs/b.md\n"
    ));
    assert!(!patch.contains("logo.png"));
    assert!(err.contains("Skipped logo.png"), "{err}");

    let (names, _, _) = run_with(&lab, &["mr", "diff", "!12", "--name-only"]);
    assert!(
        names.contains("new/name.txt\n") && names.contains("logo.png\n"),
        "{names}"
    );

    if Std::new("git").arg("--version").output().is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        Std::new("git")
            .args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false"])
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q"]).status.success());
    for (name, text) in [
        ("src/lib.rs", "one\ntwo\nthree\n"),
        ("gone.txt", "bye\n"),
        ("old/name.txt", "same\n"),
        ("docs/a.md", "alpha\nbeta\n"),
    ] {
        let file = dir.path().join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    std::fs::write(dir.path().join("change.patch"), &patch).unwrap();
    let out = git(&["apply", "--check", "change.patch"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn url_selectors_resolve_without_repo_flags() {
    let (_server, lab) = plain_lab().await;
    for url in [
        "https://gitlab.test/platform/infra/terraform/-/merge_requests/12",
        "https://gitlab.test/platform/infra/terraform/-/merge_requests/12/diffs",
        "https://gitlab.test/platform/infra/terraform/merge_requests/12",
        "lab:platform/infra/terraform!12",
        "platform/infra/terraform!12",
    ] {
        let (out, err, code) = lab.run(&["mr", "view", url, "--json", "ref"]);
        assert_eq!(code, 0, "{url}: {err}");
        assert!(out.contains("platform/infra/terraform!12"), "{url}: {out}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn urls_on_an_instance_served_below_a_path_drop_the_root() {
    let server = MockServer::start().await;
    serve(&server, "/gitlab/api/v4", "https://gl.test/gitlab").await;
    let lab = Lab::new("gl.test", &format!("{}/gitlab/api/v4", server.uri()));
    let url = "https://gl.test/gitlab/platform/infra/terraform/-/merge_requests/12";
    let (out, err, code) = lab.run(&["mr", "view", url, "--json", "ref,url"]);
    assert_eq!(code, 0, "{err}");
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["ref"], "platform/infra/terraform!12");
    assert_eq!(value["url"], url);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_without_the_scope_is_an_auth_exit_with_a_next_step() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error": "insufficient_scope", "scope": "api read_api"})),
        )
        .mount(&server)
        .await;
    let lab = Lab::new("gitlab.test", &server.uri());
    let (out, err, code) = run_with(&lab, &["mr", "view", "!12"]);
    assert_eq!((code, out.as_str()), (4, ""), "{err}");
    assert!(err.contains("scope"), "{err}");
    assert!(err.contains("review-buddy auth status"), "{err}");
    let (_, _, code) = run_with(&lab, &["mr", "checks", "!12"]);
    assert_eq!(code, 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_token_asks_you_to_sign_in_again() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let lab = Lab::new("gitlab.test", &server.uri());
    let (_, err, code) = run_with(&lab, &["mr", "diff", "!12"]);
    assert_eq!(code, 4, "{err}");
    assert!(
        err.contains("review-buddy auth login --host gitlab.test"),
        "{err}"
    );
}
