//! Comment and approve in the real binary, in a pseudo-terminal, under `--demo`. Assertions run
//! against the rendered screen (see `support/pty.rs`).
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

use std::time::Duration;

#[test]
fn comment_then_approve_in_demo_and_quit_cleanly() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );

    s.send_until(b"\r", "@@ -10,7 +10,9 @@", "the diff opens");
    s.send_expect(b"c", "Comment · menus.rs line 10", "c opens the composer");
    s.send_expect(b"Needs a guard here", "Needs a guard here", "typing shows");
    s.send_expect(
        b"\r",
        "pending comment · line 10",
        "⏎ adds it to the review",
    );
    s.expect("1 suggestion · 1 comment", "the review block counts it");
    s.send_expect(b"a", "Approve with 2 comments", "a previews the approval");
    s.expect("Verdict: approve", "the preview names the verdict");
    s.send_expect(b"\r", "Approved (demo)", "⏎ confirms");

    // Back out of the diff, then quit: nothing is unsent, so there is no prompt.
    s.send_expect(b"q", "Waiting on you", "q returns to the queue");
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)));
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}
