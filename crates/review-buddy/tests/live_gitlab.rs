//! A GitLab source end to end: the live factory builds a `GitlabProvider` pointed at a stub
//! server, and both the queue and the `pr` commands read from it.
#![cfg(feature = "live")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use assert_cmd::Command;
use rb_core::{ForgeKind, MyRole, SourceId};
use rb_platform::{CommandOutput, CommandRunner, MemorySecretStore, PlatformError};
use review_buddy::app::{update, App, AppConfig, Cmd, Msg};
use review_buddy::config::Config;
use review_buddy::providers::{Deps, Factory, Live};
use review_buddy::runtime::{execute, Backend, Platform};
use serde_json::Value;
use tokio::sync::mpsc;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "glpat_stub_token_value";

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

fn json(name: &str) -> ResponseTemplate {
    let file = format!(
        "{}/../rb-gitlab/tests/fixtures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let value: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    ResponseTemplate::new(200).set_body_json(value)
}

async fn serve(server: &MockServer) {
    let get = |p: &str| Mock::given(method("GET")).and(path(p.to_string()));
    get("/user").respond_with(json("user")).mount(server).await;
    for (param, fixture) in [
        ("reviewer_username", "reviewer_p2"),
        ("assignee_username", "assignee"),
        ("author_username", "author"),
    ] {
        get("/merge_requests")
            .and(query_param(param, "octo"))
            .respond_with(json(fixture))
            .mount(server)
            .await;
    }
    get("/todos")
        .respond_with(json("todos"))
        .mount(server)
        .await;
    let mr = "/projects/platform%2Fflow/merge_requests/11";
    get(mr).respond_with(json("mr_detail")).mount(server).await;
    get(&format!("{mr}/approvals"))
        .respond_with(json("approvals"))
        .mount(server)
        .await;
    get(&format!("{mr}/reviewers"))
        .respond_with(json("mr_reviewers"))
        .mount(server)
        .await;
    get(&format!("{mr}/commits"))
        .respond_with(json("commits"))
        .mount(server)
        .await;
    get(&format!("{mr}/diffs"))
        .respond_with(json("diffs"))
        .mount(server)
        .await;
}

fn config_for(server: &MockServer) -> Config {
    toml::from_str(&format!(
        r#"
[[source]]
name = "lab"
kind = "gitlab"
host = "gitlab.test"
api_url = "{}"
auth = "env:RB_STUB_TOKEN"
"#,
        server.uri()
    ))
    .unwrap()
}

#[tokio::test]
async fn a_gitlab_source_loads_into_the_queue() {
    let server = MockServer::start().await;
    serve(&server).await;
    let env: HashMap<String, String> = [("RB_STUB_TOKEN".to_string(), TOKEN.to_string())].into();
    let deps = Deps {
        runner: Arc::new(NoCli),
        store: Arc::new(MemorySecretStore::new()),
        getenv: Arc::new(move |k| env.get(k).cloned()),
    };
    let factory = Arc::new(Factory::from_config(&config_for(&server), deps));
    let sources = factory.sources();
    let live = Arc::new(Live::new(factory, sources, None, 4));
    let backend = Backend::Live(live.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: rb_theme::ColourDepth::TrueColour,
        no_color: false,
        size: (160, 40),
    });

    app.state.loading = true;
    let cmds = update(&mut app, Msg::Cached(Box::new(live.cached_snapshot())));
    for cmd in cmds.into_iter().filter(|c| matches!(c, Cmd::LoadChanges)) {
        execute(cmd, &tx, &backend, &Platform::system());
    }
    let msg = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let msg = rx.recv().await.expect("channel open");
            if matches!(msg, Msg::SourceLoaded { .. }) {
                return msg;
            }
        }
    })
    .await
    .expect("source loads");
    update(&mut app, msg);

    assert_eq!(app.state.pending_sources, 0);
    assert!(app.state.failures.is_empty(), "{:?}", app.state.failures);
    let refs: Vec<String> = app.state.changes.iter().map(|c| c.id.short_ref()).collect();
    assert_eq!(
        refs,
        [
            "platform/flow!11",
            "platform/sub/api!12",
            "platform/flow!13",
            "octo/dotfiles!14",
            "other/thing!15"
        ]
    );
    let first = &app.state.changes[0];
    assert_eq!(first.id.kind, ForgeKind::GitLab);
    assert_eq!(first.id.source_id, SourceId::new("lab"));
    assert_eq!(first.my_role, MyRole::Reviewing);
    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| r
        .headers
        .get("private-token")
        .is_some_and(|v| v.to_str().unwrap() == TOKEN)));
}

fn run(server: &MockServer, home: &std::path::Path, args: &[&str]) -> std::process::Output {
    let config = home.join("config.toml");
    std::fs::write(
        &config,
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
        .arg(&config)
        .args(args);
    cmd.output().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn pr_list_and_view_read_merge_requests_from_a_stubbed_gitlab() {
    let server = MockServer::start().await;
    serve(&server).await;
    let home = tempfile::tempdir().unwrap();

    let out = run(&server, home.path(), &["pr", "list", "--source", "lab"]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("platform/flow!11"), "{text}");
    assert!(text.contains("Add retry to sync"), "{text}");
    assert!(!text.contains(TOKEN));

    let out = run(
        &server,
        home.path(),
        &[
            "pr",
            "view",
            "11",
            "-s",
            "lab",
            "-R",
            "platform/flow",
            "--json",
            "number,ref,url,title",
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["number"], 11);
    assert_eq!(value["ref"], "platform/flow!11");
    assert_eq!(value["title"], "Add retry to sync");
}
