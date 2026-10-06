//! The terminal-only paths of `auth token` and `auth logout`, driven in a pseudo-terminal.
#![cfg(unix)]

use std::io::{Read, Write};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const SECRET: &str = "ghp_pty_secret_value_77";

/// Runs the binary on a terminal, typing `input` first, and returns what it printed and its code.
fn on_a_tty(args: &[&str], input: &str) -> (String, u32) {
    let home = tempfile::tempdir().unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(args);
    cmd.env_clear();
    for key in ["PATH", "TMPDIR"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.env("HOME", home.path());
    cmd.env("XDG_CONFIG_HOME", home.path().join("config"));
    cmd.env("XDG_DATA_HOME", home.path().join("data"));
    cmd.env("XDG_CACHE_HOME", home.path().join("cache"));
    cmd.env("XDG_STATE_HOME", home.path().join("state"));
    cmd.env("TERM", "xterm-256color");
    cmd.env("NO_COLOR", "1");
    cmd.env("REVIEW_BUDDY_TEST_KEYRING", format!("ghe.test={SECRET}"));
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut writer = pair.master.take_writer().unwrap();
    writer.write_all(input.as_bytes()).unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut out = String::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    let code = child.wait().unwrap().exit_code();
    (out.replace('\r', ""), code)
}

#[test]
fn token_refuses_a_terminal_without_show_and_prints_nothing_secret() {
    let (out, code) = on_a_tty(&["auth", "token", "--host", "ghe.test"], "");
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("Not printing a token to a terminal"), "{out}");
    assert!(out.contains("--show"), "{out}");
    assert!(!out.contains(SECRET), "{out}");
}

#[cfg(feature = "live")]
#[test]
fn token_with_show_prints_it_on_a_terminal() {
    let (out, code) = on_a_tty(&["auth", "token", "--host", "ghe.test", "--show"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(SECRET), "{out}");
}

#[cfg(feature = "live")]
#[test]
fn logout_defaults_to_no_on_a_terminal() {
    let (out, code) = on_a_tty(&["auth", "logout", "--host", "ghe.test"], "\n");
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("[y/N]"), "{out}");
    assert!(out.contains("Cancelled. Nothing was changed."), "{out}");
}

#[cfg(feature = "live")]
#[test]
fn logout_removes_it_after_an_explicit_yes() {
    let (out, code) = on_a_tty(&["auth", "logout", "--host", "ghe.test"], "y\n");
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("Removed the stored token for ghe.test"),
        "{out}"
    );
    assert!(!out.contains(SECRET), "{out}");
}

#[test]
fn source_add_defaults_to_no_on_a_terminal() {
    let (out, code) = on_a_tty(&["source", "add", "--host", "ghe.test", "--no-test"], "\n");
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("[[source]]") && out.contains("[y/N]"), "{out}");
}
