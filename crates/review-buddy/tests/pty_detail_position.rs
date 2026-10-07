//! Drives the real binary in demo mode: `P` cycles where the Detail pane sits, and the
//! rendered screen shows it in the new place each time.
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

use std::time::Duration;

fn position(screen: &str, needle: &str) -> (usize, usize) {
    for (row, line) in screen.lines().enumerate() {
        if let Some(byte) = line.find(needle) {
            return (row, line[..byte].chars().count());
        }
    }
    panic!("no {needle:?} on\n{screen}");
}

#[test]
fn p_moves_the_detail_pane_around_the_queue() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("PATH", "");
    cmd.env_remove("REVIEW_BUDDY_DETAIL_POSITION");
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.expect("q quit", "the footer is up, so keys are being read");

    let beside = |screen: &str| {
        let (qr, qc) = position(screen, "╭ queue");
        let (dr, dc) = position(screen, "Overview");
        (qr, qc, dr, dc)
    };

    let (qr, qc, dr, dc) = beside(&s.screen());
    assert!(dr > qr && dc > qc, "auto is right at 160 columns");

    s.send_expect(b"P", "Detail position: right", "P goes to right");
    let (qr, qc, _, dc) = beside(&s.screen());
    assert!(dc > qc && qr >= 1);

    s.send_expect(b"P", "Detail position: left", "P goes to left");
    let (_, qc, _, dc) = beside(&s.screen());
    assert!(dc < qc, "detail is left of the queue");

    s.send_expect(b"P", "Detail position: top", "P goes to top");
    let (qr, _, dr, _) = beside(&s.screen());
    assert!(dr < qr, "detail is above the queue");

    s.send_expect(b"P", "Detail position: bottom", "P goes to bottom");
    let (qr, _, dr, _) = beside(&s.screen());
    assert!(dr > qr, "detail is below the queue");
    assert!(s.sees("▼"), "the close marker points down");

    s.send_expect(b"P", "Detail position: auto", "P returns to auto");
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)), "q didn't exit");
}
