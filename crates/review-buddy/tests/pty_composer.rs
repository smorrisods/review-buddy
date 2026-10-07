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

#[test]
fn request_changes_and_comment_reviews_in_demo() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );

    s.send_until(b"\r", "@@ -10,7 +10,9 @@", "the diff opens");
    s.send_expect(b"x", "Submit your review", "x opens the review modal");
    s.expect("(•) 3 Request changes", "request changes is chosen");
    s.expect(
        "Requesting changes needs a short summary",
        "an empty summary is explained",
    );
    s.send_expect(
        b"Needs a test first",
        "Needs a test first",
        "typing fills the summary",
    );
    s.send_expect(
        b"\t",
        "› Request changes ‹",
        "one tab from the summary reaches Submit",
    );
    s.send_expect(
        b"\r",
        "Changes requested (demo)",
        "⏎ submits on the Submit button",
    );
    s.expect(
        "Your review: ✎ changes requested",
        "the review block shows the verdict",
    );

    s.send_expect(b"R", "Submit your review", "R opens it on Comment");
    s.expect("(•) 1 Comment", "comment is chosen");
    s.send_expect(b"\x1b", "Your review: ✎ changes requested", "esc closes it");

    s.send_expect(b"q", "Waiting on you", "q returns to the queue");
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)));
}

#[test]
fn tab_then_enter_submits_and_the_hint_says_so_without_kitty_keys() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.env("REVIEW_BUDDY_KITTY_KEYS", "0");
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.send_until(b"\r", "@@ -10,7 +10,9 @@", "the diff opens");
    s.send_expect(b"x", "Submit your review", "x opens the review modal");
    s.send_expect(b"Needs a test first", "Needs a test first", "typing");
    s.expect(
        "tab then ⏎ submits · ⌃P submits now",
        "the hint doesn't promise ⌃⏎ here",
    );
    s.send_expect(b"\t", "› Request changes ‹", "tab reaches Submit");
    s.send_expect(b"\r", "Changes requested (demo)", "⏎ submits");
}
