//! Drives the real binary in demo mode: `p` closes and reopens the Detail pane, `S` cycles where
//! the Sources sit. Assertions run against the rendered screen (see `support/pty.rs`).
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

use std::time::Duration;

#[test]
fn the_layout_keys_change_the_rendered_dashboard() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("PATH", "");
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.expect("q quit", "the footer is up, so keys are being read");
    assert!(s.sees("sources") && s.sees("Overview"));

    s.send_expect(b"p", "p show detail", "p closes the detail pane");
    assert!(!s.sees("Overview"), "the detail pane is gone");
    assert!(s.sees("⟨ detail"));

    s.send_expect(b"S", "Sources: left", "S cycles the sources layout");
    s.send_expect(b"S", "Sources: top", "S again puts them on top");
    assert!(s.sees("1 ● All"), "the tab strip shows");
    assert!(!s.sees("╭ sources"));

    s.send_expect(b"p", "Overview", "p brings the detail pane back");
    assert!(!s.sees("p show detail"));
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)), "q didn't exit");
}
