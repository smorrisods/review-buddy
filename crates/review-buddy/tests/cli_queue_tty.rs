//! `queue` and `pr list` on a terminal: headings, glyphs and the calm notes on stderr.
#![cfg(all(unix, feature = "demo"))]

use std::io::Read;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

fn run_in_pty(args: &[&str]) -> String {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.args(args);
    cmd.env("TERM", "xterm-256color");
    cmd.env_remove("NO_COLOR");
    cmd.env_remove("REVIEW_BUDDY_SOURCE");
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut out = String::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    child.wait().unwrap();
    strip(&out)
}

fn strip(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out.replace('\r', "")
}

#[test]
fn queue_shows_headings_glyphs_and_the_end_note() {
    let text = run_in_pty(&["queue"]);
    for heading in ["Waiting on you · 2", "Worth a look · 2", "Can wait · 1"] {
        assert!(text.contains(heading), "{text}");
    }
    assert!(text.contains("review-buddy#214"), "{text}");
    assert!(!text.contains("liminal-hq/review-buddy#214"), "{text}");
    assert!(text.contains('◐') && text.contains('✕'), "{text}");
    assert!(text.contains("2h"), "{text}");
    assert!(text.contains("── That's everything."), "{text}");
    assert!(text.contains("2 hidden by your Show filters."), "{text}");
    assert!(!text.contains('\t'), "{text}");
}

#[test]
fn empty_queue_says_so_calmly() {
    let text = run_in_pty(&["queue", "--source", "smorris", "--show", "reviewing"]);
    assert!(text.contains("Your Show filters hide all 1."), "{text}");
}

#[test]
fn pr_list_aligns_columns_and_notes_empty_results() {
    let text = run_in_pty(&["pr", "list", "-L", "2"]);
    assert!(text.contains("Source"), "{text}");
    assert!(text.contains("Title"), "{text}");
    assert!(text.contains("2h"), "{text}");
    let empty = run_in_pty(&["pr", "list", "--search", "no-such-change"]);
    assert!(empty.contains("Nothing matches."), "{empty}");
}
