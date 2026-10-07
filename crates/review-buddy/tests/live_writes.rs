//! Review writes through the live backend, end to end against a stub GitHub: the real `Live`,
//! `runtime::execute` and the same `update` loop the interface runs. Nothing here reaches a
//! real forge.
#![cfg(feature = "live")]

mod support;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_core::{ChangeId, ForgeKind, SourceId};
use rb_platform::{CommandOutput, CommandRunner, MemorySecretStore, PlatformError};
use review_buddy::app::{update, App, AppConfig, Cmd, Msg, Screen};
use review_buddy::config::Config;
use review_buddy::providers::{Deps, Factory, Live};
use review_buddy::runtime::{execute, Backend, Platform, Settings};
use serde_json::{json, Value};
use support::*;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn live_for(api_url: &str) -> Arc<Live> {
    let config: Config = toml::from_str(&format!(
        r#"
[[source]]
name = "work"
kind = "github"
host = "ghe.test"
api_url = "{api_url}"
auth = "env:RB_STUB_TOKEN"
"#
    ))
    .unwrap();
    let env: HashMap<String, String> = [("RB_STUB_TOKEN".to_string(), TOKEN.to_string())].into();
    let deps = Deps {
        runner: Arc::new(NoCli),
        store: Arc::new(MemorySecretStore::new()),
        getenv: Arc::new(move |k| env.get(k).cloned()),
    };
    let factory = Arc::new(Factory::from_config(&config, deps));
    let sources = factory.sources();
    Arc::new(Live::new(factory, sources, None, 4))
}

fn change_id() -> ChangeId {
    ChangeId {
        source_id: SourceId::new("work"),
        kind: ForgeKind::GitHub,
        repo: "acme/widgets".into(),
        number: 7,
    }
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

    fn ctrl_enter(&mut self) -> Vec<Cmd> {
        update(
            &mut self.app,
            Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
        )
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.press(KeyCode::Char(c));
        }
    }

    fn run(&self, cmds: Vec<Cmd>) {
        for cmd in cmds {
            execute(cmd, &self.tx, &self.backend, &Platform::system());
        }
    }

    async fn next(&mut self, want: fn(&Msg) -> bool) -> Msg {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let msg = self.rx.recv().await.expect("channel open");
                if want(&msg) {
                    return msg;
                }
            }
        })
        .await
        .expect("message arrives")
    }

    /// Sends what the cmds ask for and feeds the one answer back through `update`.
    async fn settle(&mut self, cmds: Vec<Cmd>, want: fn(&Msg) -> bool) -> Vec<Cmd> {
        self.run(cmds);
        let msg = self.next(want).await;
        update(&mut self.app, msg)
    }

    fn diff(&self) -> &review_buddy::app::DiffState {
        self.app.diff.as_ref().unwrap()
    }

    fn draft_len(&self) -> usize {
        self.diff().data.as_ref().unwrap().draft.comments.len()
    }

    fn toast(&self) -> String {
        self.app.toasts.last().expect("a toast").notice.text.clone()
    }

    fn submitting(&self) -> bool {
        self.diff().submitting
    }
}

fn is_submitted(msg: &Msg) -> bool {
    matches!(msg, Msg::ReviewSubmitted { .. })
}

fn is_replied(msg: &Msg) -> bool {
    matches!(msg, Msg::ReplyPosted { .. })
}

fn is_info(msg: &Msg) -> bool {
    matches!(msg, Msg::InfoLoaded { .. })
}

/// Opens the diff of acme/widgets#7 from `server` and returns the loaded session.
async fn open_diff(server: &MockServer, settings: Option<Settings>) -> Session {
    let live = live_for(&server.uri());
    let backend = Backend::Live(live);
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: rb_theme::ColourDepth::TrueColour,
        no_color: false,
        size: (160, 40),
    });
    if let Some(settings) = &settings {
        settings.apply(&mut app);
    }
    let mut session = Session {
        app,
        backend,
        tx,
        rx,
    };
    let id = change_id();
    let cmds = review_buddy::app::open_change(&mut session.app, &id);
    session
        .settle(cmds, |m| matches!(m, Msg::DiffLoaded { .. }))
        .await;
    assert_eq!(session.app.screen, Screen::Diff);
    assert!(session.diff().data.is_some(), "the diff loaded");
    session
}

