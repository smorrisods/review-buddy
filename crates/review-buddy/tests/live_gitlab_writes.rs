//! A GitLab review end to end against a stub server: the real `Live` factory, `runtime::execute`
//! and the same `update` loop the interface runs, plus the `pr` read commands. Nothing here
//! reaches a real forge.
#![cfg(feature = "live")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use assert_cmd::Command;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_core::{ChangeId, ForgeKind, SourceId};
use rb_platform::{CommandOutput, CommandRunner, MemorySecretStore, PlatformError};
use review_buddy::app::{update, App, AppConfig, Cmd, Msg, Screen};
use review_buddy::config::Config;
use review_buddy::providers::{Deps, Factory, Live};
use review_buddy::runtime::{execute, Backend, Platform};
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "glpat_stub_token_value";
const MR: &str = "/projects/platform%2Fflow/merge_requests/11";
const HEAD: &str = "cccccccccccccccccccccccccccccccccccccccc";
const PATCH: &str = "@@ -1,3 +1,4 @@\n ctx\n-old\n+new\n+newer\n ctx\n";

struct NoCli;

impl CommandRunner for NoCli {
    fn run(
        &self,
        program: &str,
        _: &[&str],
        _: Option<&[u8]>,
    ) -> Result<CommandOutput, PlatformError> {
        Err(PlatformError::CommandFailed {
            program: program.into(),
        })
    }
}

fn config(api_url: &str) -> Config {
    toml::from_str(&format!(
        r#"
[[source]]
name = "lab"
kind = "gitlab"
host = "gitlab.test"
api_url = "{api_url}"
auth = "env:RB_STUB_TOKEN"
"#
    ))
    .unwrap()
}

fn live_for(api_url: &str) -> Arc<Live> {
    let env: HashMap<String, String> = [("RB_STUB_TOKEN".to_string(), TOKEN.to_string())].into();
    let deps = Deps {
        runner: Arc::new(NoCli),
        store: Arc::new(MemorySecretStore::new()),
        getenv: Arc::new(move |k| env.get(k).cloned()),
    };
    let factory = Arc::new(Factory::from_config(&config(api_url), deps));
    let sources = factory.sources();
    Arc::new(Live::new(factory, sources, None, 4))
}

fn change_id() -> ChangeId {
    ChangeId {
        source_id: SourceId::new("lab"),
        kind: ForgeKind::GitLab,
        repo: "platform/flow".into(),
        number: 11,
    }
}

fn ok(status: u16, body: Value) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(body)
}

async fn serve(server: &MockServer) {
    let on = |verb: &str, p: &str, response: ResponseTemplate| {
        Mock::given(method(verb))
            .and(path(p.to_string()))
            .respond_with(response)
    };
    on(
        "GET",
        "/user",
        ok(200, json!({"username": "octo", "name": "Octo"})),
    )
    .mount(server)
    .await;
    let detail = format!(
        "{}/../rb-gitlab/tests/fixtures/mr_detail.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let detail: Value = serde_json::from_str(&std::fs::read_to_string(detail).unwrap()).unwrap();
    on("GET", MR, ok(200, detail)).mount(server).await;
    on(
        "GET",
        &format!("{MR}/diffs"),
        ok(
            200,
            json!([{"old_path": "src/a.rs", "new_path": "src/a.rs", "diff": PATCH}]),
        ),
    )
    .mount(server)
    .await;
    on(
        "GET",
        &format!("{MR}/discussions"),
        ok(
            200,
            json!([{"id": "d1", "individual_note": true, "notes": [
                {"id": 1, "body": "Overall fine", "author": {"username": "bo"},
                 "created_at": "2026-02-01T10:00:00.000Z", "system": false}]}]),
        ),
    )
    .mount(server)
    .await;
    on("GET", &format!("{MR}/draft_notes"), ok(200, json!([])))
        .mount(server)
        .await;
    on(
        "GET",
        &format!("{MR}/approvals"),
        ok(200, json!({"approved_by": []})),
    )
    .mount(server)
    .await;
    on(
        "POST",
        &format!("{MR}/draft_notes"),
        ok(201, json!({"id": 5})),
    )
    .mount(server)
    .await;
    on(
        "POST",
        &format!("{MR}/draft_notes/bulk_publish"),
        ResponseTemplate::new(204),
    )
    .mount(server)
    .await;
    on("POST", &format!("{MR}/approve"), ok(201, json!({})))
        .mount(server)
        .await;
    on(
        "GET",
        "/projects/platform%2Fflow/pipelines/77/jobs",
        ok(
            200,
            json!([{"name": "unit", "stage": "test", "status": "success", "allow_failure": false,
                    "web_url": "https://gitlab.test/j/1"}]),
        ),
    )
    .mount(server)
    .await;
    on(
        "GET",
        "/projects/platform%2Fflow/pipelines/77/bridges",
        ok(200, json!([])),
    )
    .mount(server)
    .await;
}

async fn writes(server: &MockServer) -> Vec<(String, Value)> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() != "GET")
        .map(|r| {
            (
                r.url.path().to_string(),
                serde_json::from_slice(&r.body).unwrap_or(Value::Null),
            )
        })
        .collect()
}

struct Session {
    app: App,
    backend: Backend,
    tx: UnboundedSender<Msg>,
    rx: UnboundedReceiver<Msg>,
}

