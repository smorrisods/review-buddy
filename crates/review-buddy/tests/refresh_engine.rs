//! The refresh engine against a stub server and a clock that only moves when told to: pauses
//! and resumes, conditional requests, and recovery from an outage, with no real waiting.
#![cfg(feature = "live")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ratatui::{backend::TestBackend, Terminal};
use rb_core::{
    ChangeId, ChangeState, ChangeSummary, CiState, ForgeKind, MyReview, MyRole, SourceId, Timestamp,
};
use rb_platform::{CommandOutput, CommandRunner, MemorySecretStore, PlatformError};
use rb_store::Store;
use review_buddy::app::{update, App, AppConfig, FailureKind, Msg, SourceStatus};
use review_buddy::config::Config;
use review_buddy::providers::refresh::{FixedJitter, ManualClock};
use review_buddy::providers::{Deps, Factory, Live};
use review_buddy::ui::{self, HitMap};
use serde_json::Value;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "ghp_stub_token_value";
const START: Timestamp = Timestamp(1_790_000_000);

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

async fn serve_ok(server: &MockServer) {
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("review_requested_p2")))
        .mount(server)
        .await;
}

async fn serve_status(server: &MockServer, status: u16, retry_after: Option<&str>) {
    server.reset().await;
    let mut response = ResponseTemplate::new(status)
        .set_body_json(serde_json::json!({"message": "API rate limit exceeded"}));
    if let Some(secs) = retry_after {
        response = response.insert_header("retry-after", secs);
    }
    Mock::given(method("POST"))
        .respond_with(response)
        .mount(server)
        .await;
}

fn config_for(api_url: &str) -> Config {
    toml::from_str(&format!(
        r#"
[[source]]
name = "work"
kind = "github"
host = "ghe.test"
api_url = "{api_url}"
auth = "env:RB_STUB_TOKEN"
"#
    ))
    .unwrap()
}

fn deps() -> Deps {
    let env: HashMap<String, String> =
        HashMap::from([("RB_STUB_TOKEN".to_string(), TOKEN.to_string())]);
    Deps {
        runner: Arc::new(NoCli),
        store: Arc::new(MemorySecretStore::new()),
        getenv: Arc::new(move |k| env.get(k).cloned()),
    }
}

fn live_with(api_url: &str, cache: Option<Store>) -> (Arc<Live>, Arc<ManualClock>) {
    let factory = Arc::new(Factory::from_config(&config_for(api_url), deps()));
    let sources = factory.sources();
    let clock = ManualClock::new(START);
    let live = Live::new(factory, sources, cache, 4)
        .with_clock(clock.clone())
        .with_jitter(Arc::new(FixedJitter(0.0)));
    (Arc::new(live), clock)
}

