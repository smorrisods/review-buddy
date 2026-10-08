//! Text selection in the real binary under `--demo`, in a pseudo-terminal whose output feeds a
//! `vt100` parser: an SGR drag over code copies through OSC 52 and highlights exactly the
//! selected cells, esc clears it, and `M` hands the mouse back to the terminal and takes it again.
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

use std::time::Duration;

use rb_theme::ColourDepth;
use review_buddy::app::{App, AppConfig};
use review_buddy::demo::DEFAULT_FROZEN;
use review_buddy::ui;

fn sgr(button: u8, (x, y): (u16, u16), press: bool) -> Vec<u8> {
    format!(
        "\x1b[<{button};{};{}{}",
        x + 1,
        y + 1,
        if press { 'M' } else { 'm' }
    )
    .into_bytes()
}

fn launch(home: &std::path::Path) -> pty::Pty {
    let mut cmd = pty::command(home);
    cmd.args(["--demo", "--frozen-time", DEFAULT_FROZEN]);
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.send_until(b"\r", "pub struct Menu", "enter opens the diff");
    s
}

fn highlight() -> vt100::Color {
    let app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (160, 40),
    });
    match ui::style::text_selection(&app.palette).bg {
        Some(ratatui::style::Color::Rgb(r, g, b)) => vt100::Color::Rgb(r, g, b),
        other => panic!("a truecolour highlight, got {other:?}"),
    }
}

#[test]
fn an_sgr_drag_over_code_copies_over_osc52_and_highlights_the_cells() {
    let home = tempfile::tempdir().unwrap();
    let mut s = launch(home.path());
    s.clear_raw();

    // Row 5 is `pub struct Menu {` and its code starts at column 49. Pressing on the 2nd
    // character and releasing on the 9th selects `b struct`.
    let mut drag = sgr(0, (51, 5), true);
    drag.extend(sgr(32, (58, 5), true));
    drag.extend(sgr(0, (58, 5), false));
    s.send(&drag);

    s.expect("Copied 8 characters", "a toast names how many, not what");
    assert!(
        s.raw_contains(b"\x1b]52;c;YiBzdHJ1Y3Q="),
        "OSC 52 carries `b struct`"
    );

    let on = highlight();
    for col in 51..=58 {
        assert_eq!(s.wait_for_bg(5, col, on), on, "column {col} is selected");
    }
    assert_ne!(s.bg_at(5, 50), on, "the cell before is not");
    assert_ne!(s.bg_at(5, 59), on, "the cell after is not");
    assert_ne!(s.bg_at(5, 40), on, "the gutter is not");

    s.send(b"\x1b");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while s.bg_at(5, 53) == on && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_ne!(s.bg_at(5, 53), on, "esc clears the highlight");
    assert!(s.sees("pub struct Menu"), "and stays on the diff");
}

#[test]
fn m_hands_the_mouse_to_the_terminal_and_back() {
    let home = tempfile::tempdir().unwrap();
    let mut s = launch(home.path());
    s.clear_raw();
    s.send(b"M");
    assert!(
        s.wait_for_raw(b"\x1b[?1000l", Duration::from_secs(10)),
        "mouse reporting is switched off"
    );
    s.expect("ouse off", "the footer says so");
    s.clear_raw();
    s.send(b"M");
    assert!(
        s.wait_for_raw(b"\x1b[?1000h", Duration::from_secs(10)),
        "and on again"
    );
}
