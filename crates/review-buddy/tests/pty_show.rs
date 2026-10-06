//! Drives the real binary in demo mode: `s` opens the Show filters, and toggles change the queue.
//! Assertions run against the rendered screen (see `support/pty.rs`).
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

use std::time::Duration;

#[test]
fn the_show_control_toggles_filters_and_counts_change() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    // Even if the demo tried to open something, nothing real could answer.
    cmd.env("PATH", "");
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );

    s.send_until(b"s", "this session only", "s opens the Show filters");
    for (key, needle) in [
        (&b"3"[..], "1 hidden by your Show filters."),
        (b"4", "[x] drafts"),
        (b"5", "[x] noise"),
        (b"\x1b", "1 hidden by your Show filters"),
    ] {
        s.send_expect(key, needle, needle);
    }
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)), "q didn't exit");
}
