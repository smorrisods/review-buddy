//! The diff screen names the change under review, in the real binary under `--demo`.
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

#[test]
fn the_diff_names_the_change_and_keeps_naming_it_after_the_review_modal() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.send_until(b"\r", "@@ -10,7 +10,9 @@", "the diff opens");
    s.expect(
        "GH liminal-hq/review-buddy#214",
        "the header names the change",
    );
    s.send_expect(b"R", "Submit your review", "the review modal opens");
    s.send_expect(
        b"\x1b",
        "GH liminal-hq/review-buddy#214",
        "esc returns to the diff",
    );
}
