//! Drives the real binary in demo mode and reads the background colour of a cell on the parsed
//! screen: `B` cycles theme, yes and no, and `REVIEW_BUDDY_BACKGROUND` starts a run painted.
#![cfg(all(unix, feature = "demo"))]

use std::time::Duration;

#[path = "support/pty.rs"]
mod pty;

use pty::{command, Pty};
use vt100::Color;

const ROWS: u16 = 40;
const COLS: u16 = 160;
const PAINTED: Color = Color::Rgb(0x05, 0x05, 0x07);

fn start(home: &std::path::Path, background: Option<&str>) -> Pty {
    let mut cmd = command(home);
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    if let Some(value) = background {
        cmd.env("REVIEW_BUDDY_BACKGROUND", value);
    }
    let pty = Pty::spawn(cmd, COLS, ROWS);
    pty.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    pty
}

fn quit(mut pty: Pty) {
    pty.send(b"q");
    assert!(pty.wait_exit(Duration::from_secs(10)));
}

#[test]
fn b_cycles_the_painted_background() {
    let home = tempfile::tempdir().unwrap();
    let mut pty = start(home.path(), None);
    let corner = (ROWS - 2, COLS - 1);
    assert_eq!(
        pty.bg_at(corner.0, corner.1),
        Color::Default,
        "Liminal HQ is transparent"
    );

    pty.send(b"B");
    pty.expect("Background: painted", "the toast names the mode");
    assert_eq!(pty.wait_for_bg(corner.0, corner.1, PAINTED), PAINTED);

    pty.send(b"B");
    pty.expect("Background: your terminal", "the next mode");
    assert_eq!(
        pty.wait_for_bg(corner.0, corner.1, Color::Default),
        Color::Default
    );

    pty.send(b"B");
    pty.expect("follows the theme", "back to the theme");
    assert_eq!(pty.bg_at(corner.0, corner.1), Color::Default);
    quit(pty);
}

#[test]
fn the_environment_starts_a_run_painted() {
    let home = tempfile::tempdir().unwrap();
    let pty = start(home.path(), Some("yes"));
    assert_eq!(pty.wait_for_bg(ROWS - 2, COLS - 1, PAINTED), PAINTED);
    quit(pty);
}