async fn full_stub() -> MockServer {
    let server = MockServer::start().await;
    serve_happy_writes(&server).await;
    serve_reads(&server).await;
    server
}

fn composer_open(session: &Session) -> bool {
    session.diff().composer.is_some()
}

fn add_comment(session: &mut Session, text: &str) {
    session.press(KeyCode::Char('c'));
    assert!(composer_open(session), "the composer opened");
    session.type_text(text);
    session.press(KeyCode::Enter);
    assert_eq!(session.draft_len(), 1, "the comment joined the review");
}

#[tokio::test]
async fn a_comment_then_approve_sends_the_exact_review_and_clears_the_draft() {
    let server = full_stub().await;
    let mut s = open_diff(&server, None).await;
    add_comment(&mut s, "Looks right to me");
    assert!(
        writes(&server).await.is_empty(),
        "nothing is sent while drafting"
    );

    s.press(KeyCode::Char('a'));
    assert!(s.diff().review.is_some(), "the preview comes first");
    assert!(
        writes(&server).await.is_empty(),
        "nothing is sent before confirming"
    );
    let reads_before = count_requests(&server, "reviewThreads").await;

    let cmds = s.press(KeyCode::Enter);
    assert!(matches!(cmds.as_slice(), [Cmd::SubmitReview { .. }]));
    assert!(s.submitting());
    s.press(KeyCode::Char('a'));
    assert!(
        s.press(KeyCode::Enter).is_empty(),
        "no second submit while one is in flight"
    );

    let cmds = s.settle(cmds, is_submitted).await;
    assert_eq!(s.toast(), "Approved");
    assert_eq!(s.draft_len(), 0);
    assert!(!s.submitting());
    assert!(cmds.iter().any(|c| matches!(c, Cmd::LoadChanges)));
    assert!(cmds
        .iter()
        .any(|c| matches!(c, Cmd::LoadInfo(i) if *i == change_id())));

    let sent = writes(&server).await;
    let names: Vec<_> = sent.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["find pending", "create review", "add thread", "submit"]
    );
    assert_eq!(
        sent[0].1,
        json!({"owner": "acme", "name": "widgets", "number": 7})
    );
    assert_eq!(sent[1].1, json!({"input": {"pullRequestId": "PR_1"}}));
    let thread = &sent[2].1["input"];
    assert_eq!(thread["pullRequestReviewId"], "PRR_new");
    assert_eq!(thread["path"], "src/new_name.rs");
    assert_eq!(thread["body"], "Looks right to me");
    assert!(thread["line"].is_u64() && thread["side"].is_string());
    assert_eq!(
        sent[3].1,
        json!({"input": {"pullRequestReviewId": "PRR_new", "event": "APPROVE"}})
    );
    assert!(server.received_requests().await.unwrap().iter().all(|r| r
        .headers
        .get("authorization")
        .is_some_and(|v| v.to_str().unwrap() == format!("Bearer {TOKEN}"))));

    s.run(
        cmds.into_iter()
            .filter(|c| matches!(c, Cmd::LoadInfo(_)))
            .collect(),
    );
    let msg = s.next(is_info).await;
    update(&mut s.app, msg);
    assert!(
        count_requests(&server, "reviewThreads").await > reads_before,
        "the change's threads were fetched again"
    );
    assert!(s.app.state.details.contains_key(&change_id()));
}

#[tokio::test]
async fn an_existing_pending_review_is_reused_without_duplicating_comments() {
    let server = MockServer::start().await;
    let existing = json!({"repository": {"pullRequest": {"id": "PR_1", "reviews": {"nodes": [
        {"id": "PRR_old", "comments": {"nodes": []}}
    ]}}}});
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Op(FIND_PENDING))
        .respond_with(gql(existing))
        .mount(&server)
        .await;
    serve_write_chain(&server, thread_added()).await;
    serve_reads(&server).await;
    let mut s = open_diff(&server, None).await;
    add_comment(&mut s, "Reuse the pending review");
    s.press(KeyCode::Char('a'));
    let cmds = s.press(KeyCode::Enter);
    s.settle(cmds, is_submitted).await;

    let names: Vec<_> = writes(&server).await.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["find pending", "add thread", "submit"]);
    let sent = writes(&server).await;
    assert_eq!(sent[1].1["input"]["pullRequestReviewId"], "PRR_old");
}

