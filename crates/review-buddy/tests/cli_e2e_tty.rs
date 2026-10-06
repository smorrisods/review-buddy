//! The same commands on a terminal: glyphs, headings and colour on, and the same text with
//! `NO_COLOR` set. Forced with a pseudo-terminal, as the other `pty_*` tests do (there is no
//! hidden flag), so unix only.
#![cfg(all(unix, feature = "demo"))]

#[path = "support/cli.rs"]
mod sandbox;
use sandbox::{run_in_pty, strip_ansi, Sandbox};

const SCOPE: [&str; 4] = ["--source", "liminal-hq", "-R", "liminal-hq/review-buddy"];

fn tty(args: &[&str], colour: bool) -> (String, u32) {
    let mut all: Vec<&str> = Vec::new();
    if args[0] != "queue" {
        all.extend(SCOPE);
    }
    all.extend(args);
    run_in_pty(&Sandbox::new(), &all, colour)
}

fn check(args: &[&str], code: u32, needles: &[&str]) {
    let (raw, got) = tty(args, true);
    assert_eq!(got, code, "{args:?}\n{raw}");
    assert!(
        raw.contains("\u{1b}["),
        "{args:?}: no colour on a terminal\n{raw}"
    );
    let text = strip_ansi(&raw);
    for needle in needles {
        assert!(text.contains(needle), "{args:?}: {needle}\n{text}");
    }

    let (plain, got) = tty(args, false);
    assert_eq!(got, code, "{args:?} with NO_COLOR");
    assert!(
        !plain.contains('\u{1b}'),
        "{args:?}: NO_COLOR still coloured\n{plain}"
    );
    for needle in needles {
        assert!(
            plain.contains(needle),
            "{args:?} with NO_COLOR: {needle}\n{plain}"
        );
    }
}

#[test]
fn queue_has_a_header_row_headings_glyphs_and_an_end_note() {
    check(
        &["queue"],
        0,
        &[
            "Ref",
            "Title",
            "Waiting on you · 2",
            "Worth a look · 2",
            "◐",
            "● ",
            "That's everything.",
        ],
    );
}

#[test]
fn pr_view_is_laid_out_for_reading() {
    check(
        &["pr", "view", "214"],
        0,
        &[
            "liminal-hq/review-buddy#214 · open",
            "ada wants ada/menus → main",
            "+32 −1 · 3 files · opened 3d ago",
            "Reviewers smorris (requested), jo (commented)",
            "Checks    1 running · 2 passing",
        ],
    );
}

#[test]
fn pr_checks_has_headers_state_glyphs_and_the_running_note() {
    check(
        &["pr", "checks", "214"],
        8,
        &[
            "STATE",
            "CHECK",
            "● pass",
            "◐ running",
            "⊘ cancelled",
            "1 check is still running.",
        ],
    );
}

#[test]
fn theme_list_has_headers_and_marks_the_current_theme() {
    check(
        &["theme", "list"],
        0,
        &[
            "Id",
            "Appearance",
            "Liminal HQ",
            "dark        current",
            "Afterglow Light",
        ],
    );
}
