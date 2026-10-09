//! Drives the real binary in demo mode: with Detail closed (`p`) the queue has room for the status
//! cluster, and the rows show review state, comments, open threads, CI in words and size.
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

use std::time::Duration;

#[test]
fn closing_detail_gives_the_queue_room_for_the_status_cluster() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("PATH", "");
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    assert!(
        !s.sees("CI running"),
        "the 48-column queue beside Detail has no room for the cluster"
    );

    s.send_until(b"p", "CI running", "closing Detail widens the queue");
    for (needle, why) in [
        ("✓1", "approvals from other people"),
        ("✕1", "a change request"),
        ("○1", "an outstanding reviewer"),
        ("¶3", "the comment count"),
        ("2 open", "unresolved threads"),
        ("CI failing", "CI in words"),
        ("+32 −1", "size"),
    ] {
        s.expect(needle, why);
    }
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)), "q didn't exit");
}
