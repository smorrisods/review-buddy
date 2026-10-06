//! A pseudo-terminal session whose output feeds a `vt100` parser, so assertions run against the
//! rendered screen (`screen.contents()`) instead of substring-matching raw escape-sequence
//! bytes. Include it with `#[path = "support/pty.rs"] mod pty;`.
//!
//! The reader thread parses output as it arrives, so the screen is always whole: no repaint
//! nudges, and a word split across cell updates still reads as one word.
#![allow(dead_code)]
#![cfg(unix)]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

pub const LIMIT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(25);

pub struct Pty {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
}

/// A command with a cleared environment and the variables every pty test sets: a colour terminal and a sandboxed home with
/// XDG directories under it. Callers add their own `PATH`, arguments and extras.
pub fn command(home: &Path) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.env_clear();
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("HOME", home);
    for var in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        cmd.env(var, home.join(var));
    }
    cmd
}

impl Pty {
    pub fn spawn(cmd: CommandBuilder, cols: u16, rows: u16) -> Pty {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let feed = Arc::clone(&parser);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                feed.lock().unwrap().process(&buf[..n]);
            }
        });
        Pty {
            child,
            master: pair.master,
            writer,
            parser,
        }
    }

    pub fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        self.parser
            .lock()
            .unwrap()
            .screen_mut()
            .set_size(rows, cols);
    }

    /// The visible text of the screen, rows joined by newlines.
    pub fn screen(&self) -> String {
        self.parser.lock().unwrap().screen().contents()
    }

    /// The background colour of the cell at `row`, `col` on the parsed screen.
    pub fn bg_at(&self, row: u16, col: u16) -> vt100::Color {
        let parser = self.parser.lock().unwrap();
        parser.screen().cell(row, col).unwrap().bgcolor()
    }

    /// Polls until the cell's background is `want`; returns the last colour seen.
    pub fn wait_for_bg(&self, row: u16, col: u16, want: vt100::Color) -> vt100::Color {
        let deadline = Instant::now() + LIMIT;
        loop {
            let got = self.bg_at(row, col);
            if got == want || Instant::now() >= deadline {
                return got;
            }
            std::thread::sleep(POLL);
        }
    }

    pub fn sees(&self, needle: &str) -> bool {
        self.screen().contains(needle)
    }

    /// Polls the parsed screen until `needle` shows. Returns whether it did within `limit`.
    pub fn wait_for_screen(&self, needle: &str, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            if self.sees(needle) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(POLL);
        }
    }

    /// Like `wait_for_screen` with the default limit, panicking with the screen on failure.
    pub fn expect(&self, needle: &str, why: &str) {
        assert!(
            self.wait_for_screen(needle, LIMIT),
            "{why}: waiting for {needle:?}, the screen was:\n{}",
            self.screen()
        );
    }

    /// Sends keys, then waits for `needle` to show on the screen.
    pub fn send_expect(&mut self, bytes: &[u8], needle: &str, why: &str) {
        self.send(bytes);
        self.expect(needle, why);
    }

    /// Sends an idempotent key, resending every couple of seconds until `needle` shows, for the
    /// first key after start-up when a loaded runner may not be reading input yet.
    pub fn send_until(&mut self, bytes: &[u8], needle: &str, why: &str) {
        let deadline = Instant::now() + LIMIT;
        while Instant::now() < deadline {
            self.send(bytes);
            if self.wait_for_screen(needle, Duration::from_secs(2)) {
                return;
            }
        }
        self.expect(needle, why);
    }

    /// Waits for the child to exit, returning whether it succeeded.
    pub fn wait_exit(&mut self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.success();
            }
            assert!(
                Instant::now() < deadline,
                "did not exit; the screen was:\n{}",
                self.screen()
            );
            std::thread::sleep(POLL);
        }
    }
}
