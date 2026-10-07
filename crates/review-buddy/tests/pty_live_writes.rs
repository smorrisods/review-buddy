//! Drives the real binary in a pseudo-terminal against a stub GitHub server: open a change's
//! diff, add a comment, approve, and check what reached the server. Writes only ever go to the
//! stub.
#![cfg(all(unix, feature = "live"))]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
mod support;

use support::*;
use wiremock::MockServer;

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

impl Session {
    fn send(&mut self, bytes: &str) {
        self.writer.write_all(bytes.as_bytes()).unwrap();
        self.writer.flush().unwrap();
    }

    fn expect(&mut self, needle: &str) {
        assert!(
            self.sees(needle),
            "waiting for {needle:?}; output so far: {:?}",
            tail(&self.seen)
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn adding_a_comment_and_approving_writes_through_the_stub_after_the_preview() {
    let server = MockServer::start().await;
    serve_happy_writes(&server).await;
    serve_searches(&server).await;
    serve_reads(&server).await;

    let (_home, mut session) = spawn(&config(&server), Some("ghp_pty_stub_token"));
    session.expect("Widgets get springs");
    session.send("\r");
    session.expect("new_name.rs");
    session.send("c");
    std::thread::sleep(Duration::from_millis(300));
    session.send("Looks right to me");
    std::thread::sleep(Duration::from_millis(300));
    session.send("\r");
    std::thread::sleep(Duration::from_millis(300));
    session.send("a");
    session.expect("Submit your review");
    assert!(
        writes(&server).await.is_empty(),
        "nothing is written before the preview is confirmed"
    );
    session.send("\r");
    session.expect("Approved");

    let sent = writes(&server).await;
    let names: Vec<_> = sent.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["find pending", "create review", "add thread", "submit"]
    );
    assert_eq!(sent[2].1["input"]["body"], "Looks right to me");
    assert_eq!(sent[2].1["input"]["path"], "src/new_name.rs");
    assert_eq!(sent[3].1["input"]["event"], "APPROVE");
    assert!(!String::from_utf8_lossy(&session.seen).contains("(demo)"));
    session.send("q");
    session.expect("Widgets get springs");
    session.quit();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_broken_config_stops_the_launch_with_the_file_and_line_and_exit_code_2() {
    let server = MockServer::start().await;
    let broken = format!(
        "{}\n[review]\nconfirm_post_now = \"sometimes\"\n",
        config(&server)
    );
    let (home, mut session) = spawn(&broken, Some("ghp_pty_stub_token"));
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = session.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the process should have exited");
        if let Ok(chunk) = session.rx.recv_timeout(Duration::from_millis(50)) {
            session.seen.extend(chunk);
        }
    };
    while let Ok(chunk) = session.rx.recv_timeout(Duration::from_millis(200)) {
        session.seen.extend(chunk);
    }
    assert_eq!(status.exit_code(), 2);
    let shown = String::from_utf8_lossy(&session.seen).to_string();
    let file = home.path().join("XDG_CONFIG_HOME/review-buddy/config.toml");
    assert!(
        shown.contains(&format!("{}:", file.display())) && shown.contains(":9:"),
        "{shown}"
    );
    assert!(shown.contains("Fix the config file"), "{shown}");
    assert!(server.received_requests().await.unwrap().is_empty());
}