#[tokio::test]
async fn request_changes_sends_the_summary_as_the_review_body() {
    let server = full_stub().await;
    let mut s = open_diff(&server, None).await;
    add_comment(&mut s, "This branch can be simpler");
    s.press(KeyCode::Char('x'));
    assert!(
        writes(&server).await.is_empty(),
        "nothing is sent while the modal is open"
    );
    s.type_text("Please simplify before merging.");
    let cmds = s.ctrl_enter();
    assert!(matches!(
        cmds.as_slice(),
        [Cmd::SubmitReview {
            verdict: rb_core::Verdict::RequestChanges,
            ..
        }]
    ));
    let cmds = s.settle(cmds, is_submitted).await;
    assert_eq!(s.toast(), "Changes requested");
    assert_eq!(s.draft_len(), 0);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::LoadChanges)));

    let sent = writes(&server).await;
    let names: Vec<_> = sent.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["find pending", "create review", "add thread", "submit"]
    );
    assert_eq!(
        sent[3].1,
        json!({"input": {
            "pullRequestReviewId": "PRR_new",
            "event": "REQUEST_CHANGES",
            "body": "Please simplify before merging."
        }})
    );
}

#[tokio::test]
async fn a_comment_review_posts_the_pending_comments_without_approving() {
    let server = full_stub().await;
    let mut s = open_diff(&server, None).await;
    add_comment(&mut s, "A thought on naming");
    s.press(KeyCode::Char('R'));
    let cmds = s.press(KeyCode::Enter);
    assert!(matches!(
        cmds.as_slice(),
        [Cmd::SubmitReview {
            verdict: rb_core::Verdict::Comment,
            ..
        }]
    ));
    s.settle(cmds, is_submitted).await;
    assert_eq!(s.toast(), "Review posted");
    assert_eq!(s.draft_len(), 0);

    let sent = writes(&server).await;
    let names: Vec<_> = sent.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["find pending", "create review", "add thread", "submit"]
    );
    assert_eq!(sent[2].1["input"]["body"], "A thought on naming");
    assert_eq!(
        sent[3].1,
        json!({"input": {"pullRequestReviewId": "PRR_new", "event": "COMMENT"}})
    );
}

async fn failing_approval(thread: ResponseTemplate) -> (MockServer, Session) {
    let server = MockServer::start().await;
    serve_write_chain(&server, thread).await;
    serve_reads(&server).await;
    let mut s = open_diff(&server, None).await;
    add_comment(&mut s, "Keep me safe");
    s.press(KeyCode::Char('a'));
    let cmds = s.press(KeyCode::Enter);
    let cmds = s.settle(cmds, is_submitted).await;
    assert!(!cmds
        .iter()
        .any(|c| matches!(c, Cmd::LoadChanges | Cmd::LoadInfo(_))));
    (server, s)
}

fn assert_draft_kept(s: &Session, toast_has: &[&str]) {
    assert_eq!(s.draft_len(), 1, "the draft is kept");
    assert!(!s.submitting(), "the submit flag is released");
    let toast = s.toast();
    assert!(toast.starts_with("Couldn't submit your review"), "{toast}");
    assert!(
        toast
            .ends_with("Your comments and summary are still here. Press ⏎ on Submit to try again."),
        "{toast}"
    );
    for part in toast_has {
        assert!(toast.contains(part), "{toast}");
    }
}

#[tokio::test]
async fn a_403_keeps_the_draft_and_says_which_scope_is_missing() {
    let denied = ResponseTemplate::new(403)
        .set_body_json(json!({"message": "Resource not accessible by personal access token"}));
    let (_server, s) = failing_approval(denied).await;
    assert_draft_kept(&s, &["`repo` scope"]);
}

#[tokio::test]
async fn a_422_for_a_moved_line_keeps_the_draft_and_says_to_refresh() {
    let moved = ResponseTemplate::new(422)
        .set_body_json(json!({"message": "Line must be part of the diff"}));
    let (_server, s) = failing_approval(moved).await;
    assert_draft_kept(&s, &["Refresh the diff"]);
}

#[tokio::test]
async fn a_rate_limit_keeps_the_draft_and_says_to_wait() {
    let (_server, s) = failing_approval(gql_error("RATE_LIMITED", "API rate limit exceeded")).await;
    assert_draft_kept(&s, &["rate limit"]);
}