fn saved(title: &str) -> ChangeSummary {
    ChangeSummary {
        id: ChangeId {
            source_id: SourceId::new("work"),
            kind: ForgeKind::GitHub,
            repo: "acme/old".into(),
            number: 9,
        },
        title: title.into(),
        author: "mira".into(),
        author_is_bot: false,
        state: ChangeState::Open,
        draft: false,
        created_at: Timestamp(START.0 - 100),
        updated_at: Timestamp(START.0 - 50),
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

fn cache_in(dir: &std::path::Path, rows: &[ChangeSummary]) -> Store {
    let mut store = Store::open(&dir.join("cache.sqlite")).unwrap();
    store
        .replace_summaries(&SourceId::new("work"), rows, START)
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

fn launch(live: &Arc<Live>, app: &mut App, tx: &UnboundedSender<Msg>) {
    update(app, Msg::Cached(Box::new(live.cached_snapshot())));
    if let Some(at) = live.cached_at() {
        update(app, Msg::CacheTime(at));
    }
    live.refresh(tx);
}

/// The next message the app should see, applied. Real time only passes while waiting for the
/// spawned task to answer; the engine's own waiting runs on the manual clock.
async fn step(app: &mut App, rx: &mut UnboundedReceiver<Msg>) -> Msg {
    let msg = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("a message arrives")
        .expect("channel open");
    update(app, msg.clone());
    msg
}

async fn settle(app: &mut App, rx: &mut UnboundedReceiver<Msg>, want: fn(&Msg) -> bool) {
    while !want(&step(app, rx).await) {}
}

fn is_answer(msg: &Msg) -> bool {
    matches!(msg, Msg::SourceLoaded { .. } | Msg::SourceUpdated { .. })
}

#[tokio::test]
async fn a_rate_limit_pauses_the_host_then_resumes_at_the_reset() {
    let server = MockServer::start().await;
    serve_status(&server, 429, Some("90")).await;
    let (live, clock) = live_with(&server.uri(), None);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    launch(&live, &mut app, &tx);
    settle(&mut app, &mut rx, is_answer).await;
    let id = SourceId::new("work");
    let until = Timestamp(START.0 + 90);
    assert_eq!(live.status(&id), SourceStatus::RateLimited { until });
    assert_eq!(app.state.failures[&id].kind, FailureKind::RateLimited);
    assert_eq!(app.toasts.len(), 1);
    let text = screen(&mut app);
    assert!(text.contains("paused until 14:14"), "{text}");
    clock.parked(1).await;
    let sent = server.received_requests().await.unwrap().len();
    assert_eq!(sent, 1);

    // Asking again, even with `r`, sends nothing while the host is paused.
    clock.advance(Duration::from_secs(30));
    live.refresh_now(&tx);
    settle(&mut app, &mut rx, is_answer).await;
    assert_eq!(server.received_requests().await.unwrap().len(), sent);
    assert_eq!(app.toasts.len(), 1, "no second toast for the same pause");

    serve_ok(&server).await;
    clock.advance(Duration::from_secs(60));
    settle(&mut app, &mut rx, |m| {
        matches!(m, Msg::SourceUpdated { .. })
    })
    .await;
    assert!(app.state.failures.is_empty());
    assert_eq!(app.state.changes.len(), 1);
    assert_eq!(live.status(&id), SourceStatus::Ok);
}

#[tokio::test]
async fn an_unchanged_page_keeps_the_cached_rows() {
    let server = MockServer::start().await;
    serve_ok(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let (live, _clock) = live_with(
        &server.uri(),
        Some(cache_in(dir.path(), &[saved("Old copy")])),
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    launch(&live, &mut app, &tx);
    settle(&mut app, &mut rx, is_answer).await;
    let titles =
        |app: &App| -> Vec<String> { app.state.changes.iter().map(|c| c.title.clone()).collect() };
    assert_eq!(titles(&app), ["Add retry to sync"]);
    let mut store = Store::open(&dir.path().join("cache.sqlite")).unwrap();
    let work = SourceId::new("work");
    let etags: Vec<_> = store
        .etag(
            &work,
            &format!("changes:{:?}", rb_core::Scope::everything()),
        )
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(etags.len(), 1, "the page's ETag is saved for next time");

    // Swap the saved rows for a marker. The server's answer hasn't changed, so a not-modified
    // reply must leave exactly what is saved.
    let mut marker = saved("Marker");
    marker.id.number = 1;
    store.replace_summaries(&work, &[marker], START).unwrap();
    app.state.pending_sources = 1;
    live.refresh_now(&tx);
    settle(&mut app, &mut rx, is_answer).await;
    assert_eq!(titles(&app), ["Marker"]);
    assert!(app.state.failures.is_empty());
}

#[tokio::test]
async fn an_outage_shows_the_offline_banner_then_recovers_with_backoff() {
    let server = MockServer::start().await;
    serve_status(&server, 503, None).await;
    let dir = tempfile::tempdir().unwrap();
    let (live, clock) = live_with(
        &server.uri(),
        Some(cache_in(dir.path(), &[saved("Saved copy")])),
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    launch(&live, &mut app, &tx);
    settle(&mut app, &mut rx, is_answer).await;
    let id = SourceId::new("work");
    assert!(matches!(live.status(&id), SourceStatus::Offline { .. }));
    assert_eq!(app.state.changes[0].title, "Saved copy", "cached rows stay");
    let text = screen(&mut app);
    assert!(text.contains("offline · cached 14:13"), "{text}");
    assert_eq!(app.toasts.len(), 1);

    // The first retry waits the shortest backoff; a second failure waits longer and stays quiet.
    clock.parked(1).await;
    clock.advance(Duration::from_secs(3));
    settle(&mut app, &mut rx, |m| {
        matches!(m, Msg::SourceUpdated { .. })
    })
    .await;
    assert_eq!(app.toasts.len(), 1, "still offline is not news");
    clock.parked(1).await;
    clock.advance(Duration::from_secs(4));
    clock.parked(1).await;
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        2,
        "the second wait is longer than 4 seconds"
    );

    serve_ok(&server).await;
    clock.advance(Duration::from_secs(10));
    settle(&mut app, &mut rx, |m| {
        matches!(m, Msg::SourceUpdated { .. })
    })
    .await;
    assert!(app.state.failures.is_empty());
    assert!(!screen(&mut app).contains("offline · cached"));
    assert!(app
        .toasts
        .iter()
        .any(|t| t.notice.text.contains("work is back")));
    assert_eq!(live.status(&id), SourceStatus::Ok);
}

#[tokio::test]
async fn a_closed_port_reads_as_offline_and_asking_with_r_retries_at_once() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let dir = tempfile::tempdir().unwrap();
    let (live, clock) = live_with(
        &format!("http://127.0.0.1:{port}"),
        Some(cache_in(dir.path(), &[saved("Saved copy")])),
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    launch(&live, &mut app, &tx);
    settle(&mut app, &mut rx, is_answer).await;
    let id = SourceId::new("work");
    assert_eq!(app.state.failures[&id].kind, FailureKind::Offline);
    assert!(screen(&mut app).contains("offline · cached"));

    clock.parked(1).await;
    app.state.pending_sources = 0;
    assert!(!update(&mut app, Msg::RefreshDue).is_empty());
    live.refresh(&tx);
    settle(&mut app, &mut rx, |m| matches!(m, Msg::SourceLoaded { .. })).await;
    assert_eq!(
        clock.sleepers(),
        1,
        "the interval didn't cut the backoff short"
    );

    app.state.pending_sources = 0;
    live.refresh_now(&tx);
    settle(&mut app, &mut rx, |m| {
        matches!(m, Msg::SourceUpdated { .. })
    })
    .await;
}

#[tokio::test]
async fn a_source_is_probed_once_per_session_and_the_app_hears_about_it() {
    let server = MockServer::start().await;
    serve_ok(&server).await;
    let (live, _clock) = live_with(&server.uri(), None);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = new_app();

    launch(&live, &mut app, &tx);
    settle(&mut app, &mut rx, |m| matches!(m, Msg::Probed { .. })).await;
    let id = SourceId::new("work");
    assert!(app.state.probes.contains_key(&id));
    assert!(app
        .state
        .supports(&id, rb_core::FeatureAction::RequestChanges));

    live.refresh_now(&tx);
    settle(&mut app, &mut rx, is_answer).await;
    tokio::task::yield_now().await;
    let probes = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/user"))
        .count();
    assert_eq!(probes, 1, "the second refresh didn't probe again");
}

#[tokio::test]
async fn overlapping_requests_coalesce_into_one_cycle() {
    let server = MockServer::start().await;
    serve_ok(&server).await;
    let (live, _clock) = live_with(&server.uri(), None);
    let (tx, mut rx) = mpsc::unbounded_channel();
    live.refresh(&tx);
    live.refresh(&tx);
    live.refresh_now(&tx);
    let mut answers = 0;
    while answers < 3 {
        let msg = rx.recv().await.unwrap();
        if is_answer(&msg) {
            answers += 1;
        }
    }
    let listings = |requests: Vec<wiremock::Request>| {
        requests
            .iter()
            .filter(|r| r.url.path().ends_with("/graphql"))
            .count()
    };
    let graphql = listings(server.received_requests().await.unwrap());
    let (solo, _) = live_with(&server.uri(), None);
    solo.refresh(&tx);
    while !is_answer(&rx.recv().await.unwrap()) {}
    let one = listings(server.received_requests().await.unwrap()) - graphql;
    assert_eq!(graphql, one, "three requests made the same calls as one");
}

#[tokio::test]
async fn the_interval_sends_refresh_due_on_the_clock_and_focus_respects_the_gap() {
    let server = MockServer::start().await;
    serve_ok(&server).await;
    let (live, clock) = live_with(&server.uri(), None);
    let (tx, mut rx) = mpsc::unbounded_channel();
    live.spawn_interval(Duration::from_secs(120), &tx);
    clock.parked(1).await;
    assert!(rx.try_recv().is_err());
    clock.advance(Duration::from_secs(108));
    assert!(matches!(rx.recv().await, Some(Msg::RefreshDue)));

    live.refresh(&tx);
    while !is_answer(&rx.recv().await.unwrap()) {}
    clock.advance(Duration::from_secs(10));
    live.refresh_on_focus_gap(&tx);
    assert!(matches!(rx.recv().await, Some(Msg::RefreshSkipped)));
    clock.advance(Duration::from_secs(25));
    live.refresh_on_focus_gap(&tx);
    while !is_answer(&rx.recv().await.unwrap()) {}
}
