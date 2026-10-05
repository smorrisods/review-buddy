//! The live path end to end: a `GithubProvider` built through the factory, pointed at a stub
//! server, feeding the same `update` the interface uses.
#![cfg(feature = "live")]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ratatui::{backend::TestBackend, Terminal};
use rb_core::{
    ChangeId, ChangeState, ChangeSummary, CiState, ForgeKind, MyReview, MyRole, SourceId, Timestamp,
};
use rb_platform::{CommandOutput, CommandRunner, MemorySecretStore, PlatformError};
use rb_store::Store;
use review_buddy::app::{update, App, AppConfig, Cmd, FailureKind, Msg};
use review_buddy::config::Config;
use review_buddy::providers::{Deps, Factory, Live};
use review_buddy::runtime::{execute, Backend, Platform};
use review_buddy::ui::{self, HitMap};
use serde_json::Value;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use wiremock::matchers::{method, path, query_param_is_missing};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

const TOKEN: &str = "ghp_stub_token_value";

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

async fn serve_searches(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Body("\"q\""))
        .respond_with(json("review_requested_p2"))
        .mount(server)
        .await;
}

async fn serve_details(server: &MockServer) {
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
    let runs = "/repos/acme/widgets/commits/abc123/check-runs";
    Mock::given(method("GET"))
        .and(path(runs))
        .and(query_param_is_missing("page"))
        .respond_with(json("check_runs_p2"))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/commits/abc123/status"))
        .respond_with(json("commit_status"))
        .mount(server)
        .await;
    let files = "/repos/acme/widgets/pulls/7/files";
    Mock::given(method("GET"))
        .and(path(files))
        .and(query_param_is_missing("page"))
        .respond_with(json("files_p2"))
        .mount(server)
        .await;
}

fn config_for(server: &MockServer, auth: &str) -> Config {
    toml::from_str(&format!(
        r#"
[[source]]
name = "work"
kind = "github"
host = "ghe.test"
api_url = "{}"
auth = "{auth}"
"#,
        server.uri()
    ))
    .unwrap()
}

fn deps(token: Option<&str>) -> Deps {
    let env: HashMap<String, String> = token
        .map(|t| ("RB_STUB_TOKEN".to_string(), t.to_string()))
        .into_iter()
        .collect();
    Deps {
        runner: Arc::new(NoCli),
        store: Arc::new(MemorySecretStore::new()),
        getenv: Arc::new(move |k| env.get(k).cloned()),
    }
}

fn live_from(config: &Config, token: Option<&str>, cache: Option<Store>) -> Arc<Live> {
    let factory = Arc::new(Factory::from_config(config, deps(token)));
    let sources = factory.sources();
    Arc::new(Live::new(factory, sources, cache, 4))
}

fn change(number: u64, repo: &str, title: &str) -> ChangeSummary {
    ChangeSummary {
        id: ChangeId {
            source_id: SourceId::new("work"),
            kind: ForgeKind::GitHub,
            repo: repo.into(),
            number,
        },
        title: title.into(),
        author: "mira".into(),
        author_is_bot: false,
        state: ChangeState::Open,
        draft: false,
        created_at: Timestamp(1_790_000_000),
        updated_at: Timestamp(1_790_100_000),
        branch: "feat".into(),
        base: "main".into(),
        head_sha: "abc123".into(),
        base_sha: "def456".into(),
        adds: 1,
        dels: 1,
        files: 1,
        ci: CiState::Pass,
        labels: Vec::new(),
        reviewers: Vec::new(),
        my_role: MyRole::Reviewing,
        my_review: MyReview::None,
        my_reviewed_sha: None,
        i_commented: false,
        has_new_activity: false,
    }
}

fn store_with(dir: &Path, rows: &[ChangeSummary]) -> Store {
    let mut store = Store::open(&dir.join("cache.sqlite")).unwrap();
    store
        .replace_summaries(&SourceId::new("work"), rows, Timestamp(1_790_000_000))
        .unwrap();
    store
}

fn new_app() -> App {
    App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: rb_theme::ColourDepth::TrueColour,
        no_color: false,
        size: (160, 40),
    })
}

fn start(app: &mut App, live: &Arc<Live>) -> Vec<Cmd> {
    app.state.loading = true;
    update(app, Msg::Cached(Box::new(live.cached_snapshot())))
}

fn run_cmds(cmds: Vec<Cmd>, backend: &Backend, tx: &UnboundedSender<Msg>, only: fn(&Cmd) -> bool) {
    for cmd in cmds.into_iter().filter(only) {
        execute(cmd, tx, backend, &Platform::system());
    }
}

