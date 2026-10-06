//! `source list` and `auth status` on a terminal: an aligned table with a header and colour.
#![cfg(all(unix, feature = "demo"))]

use std::io::Read;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

fn on_terminal(args: &[&str]) -> String {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.args(args);
    cmd.env("TERM", "xterm-256color");
    cmd.env_remove("NO_COLOR");
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
    out
}

#[test]
fn source_list_is_a_headed_table_on_a_terminal() {
    let out = on_terminal(&["source", "list"]);
    assert!(out.contains("Name") && out.contains("Host"), "{out:?}");
    assert!(out.contains("These are demo sources (demo)"), "{out:?}");
    assert!(!out.contains('\t'), "{out:?}");
}

#[test]
fn auth_status_marks_signed_in_sources_with_a_glyph() {
    let out = on_terminal(&["auth", "status"]);
    assert!(out.contains("✓"), "{out:?}");
    assert!(
        out.contains("signed in as smorris via demo (demo)"),
        "{out:?}"
    );
}
