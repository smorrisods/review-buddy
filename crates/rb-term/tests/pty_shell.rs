//! Runs real children in a PTY and checks the screen. Unix only: Windows has no `/bin/sh`, and the
//! ConPTY path can't be exercised from a Unix host.
#![cfg(unix)]

use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rb_term::{Pane, Pty, PtyEvent, SpawnSpec};

struct Session {
    pty: Pty,
    pane: Pane,
    rx: Receiver<PtyEvent>,
    exit: Option<Option<u32>>,
}

fn start(script: &str, cols: u16, rows: u16, env: &[(&str, &str)]) -> Session {
    let (tx, rx) = channel();
    let tx = std::sync::Mutex::new(tx);
    let spec = SpawnSpec {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        cols,
        rows,
    };
    let pty = Pty::spawn(
        &spec,
        Arc::new(move |event| {
            let _ = tx.lock().unwrap().send(event);
        }),
    )
    .expect("spawn /bin/sh");
    Session {
        pty,
        pane: Pane::live(cols, rows, 100),
        rx,
        exit: None,
    }
}

impl Session {
    /// Pumps events until `done` is true for the screen text, or panics after a timeout.
    fn wait_for(&mut self, what: &str, done: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if done(&self.pane.screen().text()) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; screen:\n{}",
                self.pane.screen().text()
            );
            if let Ok(event) = self.rx.recv_timeout(Duration::from_millis(50)) {
                match event {
                    PtyEvent::Output(bytes) => {
                        let out = self.pane.feed(&bytes);
                        if !out.reply.is_empty() {
                            self.pty.write(&out.reply).unwrap();
                        }
                    }
                    PtyEvent::Exited(code) => {
                        self.exit = Some(code);
                        self.pane.mark_exited(code);
                    }
                }
            }
        }
    }

    fn type_(&mut self, text: &str) {
        let bytes = self.pane.write(text.as_bytes());
        self.pty.write(&bytes).unwrap();
    }
}

#[test]
fn a_shell_runs_echoes_input_and_reports_its_exit() {
    let mut s = start(
        "printf 'ready\\n'; read line; printf 'got:%s\\n' \"$line\"; exit 3",
        60,
        10,
        &[],
    );
    s.wait_for("the first line", |t| t.contains("ready"));
    s.type_("abc\r");
    s.wait_for("the echoed answer", |t| t.contains("got:abc"));
    s.wait_for("the exit note", |t| {
        t.contains("process exited with status 3")
    });
    assert_eq!(s.exit, Some(Some(3)));
    assert_eq!(s.pane.exited(), Some(Some(3)));
}

#[test]
fn the_change_context_reaches_the_child() {
    let mut s = start(
        "printf '%s|%s|%s|%s|%s\\n' \"$RB_SOURCE\" \"$RB_REPO\" \"$RB_NUMBER\" \"$RB_URL\" \"$TERM\"",
        100,
        6,
        &[
            ("RB_SOURCE", "work"),
            ("RB_REPO", "acme/widgets"),
            ("RB_NUMBER", "214"),
            ("RB_URL", "https://example.test/pull/214"),
        ],
    );
    s.wait_for("the variables", |t| {
        t.contains("work|acme/widgets|214|https://example.test/pull/214|xterm-256color")
    });
}

#[test]
fn queries_from_the_child_are_answered_through_the_pty() {
    let mut s = start(
        "stty -echo raw; printf '\\033[5n'; head -c 4 | od -An -c; printf 'done\\n'",
        60,
        6,
        &[],
    );
    s.wait_for("the reply the child read", |t| {
        t.contains("033") && t.contains('[') && t.contains("done")
    });
    let text = s.pane.screen().text();
    assert!(text.contains("0   n") || text.contains("0 n"), "{text}");
}

#[test]
fn resizing_reaches_the_child() {
    let mut s = start("read x; stty size", 40, 8, &[]);
    s.pty.resize(50, 10);
    s.pane.resize(50, 10);
    std::thread::sleep(Duration::from_millis(100));
    s.type_("\r");
    s.wait_for("the new size", |t| t.contains("10 50"));
}

#[test]
fn colours_and_wide_text_survive_the_round_trip() {
    let mut s = start("printf '\\033[1;31mred\\033[0m 世界\\n'", 40, 4, &[]);
    s.wait_for("the text", |t| t.contains("red 世界"));
    let screen = s.pane.screen();
    assert_eq!(screen.cell(0, 0).unwrap().fg, rb_term::Color::Indexed(1));
}

#[test]
fn a_missing_command_is_an_error_not_a_hang() {
    let spec = SpawnSpec {
        program: "/no/such/program".into(),
        args: vec![],
        cwd: None,
        env: vec![],
        cols: 40,
        rows: 5,
    };
    assert!(Pty::spawn(&spec, Arc::new(|_| {})).is_err());
}

#[test]
fn dropping_the_pty_stops_the_child() {
    let s = start("sleep 30", 40, 5, &[]);
    let Session { pty, rx, .. } = s;
    drop(pty);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(PtyEvent::Exited(_)) => break,
            Ok(_) => {}
            Err(_) => assert!(Instant::now() < deadline, "the child kept running"),
        }
    }
}
