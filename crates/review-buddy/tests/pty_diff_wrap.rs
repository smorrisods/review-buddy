//! Soft wrap in the diff, in the real binary under `--demo`: the toggle, the `↪` continuation
//! rows, and a resize that re-wraps without losing the cursor line.
#![cfg(all(unix, feature = "demo"))]

use std::time::Duration;

#[path = "support/pty.rs"]
mod pty;

/// The line numbers on the diff pane's cursor row, if there is one: they name the line whatever
/// its width.
fn cursor_line(screen: &str) -> Option<String> {
    screen.lines().find_map(|row| {
        let cells: Vec<char> = row.chars().collect();
        let at = cells.iter().skip(34).position(|&c| c == '›')? + 34;
        let rest: String = cells[at + 1..].iter().collect();
        Some(
            rest.split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" "),
        )
    })
}

#[test]
fn z_wraps_long_lines_and_a_resize_keeps_the_cursor_line() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.send_until(b"\r", "@@ -10,7 +10,9 @@", "the diff opens");
    assert!(!s.sees("↪"), "wrap is off by default");

    s.send(b"jjj");
    assert!(
        s.wait_for_screen("›        13 +", Duration::from_secs(5)),
        "the cursor moved three lines:\n{}",
        s.screen()
    );
    let before = cursor_line(&s.screen()).expect("a cursor row");
    s.send_expect(b"z", "Wrap on", "z turns wrap on and says so");
    assert!(s.sees("· wrap"), "{}", s.screen());

    s.resize(100, 30);
    assert!(
        s.wait_for_screen("↪", Duration::from_secs(5)),
        "narrow, long lines continue on the next rows:\n{}",
        s.screen()
    );
    let after = cursor_line(&s.screen()).expect("the cursor row is still on screen");
    assert_eq!(
        after,
        before,
        "the cursor stays on its line:\n{}",
        s.screen()
    );

    s.send_expect(b"z", "Wrap off", "z turns wrap off again");
    assert!(!s.sees("↪") || !s.sees("· wrap"));
    assert!(
        s.wait_for_screen("…", Duration::from_secs(5)),
        "{}",
        s.screen()
    );
}
