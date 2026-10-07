//! `⌃Z` in the real binary, in a pseudo-terminal, under `--demo`: the process stops with the
//! terminal restored, and `SIGCONT` brings the screen back. Linux only: the stop is observed
//! through `/proc/<pid>/stat`.
#![cfg(all(target_os = "linux", feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

use std::process::Command;
use std::time::{Duration, Instant};

const QUEUE: &str = "Add a menu bar and keyboard-driven menus";

fn state(pid: u32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat[stat.rfind(')')? + 1..].trim().chars().next()
}

fn wait_for_state(pid: u32, want: char) {
    let deadline = Instant::now() + pty::LIMIT;
    while Instant::now() < deadline {
        if state(pid) == Some(want) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!(
        "process {pid} never reached state {want}; it is {:?}",
        state(pid)
    );
}

fn signal(pid: u32, name: &str) {
    let status = Command::new("kill")
        .args([&format!("-{name}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

fn demo() -> (tempfile::TempDir, pty::Pty) {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    let s = pty::Pty::spawn(cmd, 160, 40);
    (home, s)
}

#[test]
fn ctrl_z_restores_the_terminal_and_sigcont_repaints_it() {
    let (_home, mut s) = demo();
    s.expect(QUEUE, "the queue loads");
    let pid = s.pid();

    s.send_until(b"\r", "@@ -10,7 +10,9 @@", "the diff opens");
    s.send_expect(b"c", "Comment · menus.rs line 10", "the composer opens");
    s.send_expect(b"keep me", "keep me", "typing shows");

    s.clear_raw();
    s.send(b"\x1a");
    wait_for_state(pid, 'T');
    for (seq, what) in [
        (&b"\x1b[?1049l"[..], "leaves the alternate screen"),
        (b"\x1b[?25h", "shows the cursor"),
        (b"\x1b[?2004l", "turns off bracketed paste"),
        (b"\x1b[?1004l", "turns off focus events"),
        (b"\x1b[?1000l", "turns off mouse capture"),
    ] {
        assert!(s.raw_contains(seq), "suspend {what}");
    }

    s.clear_raw();
    signal(pid, "CONT");
    assert!(
        s.wait_for_raw(b"\x1b[?1049h", pty::LIMIT),
        "resume re-enters the alternate screen"
    );
    s.expect("keep me", "the composer repaints with its unsent text");
    assert_ne!(state(pid), Some('T'));

    s.send_expect(b"!", "keep me!", "keys work again");
    s.send_expect(b"\x1b", "@@ -10,7 +10,9 @@", "esc closes the composer");
}

#[test]
fn ctrl_z_works_on_the_queue_and_the_process_still_quits() {
    let (_home, mut s) = demo();
    s.expect(QUEUE, "the queue loads");
    let pid = s.pid();
    s.send(b"\x1a");
    wait_for_state(pid, 'T');
    s.clear_raw();
    signal(pid, "CONT");
    assert!(s.wait_for_raw(b"\x1b[?1049h", pty::LIMIT));
    s.expect(QUEUE, "the queue repaints");
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)));
}
