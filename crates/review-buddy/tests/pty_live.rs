//! Drives the real binary in a pseudo-terminal against a stub GitHub server named in the config.
#![cfg(all(unix, feature = "live"))]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use wiremock::matchers::{method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

struct Searching;

impl Match for Searching {
    fn matches(&self, request: &Request) -> bool {
        String::from_utf8_lossy(&request.body).contains("\"q\"")
    }
}

fn wait_for(
    rx: &mpsc::Receiver<Vec<u8>>,
    seen: &mut Vec<u8>,
    needle: &str,
    limit: Duration,
) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if String::from_utf8_lossy(seen).contains(needle) {
            return true;
        }
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
            seen.extend(chunk);
        }
    }
    String::from_utf8_lossy(seen).contains(needle)
}

fn spawn(config: &str, token: Option<&str>) -> (tempfile::TempDir, Session) {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("XDG_CONFIG_HOME/review-buddy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), config).unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("NO_COLOR");
    cmd.env_remove("REVIEW_BUDDY_CONFIG");
    cmd.env_remove("RB_STUB_TOKEN");
    cmd.env("HOME", home.path());
    for var in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        cmd.env(var, home.path().join(var));
    }
    if let Some(token) = token {
        cmd.env("RB_STUB_TOKEN", token);
    }
    let child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    (
        home,
        Session {
            child,
            writer,
            rx,
            seen: Vec::new(),
            _master: pair.master,
        },
    )
}

struct Session {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Session {
    fn sees(&mut self, needle: &str) -> bool {
        wait_for(&self.rx, &mut self.seen, needle, Duration::from_secs(15))
    }

    fn quit(mut self) {
        self.writer.write_all(b"q").unwrap();
        self.writer.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(Instant::now() < deadline, "did not exit after q");
            if let Ok(chunk) = self.rx.recv_timeout(Duration::from_millis(50)) {
                self.seen.extend(chunk);
            }
        }
    }
}

fn tail(seen: &[u8]) -> String {
    let text = String::from_utf8_lossy(seen);
    let start = text.len().saturating_sub(600);
    text[text
        .char_indices()
        .map(|(i, _)| i)
        .find(|i| *i >= start)
        .unwrap_or(0)..]
        .to_string()
}

fn config(server: &MockServer) -> String {
    format!(
        r#"[[source]]
name = "stub"
kind = "github"
host = "ghe.test"
api_url = "{}"
auth = "env:RB_STUB_TOKEN"
"#,
        server.uri()
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn live_rows_from_a_stub_server_appear_and_the_token_is_sent() {
    let server = MockServer::start().await;
    let fixture = format!(
        "{}/../rb-github/tests/fixtures/review_requested_p2.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Searching)
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let (_home, mut session) = spawn(&config(&server), Some("ghp_pty_stub_token"));
    assert!(
        session.sees("Add retry to sync"),
        "output so far: {:?}",
        tail(&session.seen)
    );
    assert!(!String::from_utf8_lossy(&session.seen).contains("(demo)"));
    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().any(|r| r
        .headers
        .get("authorization")
        .is_some_and(|v| v.to_str().unwrap() == "Bearer ghp_pty_stub_token")));
    session.quit();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_token_shows_the_sign_in_state() {
    let server = MockServer::start().await;
    let (_home, mut session) = spawn(&config(&server), None);
    assert!(
        session.sees("sign-in needed"),
        "output so far: {:?}",
        tail(&session.seen)
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    session.quit();
}

async fn serve_rows(server: &MockServer) {
    let fixture = format!(
        "{}/../rb-github/tests/fixtures/review_requested_p2.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(Searching)
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn wait_for_requests_beyond(server: &MockServer, seen: usize) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if server.received_requests().await.unwrap().len() > seen {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread")]
async fn pressing_r_sends_another_refresh_and_the_footer_says_when() {
    let server = MockServer::start().await;
    serve_rows(&server).await;
    let config = format!("{}\n[refresh]\ninterval = \"off\"\n", config(&server));

    let (_home, mut session) = spawn(&config, Some("ghp_pty_stub_token"));
    assert!(
        session.sees("Add retry to sync"),
        "output so far: {:?}",
        tail(&session.seen)
    );
    assert!(session.sees("refreshed "), "{:?}", tail(&session.seen));
    let before = server.received_requests().await.unwrap().len();
    session.writer.write_all(b"r").unwrap();
    session.writer.flush().unwrap();
    assert!(wait_for_requests_beyond(&server, before).await);
    session.quit();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_refresh_interval_polls_without_a_keypress() {
    let server = MockServer::start().await;
    serve_rows(&server).await;
    let config = format!("{}\n[refresh]\ninterval = \"1s\"\n", config(&server));

    let (_home, mut session) = spawn(&config, Some("ghp_pty_stub_token"));
    assert!(session.sees("Add retry to sync"));
    let before = server.received_requests().await.unwrap().len();
    assert!(wait_for_requests_beyond(&server, before).await);
    session.quit();
}