async fn next(rx: &mut UnboundedReceiver<Msg>, want: fn(&Msg) -> bool) -> Msg {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let msg = rx.recv().await.expect("channel open");
            if want(&msg) {
                return msg;
            }
        }
    })
    .await
    .expect("message arrives")
}

fn is_source(msg: &Msg) -> bool {
    matches!(msg, Msg::SourceLoaded { .. })
}

fn screen(app: &mut App) -> String {
    let (w, h) = app.size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|f| hits = ui::draw(f, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    buffer
        .content()
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn cached_rows_paint_first_then_the_live_refresh_replaces_them_and_the_cache() {
    let server = MockServer::start().await;
    serve_searches(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let cached = store_with(
        dir.path(),
        &[change(9, "acme/old", "Saved copy from last time")],
    );
    let live = live_from(
        &config_for(&server, "env:RB_STUB_TOKEN"),
        Some(TOKEN),
        Some(cached),
    );
    let backend = Backend::Live(live.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    let cmds = start(&mut app, &live);
    assert!(server.received_requests().await.unwrap().is_empty());
    assert_eq!(app.state.changes[0].title, "Saved copy from last time");
    assert!(screen(&mut app).contains("Saved copy from last time"));
    assert!(matches!(cmds.first(), Some(Cmd::LoadChanges)));

    run_cmds(cmds, &backend, &tx, |c| matches!(c, Cmd::LoadChanges));
    let msg = next(&mut rx, is_source).await;
    update(&mut app, msg);

    assert_eq!(app.state.pending_sources, 0);
    let titles: Vec<_> = app.state.changes.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["Add retry to sync"]);
    assert!(app.state.failures.is_empty());
    let requests = server.received_requests().await.unwrap();
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|r| r
        .headers
        .get("authorization")
        .is_some_and(|v| v.to_str().unwrap() == format!("Bearer {TOKEN}"))));

    let reopened = Store::open(&dir.path().join("cache.sqlite")).unwrap();
    let rows = reopened.list_summaries(&SourceId::new("work")).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Add retry to sync");
}

#[tokio::test]
async fn a_missing_token_without_a_cache_shows_the_sign_in_state_not_demo_data() {
    let server = MockServer::start().await;
    serve_searches(&server).await;
    let live = live_from(&config_for(&server, "env:RB_STUB_TOKEN"), None, None);
    let backend = Backend::Live(live.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    let cmds = start(&mut app, &live);
    run_cmds(cmds, &backend, &tx, |c| matches!(c, Cmd::LoadChanges));
    let msg = next(&mut rx, is_source).await;
    update(&mut app, msg);

    let failure = app.state.failures.values().next().expect("a failure");
    assert_eq!(failure.kind, FailureKind::SignIn);
    assert!(app.toasts[0].notice.text.contains("gh auth login"));
    assert!(app.toasts[0]
        .notice
        .text
        .contains("review-buddy auth login"));
    assert!(server.received_requests().await.unwrap().is_empty());
    let shown = screen(&mut app);
    assert!(shown.contains("Sign in to see your reviews."), "{shown}");
    assert!(shown.contains("sign-in needed"), "{shown}");
    assert!(!shown.contains("(demo)"));
}

#[tokio::test]
async fn a_failed_refresh_keeps_the_cached_rows_and_says_why() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({"message": "Bad credentials"})),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let cached = store_with(
        dir.path(),
        &[change(9, "acme/old", "Saved copy from last time")],
    );
    let live = live_from(
        &config_for(&server, "env:RB_STUB_TOKEN"),
        Some(TOKEN),
        Some(cached),
    );
    let backend = Backend::Live(live.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    let cmds = start(&mut app, &live);
    run_cmds(cmds, &backend, &tx, |c| matches!(c, Cmd::LoadChanges));
    let msg = next(&mut rx, is_source).await;
    update(&mut app, msg);

    assert_eq!(app.state.changes[0].title, "Saved copy from last time");
    assert_eq!(
        app.state.failures.values().next().unwrap().kind,
        FailureKind::SignIn
    );
    let shown = screen(&mut app);
    assert!(shown.contains("Saved copy from last time"));
    assert!(shown.contains("sign-in needed"));
}

#[tokio::test]
async fn rate_limits_surface_when_the_limit_resets() {
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
                .set_body_json(serde_json::json!({"message": "API rate limit exceeded"})),
        )
        .mount(&server)
        .await;
    let live = live_from(&config_for(&server, "env:RB_STUB_TOKEN"), Some(TOKEN), None);
    let backend = Backend::Live(live.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    let cmds = start(&mut app, &live);
    run_cmds(cmds, &backend, &tx, |c| matches!(c, Cmd::LoadChanges));
    let msg = next(&mut rx, is_source).await;
    update(&mut app, msg);

    let failure = app.state.failures.values().next().unwrap();
    assert_eq!(failure.kind, FailureKind::RateLimited);
    assert!(
        failure.next_step.contains("about 10 minutes")
            || failure.next_step.contains("about 9 minutes"),
        "{}",
        failure.next_step
    );
    assert!(app.toasts[0].notice.text.contains("Press r"));
    assert!(screen(&mut app).contains("rate limited"));
}

#[tokio::test]
async fn gitlab_sources_get_a_calm_state_and_the_github_source_still_loads() {
    let server = MockServer::start().await;
    serve_searches(&server).await;
    let config: Config = toml::from_str(&format!(
        r#"
[[source]]
name = "work"
kind = "github"
host = "ghe.test"
api_url = "{}"
auth = "env:RB_STUB_TOKEN"

[[source]]
name = "lab"
kind = "gitlab"
host = "gitlab.com"
"#,
        server.uri()
    ))
    .unwrap();
    let live = live_from(&config, Some(TOKEN), None);
    let backend = Backend::Live(live.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    let cmds = start(&mut app, &live);
    run_cmds(cmds, &backend, &tx, |c| matches!(c, Cmd::LoadChanges));
    for _ in 0..2 {
        let msg = next(&mut rx, is_source).await;
        update(&mut app, msg);
    }

    assert_eq!(app.state.pending_sources, 0);
    assert_eq!(app.state.changes.len(), 1);
    let lab = &app.state.failures[&SourceId::new("lab")];
    assert_eq!(lab.kind, FailureKind::Unavailable);
    assert!(lab.summary.contains("v0.2"));
}

#[tokio::test]
async fn details_and_diff_load_lazily_through_the_factory() {
    let server = MockServer::start().await;
    serve_details(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let cached = store_with(
        dir.path(),
        &[change(7, "acme/widgets", "Widgets get springs")],
    );
    let live = live_from(
        &config_for(&server, "env:RB_STUB_TOKEN"),
        Some(TOKEN),
        Some(cached),
    );
    let backend = Backend::Live(live.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    let cmds = start(&mut app, &live);
    let id = app.state.changes[0].id.clone();
    assert!(cmds
        .iter()
        .any(|c| matches!(c, Cmd::LoadInfo(i) if *i == id)));
    assert!(server.received_requests().await.unwrap().is_empty());
    run_cmds(cmds, &backend, &tx, |c| matches!(c, Cmd::LoadInfo(_)));
    let msg = next(&mut rx, |m| matches!(m, Msg::InfoLoaded { .. })).await;
    update(&mut app, msg);
    let info = &app.state.details[&id];
    assert!(!info.checks.is_empty());
    assert!(!info.threads.is_empty());

    execute(
        Cmd::LoadDiff(id.clone()),
        &tx,
        &backend,
        &Platform::system(),
    );
    let msg = next(&mut rx, |m| matches!(m, Msg::DiffLoaded { .. })).await;
    match msg {
        Msg::DiffLoaded { result, .. } => {
            let data = result.expect("diff loads");
            assert!(!data.files.is_empty());
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn pressing_r_refreshes_again_once_the_first_refresh_is_done() {
    let server = MockServer::start().await;
    serve_searches(&server).await;
    let live = live_from(&config_for(&server, "env:RB_STUB_TOKEN"), Some(TOKEN), None);
    let backend = Backend::Live(live.clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    let cmds = start(&mut app, &live);
    run_cmds(cmds, &backend, &tx, |c| matches!(c, Cmd::LoadChanges));
    let msg = next(&mut rx, is_source).await;
    update(&mut app, msg);

    let key = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('r'));
    let cmds = update(&mut app, Msg::Key(key));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::LoadChanges)));
    run_cmds(cmds, &backend, &tx, |c| matches!(c, Cmd::LoadChanges));
    let msg = next(&mut rx, is_source).await;
    update(&mut app, msg);
    assert_eq!(app.state.pending_sources, 0);

    let cmds = update(&mut app, Msg::FocusGained);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::LoadChanges)));
}