#[tokio::test]
async fn an_unreachable_server_keeps_the_draft_and_says_to_try_again() {
    let server = full_stub().await;
    let mut s = open_diff(&server, None).await;
    let dead = MockServer::start().await;
    let dead_uri = dead.uri();
    drop(dead);
    s.backend = Backend::Live(live_for(&dead_uri));
    add_comment(&mut s, "Keep me safe");
    s.press(KeyCode::Char('a'));
    let cmds = s.press(KeyCode::Enter);
    s.settle(cmds, is_submitted).await;
    assert_draft_kept(&s, &[]);

    s.backend = Backend::Live(live_for(&server.uri()));
    s.press(KeyCode::Char('a'));
    let cmds = s.press(KeyCode::Enter);
    s.settle(cmds, is_submitted).await;
    assert_eq!(s.toast(), "Approved");
    assert_eq!(s.draft_len(), 0, "trying again works and clears the draft");
}

#[tokio::test]
async fn a_failure_after_the_pending_review_exists_says_it_is_saved_there() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Op(SUBMIT))
        .respond_with(gql_error("FORBIDDEN", "nope"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    serve_happy_writes(&server).await;
    serve_reads(&server).await;
    let mut s = open_diff(&server, None).await;
    add_comment(&mut s, "Once only");
    s.press(KeyCode::Char('a'));
    let cmds = s.press(KeyCode::Enter);
    s.settle(cmds, is_submitted).await;
    assert_draft_kept(&s, &["saved as pending on GitHub"]);
}

fn reply_threads(comments: Value) -> Value {
    json!({"data": {"rateLimit": {"cost": 1, "remaining": 4990, "resetAt": "2999-01-01T00:00:00Z"},
    "repository": {"pullRequest": {"reviewThreads": {
      "pageInfo": {"hasNextPage": false, "endCursor": null},
      "nodes": [{"id": "PRT_9", "isResolved": false, "isOutdated": false,
        "path": "src/lib.rs", "line": 2, "originalLine": 2, "startLine": null,
        "diffSide": "LEFT", "startDiffSide": null,
        "comments": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": comments}}]
    }}}}})
}

fn comment(id: &str, who: &str, body: &str) -> Value {
    json!({"id": id, "author": {"login": who}, "body": body, "createdAt": "2026-01-02T10:00:00Z"})
}

#[tokio::test]
async fn replying_posts_to_the_thread_and_the_threads_are_fetched_again() {
    let server = MockServer::start().await;
    let graphql = || Mock::given(method("POST")).and(path("/graphql"));
    graphql()
        .and(Op(REPLY))
        .respond_with(gql(json!({"addPullRequestReviewThreadReply": {"comment": {
            "id": "PRRC_9", "author": {"login": "octo"}, "body": "Agreed",
            "createdAt": "2026-01-03T10:00:00Z", "state": "SUBMITTED"}}})))
        .mount(&server)
        .await;
    graphql()
        .and(Body("reviewThreads"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(reply_threads(json!([comment(
                "PRRC_1",
                "alice",
                "Why drop this?"
            )]))),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    graphql()
        .and(Body("reviewThreads"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(reply_threads(json!([
                comment("PRRC_1", "alice", "Why drop this?"),
                comment("PRRC_9", "octo", "Agreed")
            ]))),
        )
        .mount(&server)
        .await;
    graphql()
        .and(Body("mergeable"))
        .respond_with(fixture_response("detail"))
        .mount(&server)
        .await;
    let files = json!([{"filename": "src/lib.rs", "status": "modified", "additions": 1,
        "deletions": 1, "changes": 2, "patch": "@@ -1,3 +1,3 @@\n a\n-b\n+c\n d"}]);
    serve_rest_reads(&server, files).await;

    let mut s = open_diff(&server, None).await;
    for _ in 0..12 {
        s.press(KeyCode::Char('j'));
        s.press(KeyCode::Char('r'));
        if composer_open(&s) {
            break;
        }
    }
    assert!(composer_open(&s), "a reply composer opened on the thread");
    s.type_text("Agreed");
    s.press(KeyCode::Enter);
    assert!(s.diff().confirm.is_some(), "the reply is previewed first");
    assert!(writes(&server).await.is_empty());

    let cmds = s.press(KeyCode::Enter);
    assert!(matches!(cmds.as_slice(), [Cmd::Reply { .. }]));
    let cmds = s.settle(cmds, is_replied).await;
    assert_eq!(s.toast(), "Reply posted");
    let sent = writes(&server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].1,
        json!({"input": {"pullRequestReviewThreadId": "PRT_9", "body": "Agreed"}})
    );

    s.run(cmds);
    let msg = s.next(is_info).await;
    update(&mut s.app, msg);
    let threads = &s.diff().data.as_ref().unwrap().threads;
    assert_eq!(threads[0].comments.len(), 2);
    assert_eq!(threads[0].comments[1].id.0, "PRRC_9");
}

#[tokio::test]
async fn a_failed_reply_keeps_the_text_in_the_box() {
    let server = MockServer::start().await;
    let graphql = || Mock::given(method("POST")).and(path("/graphql"));
    graphql()
        .and(Op(REPLY))
        .respond_with(gql_error("FORBIDDEN", "nope"))
        .mount(&server)
        .await;
    graphql()
        .and(Body("reviewThreads"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(reply_threads(json!([comment(
                "PRRC_1",
                "alice",
                "Why drop this?"
            )]))),
        )
        .mount(&server)
        .await;
    let files = json!([{"filename": "src/lib.rs", "status": "modified", "additions": 1,
        "deletions": 1, "changes": 2, "patch": "@@ -1,3 +1,3 @@\n a\n-b\n+c\n d"}]);
    serve_rest_reads(&server, files).await;
    let mut s = open_diff(&server, None).await;
    for _ in 0..12 {
        s.press(KeyCode::Char('j'));
        s.press(KeyCode::Char('r'));
        if composer_open(&s) {
            break;
        }
    }
    s.type_text("Agreed");
    s.press(KeyCode::Enter);
    let cmds = s.press(KeyCode::Enter);
    s.settle(cmds, is_replied).await;
    assert!(composer_open(&s), "the reply box stays open");
    assert!(!s.submitting());
    let toast = s.toast();
    assert!(toast.starts_with("Couldn't post your reply"), "{toast}");
    assert!(
        toast.ends_with("Your text is still in the box. Press ⏎ to try again."),
        "{toast}"
    );
}

fn settings(toml: &str) -> Settings {
    let config: Config = toml::from_str(toml).unwrap();
    Settings::from_config(&config)
}

#[tokio::test]
async fn confirm_post_now_false_posts_straight_away_and_true_previews_first() {
    let server = full_stub().await;
    let mut s = open_diff(
        &server,
        Some(settings("[review]\nconfirm_post_now = false\n")),
    )
    .await;
    s.press(KeyCode::Char('c'));
    s.type_text("One-off note");
    let cmds = s.ctrl_enter();
    assert!(
        s.diff().confirm.is_none(),
        "no preview when it's switched off"
    );
    assert!(matches!(cmds.as_slice(), [Cmd::SubmitReview { .. }]));
    s.settle(cmds, is_submitted).await;
    assert_eq!(s.toast(), "Comment posted");
    let names: Vec<_> = writes(&server).await.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["find pending", "create review", "add thread", "submit"]
    );
    assert_eq!(writes(&server).await[3].1["input"]["event"], "COMMENT");

    let server = full_stub().await;
    let mut s = open_diff(&server, Some(settings(""))).await;
    s.press(KeyCode::Char('c'));
    s.type_text("One-off note");
    let cmds = s.ctrl_enter();
    assert!(cmds.is_empty() && s.diff().confirm.is_some());
    assert!(
        writes(&server).await.is_empty(),
        "nothing is sent before the preview is accepted"
    );
}

#[test]
fn settings_follow_the_config_and_clamp_the_tab_width() {
    let s = settings("[ui]\ntheme = \"midnight\"\ncolour_depth = \"256\"\n[diff]\ntab_width = 2\n");
    assert_eq!(s.theme, "midnight");
    assert_eq!(s.tab_width, 2);
    assert_eq!(s.depth, Some(rb_theme::ColourDepth::Ansi256));
    assert!(s.confirm_post_now);
    assert_eq!(settings("[diff]\ntab_width = 0\n").tab_width, 1);
    assert_eq!(settings("").depth, None);
    let config = s.app_config(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: rb_theme::ColourDepth::TrueColour,
        no_color: false,
        size: (80, 24),
    });
    assert_eq!(config.theme_id, "midnight");
    assert_eq!(config.depth, rb_theme::ColourDepth::Ansi256);
}
