//! Comment and approve in the real binary, in a pseudo-terminal, under `--demo`.
#![cfg(all(unix, feature = "demo"))]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

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

const LIMIT: Duration = Duration::from_secs(10);

struct Session {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
    wide: bool,
}

fn size(cols: u16) -> PtySize {
    PtySize {
        rows: 40,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

impl Session {
    /// Sends keys, then repaints (terminal output is a stream of cell updates, so a resize is
    /// the way to see the whole screen) until `needle` shows or the limit passes.
    fn send(&mut self, bytes: &[u8], needle: &str, why: &str) {
        // Keys written right on the heels of a resize can be lost by the pty, so let it settle.
        std::thread::sleep(Duration::from_millis(250));
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
        self.seen.clear();
        let deadline = Instant::now() + LIMIT;
        while Instant::now() < deadline {
            if wait_for(&self.rx, &mut self.seen, needle, Duration::from_millis(400)) {
                return;
            }
            self.wide = !self.wide;
            self.master
                .resize(size(if self.wide { 161 } else { 160 }))
                .unwrap();
        }
        panic!("{why}: {:?}", String::from_utf8_lossy(&self.seen));
    }

    fn expect(&mut self, needle: &str, why: &str) {
        assert!(
            wait_for(&self.rx, &mut self.seen, needle, LIMIT),
            "{why}: {:?}",
            String::from_utf8_lossy(&self.seen)
        );
    }
}

#[test]
fn comment_then_approve_in_demo_and_quit_cleanly() {
    let home = tempfile::tempdir().unwrap();
    let pair = native_pty_system().openpty(size(160)).unwrap();
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
    let mut child = pair.slave.spawn_command(cmd).unwrap();
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
        master: pair.master,
        writer,
        rx,
        seen: Vec::new(),
        wide: false,
    };
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );

    s.send(b"\r", "@@ -10,7 +10,9 @@", "the diff opens");
    s.send(b"c", "Comment · menus.rs line 10", "c opens the composer");
    s.send(b"Needs a guard here", "Needs a guard here", "typing shows");
    s.send(
        b"\r",
        "pending comment · line 10",
        "⏎ adds it to the review",
    );
    s.send(
        b"",
        "1 suggestion · 1 comment",
        "the review block counts it",
    );
    s.send(b"a", "Approve with 2 comments", "a previews the approval");
    s.send(b"", "Verdict: approve", "the preview names the verdict");
    s.send(b"\r", "Approved (demo)", "⏎ confirms");

    // Back out of the diff, then quit: nothing is unsent, so there is no prompt.
    s.send(b"q", "Waiting on you", "q returns to the queue");
    s.writer.write_all(b"q").unwrap();
    s.writer.flush().unwrap();
    let deadline = Instant::now() + LIMIT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "did not exit after q");
        let _ = s.rx.recv_timeout(Duration::from_millis(50));
    };
    assert!(status.success());
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}
