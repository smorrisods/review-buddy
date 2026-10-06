//! `hide_repos` in the config: `queue` and `pr list` leave those projects out, a trailing `*`
//! matches a prefix, and `--repo` still reaches a hidden project. Against a stubbed GitLab.
#![cfg(feature = "live")]

#[path = "support/cli.rs"]
mod sandbox;

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
    fn new(host: &str, api_url: &str, extra: &str) -> Self {
        let sandbox = Sandbox::new();
        let config = sandbox.write_config(&format!(
            "[[source]]\nname = \"lab\"\nkind = \"gitlab\"\nhost = \"{host}\"\napi_url = \"{api_url}\"\nauth = \"env:RB_STUB_TOKEN\"\n{extra}"
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

async fn lab(extra: &str) -> (MockServer, Lab) {
    let server = MockServer::start().await;
    serve(&server, "", "https://gitlab.test").await;
    let lab = Lab::new("gitlab.test", &server.uri(), extra);
    (server, lab)
}

fn listed(lab: &Lab, args: &[&str]) -> Vec<String> {
    let (out, err, code) = lab.run(args);
    assert_eq!(code, 0, "{err}");
    out.lines().map(str::to_string).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_and_pr_list_leave_out_hidden_projects() {
    let (_server, plain) = lab("").await;
    let all = listed(&plain, &["pr", "list", "-s", "lab"]);
    assert!(all.iter().any(|r| r.contains("platform/flow!")), "{all:?}");

    let (_server, hidden) = lab("hide_repos = [\"platform/flow\"]\n").await;
    for args in [["pr", "list", "-s", "lab"], ["queue", "-s", "lab", "--all"]] {
        let rows = listed(&hidden, &args);
        assert!(
            !rows.iter().any(|r| r.contains("platform/flow!")),
            "{args:?} {rows:?}"
        );
    }
    let (out, _, _) = hidden.run(&["queue", "-s", "lab", "--json", "ref"]);
    assert!(!out.contains("platform/flow!"), "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trailing_star_hides_by_prefix() {
    let (_server, hidden) = lab("hide_repos = [\"PLATFORM/*\"]\n").await;
    let rows = listed(&hidden, &["pr", "list", "-s", "lab"]);
    assert!(!rows.iter().any(|r| r.contains("platform/")), "{rows:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn repo_overrides_hide_repos() {
    let (_server, hidden) = lab("hide_repos = [\"platform/flow\"]\n").await;
    for cmd in ["pr", "queue"] {
        let args: Vec<&str> = if cmd == "pr" {
            vec!["pr", "list", "-s", "lab", "-R", "platform/flow"]
        } else {
            vec!["queue", "-s", "lab", "-R", "platform/flow"]
        };
        let rows = listed(&hidden, &args);
        assert!(!rows.is_empty(), "{cmd}: -R reaches the hidden project");
        assert!(
            rows.iter().all(|r| r.contains("platform/flow!")),
            "{rows:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_hide_repos_entry_is_a_calm_config_error() {
    let (_server, bad) = lab("hide_repos = [\"platform/*/flow\"]\n").await;
    let (out, err, code) = bad.run(&["pr", "list", "-s", "lab"]);
    assert_ne!(code, 0, "{out}");
    assert!(
        err.contains("hide_repos") && err.contains("at the end"),
        "{err}"
    );
}