impl Session {
    fn press(&mut self, code: KeyCode) -> Vec<Cmd> {
        update(
            &mut self.app,
            Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)),
        )
    }

    async fn settle(&mut self, cmds: Vec<Cmd>, want: fn(&Msg) -> bool) -> Vec<Cmd> {
        for cmd in cmds {
            execute(cmd, &self.tx, &self.backend, &Platform::system());
        }
        let msg = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let msg = self.rx.recv().await.expect("channel open");
                if want(&msg) {
                    return msg;
                }
            }
        })
        .await
        .expect("message arrives");
        update(&mut self.app, msg)
    }

    fn toast(&self) -> String {
        self.app.toasts.last().expect("a toast").notice.text.clone()
    }

    fn draft_len(&self) -> usize {
        self.app
            .diff
            .as_ref()
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .draft
            .comments
            .len()
    }
}

async fn open_diff(server: &MockServer) -> Session {
    let backend = Backend::Live(live_for(&server.uri()));
    let (tx, rx) = mpsc::unbounded_channel();
    let app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: rb_theme::ColourDepth::TrueColour,
        no_color: false,
        size: (160, 40),
    });
    let mut s = Session {
        app,
        backend,
        tx,
        rx,
    };
    let cmds = review_buddy::app::open_change(&mut s.app, &change_id());
    s.settle(cmds, |m| matches!(m, Msg::DiffLoaded { .. }))
        .await;
    assert_eq!(s.app.screen, Screen::Diff);
    assert!(
        s.app.diff.as_ref().unwrap().data.is_some(),
        "the diff loaded"
    );
    s
}

#[tokio::test]
async fn a_comment_then_approve_goes_through_gitlab() {
    let server = MockServer::start().await;
    serve(&server).await;
    let mut s = open_diff(&server).await;

    s.press(KeyCode::Char('c'));
    assert!(
        s.app.diff.as_ref().unwrap().composer.is_some(),
        "the composer opened"
    );
    for c in "Looks right to me".chars() {
        s.press(KeyCode::Char(c));
    }
    s.press(KeyCode::Enter);
    assert_eq!(s.draft_len(), 1);
    assert!(
        writes(&server).await.is_empty(),
        "nothing is sent while drafting"
    );

    s.press(KeyCode::Char('a'));
    assert!(
        s.app.diff.as_ref().unwrap().confirm.is_some(),
        "the preview comes first"
    );
    let cmds = s.press(KeyCode::Enter);
    assert!(matches!(cmds.as_slice(), [Cmd::SubmitReview { .. }]));
    s.settle(cmds, |m| matches!(m, Msg::ReviewSubmitted { .. }))
        .await;
    assert_eq!(s.toast(), "Approved");
    assert_eq!(s.draft_len(), 0);

    let sent = writes(&server).await;
    let paths: Vec<_> = sent
        .iter()
        .map(|w| w.0.rsplit('/').next().unwrap().to_string())
        .collect();
    assert_eq!(paths, ["draft_notes", "bulk_publish", "approve"]);
    assert_eq!(sent[0].1["note"], "Looks right to me");
    let position = &sent[0].1["position"];
    assert_eq!(position["new_path"], "src/a.rs");
    assert_eq!(position["head_sha"], HEAD);
    assert!(position["new_line"].is_u64() || position["old_line"].is_u64());
    assert_eq!(sent[2].1, json!({"sha": HEAD}));
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| r.headers.get("private-token").is_some_and(|v| v == TOKEN)));
}

#[tokio::test]
async fn a_forbidden_token_keeps_the_draft_and_names_the_scope() {
    let server = MockServer::start().await;
    serve(&server).await;
    Mock::given(method("POST"))
        .and(path(format!("{MR}/draft_notes")))
        .respond_with(ok(403, json!({"message": "403 Forbidden"})))
        .with_priority(1)
        .mount(&server)
        .await;
    let mut s = open_diff(&server).await;
    s.press(KeyCode::Char('c'));
    for c in "Keep me".chars() {
        s.press(KeyCode::Char(c));
    }
    s.press(KeyCode::Enter);
    s.press(KeyCode::Char('a'));
    let cmds = s.press(KeyCode::Enter);
    s.settle(cmds, |m| matches!(m, Msg::ReviewSubmitted { .. }))
        .await;
    assert_eq!(s.draft_len(), 1, "the draft is kept");
    let toast = s.toast();
    assert!(toast.contains("`api` scope"), "{toast}");
}

fn run(server: &MockServer, home: &std::path::Path, args: &[&str]) -> std::process::Output {
    let file = home.join("config.toml");
    std::fs::write(
        &file,
        format!(
            "[[source]]\nname = \"lab\"\nkind = \"gitlab\"\nhost = \"gitlab.test\"\napi_url = \"{}\"\nauth = \"env:RB_STUB_TOKEN\"\n",
            server.uri()
        ),
    )
    .unwrap();
    let mut cmd = Command::cargo_bin("review-buddy").unwrap();
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("USERPROFILE", home)
        .envs(std::env::var_os("SystemRoot").map(|v| ("SystemRoot", v)))
        .env("XDG_CONFIG_HOME", home.join("xdg-config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("RB_STUB_TOKEN", TOKEN)
        .arg("--config")
        .arg(&file)
        .args(["-s", "lab", "-R", "platform/flow", "pr"])
        .args(args);
    cmd.output().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn pr_diff_checks_and_comments_read_from_gitlab() {
    let server = MockServer::start().await;
    serve(&server).await;
    let home = tempfile::tempdir().unwrap();

    let out = run(&server, home.path(), &["diff", "11"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("src/a.rs") && text.contains("+newer"),
        "{text}"
    );

    let out = run(&server, home.path(), &["checks", "11"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("pass\ttest / unit"), "{text}");

    let out = run(&server, home.path(), &["view", "11", "--comments"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("Overall fine"), "{text}");
}
