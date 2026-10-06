//! `pr view` and `open` on a real terminal.
#![cfg(all(unix, feature = "demo"))]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

struct Session {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
    _home: tempfile::TempDir,
}

impl Session {
    fn start(args: &[&str], pager: Option<&str>) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
        cmd.env_clear();
        cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
        cmd.env("HOME", home.path());
        cmd.env("XDG_CONFIG_HOME", home.path().join("config"));
        cmd.env("XDG_DATA_HOME", home.path().join("data"));
        cmd.env("XDG_CACHE_HOME", home.path().join("cache"));
        cmd.env("XDG_STATE_HOME", home.path().join("state"));
        cmd.env("TERM", "xterm-256color");
        cmd.env("NO_COLOR", "1");
        if let Some(pager) = pager {
            cmd.env("REVIEW_BUDDY_PAGER", pager);
        }
        cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
        cmd.args(["-s", "liminal-hq", "-R", "liminal-hq/review-buddy"]);
        cmd.args(args);
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
        Self {
            child,
            writer,
            rx,
            seen: Vec::new(),
            _home: home,
        }
    }

    fn wait_for(&mut self, needle: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if String::from_utf8_lossy(&self.seen).contains(needle) {
                return true;
            }
            if let Ok(chunk) = self.rx.recv_timeout(Duration::from_millis(100)) {
                self.seen.extend(chunk);
            }
        }
        String::from_utf8_lossy(&self.seen).contains(needle)
    }

    fn finish(mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            if let Ok(chunk) = self.rx.recv_timeout(Duration::from_millis(100)) {
                self.seen.extend(chunk);
            }
        }
        while let Ok(chunk) = self.rx.try_recv() {
            self.seen.extend(chunk);
        }
        String::from_utf8_lossy(&self.seen).into_owned()
    }
}

#[test]
fn view_goes_through_the_pager_on_a_terminal() {
    let mut session = Session::start(&["pr", "view", "214"], Some("sed s/^/PAGED:/"));
    assert!(session.wait_for("PAGED:Add a menu bar and keyboard-driven menus"));
    let text = session.finish();
    assert!(text.contains("PAGED:Reviewers"), "{text}");
}

#[test]
fn cat_turns_the_pager_off() {
    let mut session = Session::start(&["pr", "view", "214"], Some("cat"));
    assert!(session.wait_for("Add a menu bar and keyboard-driven menus"));
    assert!(!session.finish().contains("PAGED:"));
}

#[test]
fn open_starts_on_the_diff_for_the_selector() {
    let mut session = Session::start(&["open", "214"], None);
    assert!(session.wait_for("menus.rs"), "{:?}", session.seen);
    assert!(!String::from_utf8_lossy(&session.seen).contains("Waiting on you"));
    session.writer.write_all(b"q").unwrap();
    session.writer.write_all(b"q").unwrap();
    let _ = session.finish();
}
