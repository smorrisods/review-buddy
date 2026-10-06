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

#[test]
fn unticking_a_project_changes_the_counts_and_the_rows() {
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
        s.sees("Clarify the install"),
        "the project's change is listed"
    );

    s.send_until(
        b"s",
        "Projects  4 of 4 projects shown",
        "the projects are listed",
    );
    s.send_expect(b"/", "type", "/ focuses the search");
    s.send_expect(b"notes", "1 match", "the search narrows the list");
    s.send(b"\r");
    s.send_expect(
        b"n",
        "3 of 4 projects shown",
        "n unticks the matching project",
    );
    s.expect("[ ] kai-codes/notes", "the project is unticked");
    s.send_expect(
        b"\x1b",
        "1 hidden by your Show filters: 1 by project.",
        "esc clears the search and the control stays open",
    );
    s.send(b"\x1b");
    // The end note behind the control already says "1 hidden by project", so wait for the control
    // itself to go; a `q` sent while it is still open is swallowed (and an immediate key after Esc
    // can be read as Alt+key).
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while s.sees("Changes last for this session only") {
        assert!(
            std::time::Instant::now() < deadline,
            "esc didn't close the control: {}",
            s.screen()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !s.sees("Clarify the install"),
        "the project's rows are gone"
    );
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)), "q didn't exit");
}
