//! Drives the real binary in demo mode with SGR mouse sequences: pressing on the seam between
//! Queue and Detail and dragging it moves the Detail pane, and the keys do the same.
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

fn detail_column(screen: &str) -> usize {
    for line in screen.lines() {
        if let Some(byte) = line.find("╭ liminal-hq/review-buddy#214") {
            return line[..byte].chars().count();
        }
    }
    panic!("no detail title on\n{screen}");
}

#[test]
fn dragging_the_seam_resizes_the_queue_and_double_click_resets_it() {
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
    let start = detail_column(&s.screen());
    assert_eq!(start, 74, "sources 26 + queue 48");

    // SGR coordinates are 1-based: column 75 is the seam's first column.
    s.send(b"\x1b[<0;75;12M");
    s.send_expect(
        b"\x1b[<32;85;12M",
        "queue 58 columns",
        "the drag shows its size",
    );
    assert_eq!(detail_column(&s.screen()), start + 10);
    s.send(b"\x1b[<0;85;12m");
    assert_eq!(
        detail_column(&s.screen()),
        start + 10,
        "release keeps the size"
    );

    s.send(b"\x1b[<0;85;12M");
    s.send(b"\x1b[<0;85;12m");
    s.send(b"\x1b[<0;85;12M");
    s.send_expect(b"\x1b[<0;85;12m", "(automatic)", "a double-click resets it");
    assert_eq!(detail_column(&s.screen()), start);

    s.send(b"\x1b[<0;40;3M");
    s.send(b"\x1b[<0;40;3m");
    s.send_expect(b">", "queue 50 columns", "> grows the focused pane");
    s.send(b"q");
}
