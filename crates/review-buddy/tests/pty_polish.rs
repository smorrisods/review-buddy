//! Dashboard approve and range comments in the real binary, in a pseudo-terminal, under `--demo`.
#![cfg(all(unix, feature = "demo"))]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const LIMIT: Duration = Duration::from_secs(10);

struct Session {
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _home: tempfile::TempDir,
}

fn start() -> Session {
    let home = tempfile::tempdir().unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("NO_COLOR");
    cmd.env("HOME", home.path());
    for var in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        cmd.env(var, home.path().join(var));
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
    let mut s = Session {
        writer,
        rx,
        seen: Vec::new(),
        child,
        _home: home,
    };
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s
}

impl Session {
    fn expect(&mut self, needle: &str, why: &str) {
        let deadline = Instant::now() + LIMIT;
        while Instant::now() < deadline {
            if String::from_utf8_lossy(&self.seen).contains(needle) {
                return;
            }
            if let Ok(chunk) = self.rx.recv_timeout(Duration::from_millis(100)) {
                self.seen.extend(chunk);
            }
        }
        let all = String::from_utf8_lossy(&self.seen);
        let tail: String = all
            .chars()
            .rev()
            .take(800)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        panic!("{why}: …{tail:?}");
    }

    fn send(&mut self, bytes: &[u8], needle: &str, why: &str) {
        std::thread::sleep(Duration::from_millis(300));
        self.seen.clear();
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
        self.expect(needle, why);
    }

    fn quit(&mut self) {
        std::thread::sleep(Duration::from_millis(300));
        self.writer.write_all(b"q").unwrap();
        self.writer.flush().unwrap();
        let deadline = Instant::now() + LIMIT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(Instant::now() < deadline, "did not exit after q");
            let _ = self.rx.recv_timeout(Duration::from_millis(50));
        }
    }
}

fn sgr(button: u8, (x, y): (u16, u16), press: bool) -> Vec<u8> {
    format!(
        "\x1b[<{button};{};{}{}",
        x + 1,
        y + 1,
        if press { 'M' } else { 'm' }
    )
    .into_bytes()
}

#[test]
fn a_on_the_dashboard_previews_the_approval_in_the_diff() {
    let mut s = start();
    s.send(b"a", "Approve this change?", "a opens the approve preview");
    s.send(b"\r", "Approved (demo)", "⏎ confirms");
    s.send(b"q", "Waiting on you", "q returns to the queue");
    s.quit();
}

#[test]
fn a_dragged_range_becomes_a_range_comment() {
    let mut s = start();
    s.send(b"\r", "@@ -10,7 +10,9 @@", "the diff opens");
    let mut drag = sgr(0, (60, 2), true);
    drag.extend(sgr(32, (60, 5), true));
    drag.extend(sgr(0, (60, 5), false));
    s.send(&drag, "▌", "dragging marks the selected lines");
    s.send(b"c", "Comment · menus.rs lines", "c anchors a range");
    s.send(b"Both of these", "oth of these", "typing shows");
    s.send(b"\r", "pending comment · lines", "⏎ adds the range");
    s.send(b"q", "Waiting on you", "q returns to the queue");
    s.quit();
}
