//! `pr diff` on a terminal: coloured, with the pager.
#![cfg(all(unix, feature = "demo"))]

use std::io::Read;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

fn run_on_tty(extra: &[&str], envs: &[(&str, &str)]) -> String {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args([
        "--demo",
        "--frozen-time",
        "2026-10-05T10:00",
        "-s",
        "liminal-hq",
        "-R",
        "liminal-hq/review-buddy",
        "pr",
        "diff",
        "214",
    ]);
    cmd.args(extra);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("REVIEW_BUDDY_PAGER", "cat");
    cmd.env_remove("NO_COLOR");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    child.wait().unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn a_terminal_gets_colour_and_the_patch_flag_turns_it_off() {
    let coloured = run_on_tty(&[], &[]);
    assert!(coloured.contains("\u{1b}["), "{coloured:?}");
    assert!(coloured.contains("src/ui/menus.rs"));

    let raw = run_on_tty(&["--patch"], &[]);
    assert!(raw.contains("+++ b/src/ui/menus.rs"), "{raw:?}");
    assert!(!raw.contains("\u{1b}["), "{raw:?}");
}

#[test]
fn no_color_keeps_a_terminal_plain() {
    let out = run_on_tty(&[], &[("NO_COLOR", "1")]);
    assert!(out.contains("+++ b/src/ui/menus.rs"));
    assert!(!out.contains("\u{1b}["), "{out:?}");
}
