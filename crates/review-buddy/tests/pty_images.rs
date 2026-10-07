//! Drives the real binary in demo mode to see the pictures in a description: a terminal that
//! never answers the capability query still starts and shows notes or halfblocks, a terminal
//! that does answer gets sixel, and `i` then `o` offers the selected picture.
//! Assertions run against the rendered screen (see `support/pty.rs`).
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

use std::time::Duration;

use portable_pty::CommandBuilder;

const BEFORE: &str = "▣ image: Muted text before the change";

fn demo(home: &std::path::Path, tweak: impl FnOnce(&mut CommandBuilder)) -> pty::Pty {
    let mut cmd = pty::command(home);
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    // Even if the demo tried to open something, nothing real could answer.
    cmd.env("PATH", "");
    tweak(&mut cmd);
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.send_until(b"j", "Muted text dipped under", "j selects the next change");
    s
}

fn quit(mut s: pty::Pty) {
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)), "q didn't exit");
}

#[test]
fn a_silent_256_colour_terminal_gets_notes_and_never_hangs() {
    let home = tempfile::tempdir().unwrap();
    let s = demo(home.path(), |cmd| cmd.env_remove("COLORTERM"));
    s.expect(
        &format!("{BEFORE} – can't be drawn in this terminal"),
        "the picture is a note",
    );
    assert!(
        !s.sees('\u{2584}'.to_string().as_str()),
        "no halfblocks without truecolour"
    );
    quit(s);
}

#[test]
fn a_silent_truecolour_terminal_gets_halfblocks() {
    let home = tempfile::tempdir().unwrap();
    let s = demo(home.path(), |_| {});
    s.expect(BEFORE, "the caption shows once the picture loads");
    s.expect("\u{2584}", "the picture is drawn in halfblocks");
    quit(s);
}

#[test]
fn images_off_means_notes_whatever_the_terminal() {
    let home = tempfile::tempdir().unwrap();
    let s = demo(home.path(), |cmd| cmd.env("REVIEW_BUDDY_IMAGES", "off"));
    s.expect("can't be drawn in this terminal", "the picture is a note");
    assert!(!s.sees("\u{2584}"));
    quit(s);
}

#[test]
fn no_color_means_notes() {
    let home = tempfile::tempdir().unwrap();
    let s = demo(home.path(), |cmd| cmd.env("NO_COLOR", "1"));
    s.expect("can't be drawn in this terminal", "the picture is a note");
    assert!(!s.sees("\u{2584}"));
    quit(s);
}

#[test]
fn a_terminal_that_answers_the_query_is_drawn_to_in_sixel() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("PATH", "");
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    assert!(
        s.wait_for_raw(b"\x1b[5n", pty::LIMIT),
        "the app asks the terminal what it can do"
    );
    s.send(b"\x1b[?64;4;6c\x1b[6;20;10t\x1b[0n");
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.send_until(b"j", "Muted text dipped under", "j selects the next change");
    assert!(
        s.wait_for_raw(b"\x1bP", pty::LIMIT),
        "a sixel image is sent"
    );
    quit(s);
}

#[test]
fn i_then_o_offers_the_selected_picture_without_touching_anything() {
    let home = tempfile::tempdir().unwrap();
    let mut s = demo(home.path(), |_| {});
    s.send_expect(
        b"i",
        "Image 1 of 2: Muted text before the change",
        "i selects the first picture",
    );
    s.send_expect(
        b"o",
        "Would open https://demo.invalid/screenshots/contrast-before.png (demo)",
        "o offers the picture's address",
    );
    quit(s);
}
