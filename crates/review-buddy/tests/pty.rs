//! Drives the real binary in a pseudo-terminal.
#![cfg(unix)]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

fn wait_for(
    rx: &mpsc::Receiver<Vec<u8>>,
    seen: &mut Vec<u8>,
    needle: &str,
    limit: Duration,
) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if String::from_utf8_lossy(seen).contains(needle) {
            return true;
        }
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
            seen.extend(chunk);
        }
    }
    String::from_utf8_lossy(seen).contains(needle)
}

#[test]
fn starts_draws_the_wordmark_and_quits_on_q() {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("NO_COLOR");
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut seen = Vec::new();
    // The wordmark is drawn one character at a time, so look for its first word's
    // tail and the footer hint rather than the whole string.
    assert!(
        wait_for(
            &rx,
            &mut seen,
            "Nothing connected yet.",
            Duration::from_secs(10)
        ),
        "output so far: {:?}",
        String::from_utf8_lossy(&seen)
    );
    let screen = String::from_utf8_lossy(&seen).into_owned();
    assert!(
        screen.contains("\u{1b}[?1049h"),
        "enters the alternate screen"
    );
    for ch in "review buddy".chars().filter(|c| !c.is_whitespace()) {
        assert!(screen.contains(ch));
    }

    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "did not exit after q");
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
            seen.extend(chunk);
        }
    };
    assert!(status.success());
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
        seen.extend(chunk);
    }
    let out = String::from_utf8_lossy(&seen);
    assert!(out.contains("\u{1b}[?1049l"), "leaves the alternate screen");
}

#[cfg(feature = "demo")]
#[test]
fn demo_mode_loads_the_fixtures_and_touches_no_real_directories() {
    let home = tempfile::tempdir().unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("NO_COLOR");
    cmd.env("HOME", home.path());
    for var in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        cmd.env(var, home.path().join(var));
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut seen = Vec::new();
    assert!(
        wait_for(&rx, &mut seen, "7 changes", Duration::from_secs(10)),
        "output so far: {:?}",
        String::from_utf8_lossy(&seen)
    );
    let screen = String::from_utf8_lossy(&seen).into_owned();
    assert!(screen.contains("demo · 4 sources"));
    assert!(!screen.contains("Nothing connected yet."));
    assert!(
        wait_for(&rx, &mut seen, "Waiting on you", Duration::from_secs(10)),
        "bucket heading missing: {:?}",
        String::from_utf8_lossy(&seen)
    );
    assert!(wait_for(
        &rx,
        &mut seen,
        "Add a menu bar and keyboard-driven menus",
        Duration::from_secs(10)
    ));

    seen.clear();
    writer.write_all(b"j").unwrap();
    writer.flush().unwrap();
    assert!(
        wait_for(&rx, &mut seen, "Raise", Duration::from_secs(10)),
        "j moves the selection and the detail follows: {:?}",
        String::from_utf8_lossy(&seen)
    );
    writer.write_all(b"\t").unwrap();
    writer.flush().unwrap();
    std::thread::sleep(Duration::from_millis(200));

    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "did not exit after q");
        let _ = rx.recv_timeout(Duration::from_millis(50));
    };
    assert!(status.success());
    assert_eq!(
        std::fs::read_dir(home.path()).unwrap().count(),
        0,
        "demo mode must not create anything under the real HOME or XDG directories"
    );
}

#[cfg(feature = "demo")]
#[test]
fn demo_diff_opens_navigates_and_returns_to_the_dashboard() {
    let home = tempfile::tempdir().unwrap();
    let size = |cols: u16| PtySize {
        rows: 40,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = native_pty_system().openpty(size(160)).unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("NO_COLOR");
    cmd.env("HOME", home.path());
    for var in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        cmd.env(var, home.path().join(var));
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let limit = Duration::from_secs(10);
    let mut seen = Vec::new();
    assert!(wait_for(
        &rx,
        &mut seen,
        "Add a menu bar and keyboard-driven menus",
        limit
    ));

    // Terminal output is a stream of cell updates, so a resize forces a full repaint to look at.
    let mut wide = false;
    let mut repaint = |seen: &mut Vec<u8>| {
        wide = !wide;
        pair.master
            .resize(size(if wide { 161 } else { 160 }))
            .unwrap();
        let _ = seen;
    };

    seen.clear();
    writer.write_all(b"\r").unwrap();
    writer.flush().unwrap();
    assert!(
        wait_for(&rx, &mut seen, "@@ -10,7 +10,9 @@", limit),
        "the diff opens: {:?}",
        String::from_utf8_lossy(&seen)
    );
    assert!(
        wait_for(&rx, &mut seen, "thread · line 44", limit),
        "the thread block shows: {:?}",
        String::from_utf8_lossy(&seen)
    );
    assert!(wait_for(&rx, &mut seen, "jo · 1d", limit));

    writer.write_all(b"j").unwrap();
    writer.write_all(b"n").unwrap();
    writer.flush().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    seen.clear();
    repaint(&mut seen);
    assert!(
        wait_for(&rx, &mut seen, "hunk 2 of 3 · file 1 of 3", limit),
        "n moves to the next hunk: {:?}",
        String::from_utf8_lossy(&seen)
    );

    writer.write_all(b"]").unwrap();
    writer.flush().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    seen.clear();
    repaint(&mut seen);
    assert!(
        wait_for(&rx, &mut seen, "file 2 of 3", limit),
        "] opens the next file: {:?}",
        String::from_utf8_lossy(&seen)
    );
    assert!(wait_for(&rx, &mut seen, " src/ui/menubar.rs ", limit));

    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    // A lone escape byte is held back briefly while the terminal decides it isn't a sequence.
    std::thread::sleep(Duration::from_millis(300));
    seen.clear();
    repaint(&mut seen);
    assert!(
        wait_for(&rx, &mut seen, "Waiting on you", limit),
        "esc returns to the dashboard: {:?}",
        String::from_utf8_lossy(&seen)
    );

    std::thread::sleep(Duration::from_millis(200));
    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "did not exit after q");
        let _ = rx.recv_timeout(Duration::from_millis(50));
    };
    assert!(status.success());
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[cfg(feature = "demo")]
/// Runs a command to completion in a pseudo-terminal and returns its exit code and output.
fn run_to_exit(args: &[&str], env: &[(&str, &str)], cols: u16) -> (u32, String) {
    let home = tempfile::tempdir().unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(args);
    cmd.env_clear();
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    cmd.env("HOME", home.path());
    cmd.env("XDG_CONFIG_HOME", home.path().join("config"));
    cmd.env("XDG_DATA_HOME", home.path().join("data"));
    cmd.env("XDG_CACHE_HOME", home.path().join("cache"));
    cmd.env("XDG_STATE_HOME", home.path().join("state"));
    cmd.env("XDG_CONFIG_DIRS", home.path().join("etc"));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "command did not exit");
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(pair.master);
    let mut seen = Vec::new();
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(300)) {
        seen.extend(chunk);
    }
    (
        status.exit_code(),
        String::from_utf8_lossy(&seen).into_owned(),
    )
}

#[cfg(feature = "demo")]
mod tty_tables {
    use super::*;

    fn strip(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' && chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    const COLOUR: [(&str, &str); 2] = [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")];

    #[test]
    fn theme_list_draws_an_aligned_coloured_table_on_a_terminal() {
        let (code, out) = run_to_exit(&["--demo", "theme", "list"], &COLOUR, 100);
        assert_eq!(code, 0, "{out:?}");
        assert!(out.contains("\u{1b}[38;2;"), "expected colour: {out:?}");
        assert!(
            !out.contains("\u{1b}[?1049h"),
            "a command never takes the screen"
        );
        let plain = strip(&out);
        let lines: Vec<&str> = plain.lines().map(str::trim_end).collect();
        assert_eq!(lines.len(), 5, "{plain:?}");
        assert!(lines[0].starts_with("Id") && lines[0].contains("Appearance"));
        assert!(!plain.contains('\t'));
        let name_col = lines[0].find("Name").unwrap();
        assert!(lines[1..].iter().all(|l| l.is_char_boundary(name_col)));
        assert_eq!(&lines[1][name_col..name_col + 10], "Liminal HQ");
        assert_eq!(&lines[2][name_col..name_col + 4], "Dusk");
        assert!(lines[1].ends_with("current"));
    }

    #[test]
    fn colour_is_off_for_no_color_and_the_no_color_flag() {
        for (args, env) in [
            (
                &["--demo", "theme", "list"][..],
                &[("NO_COLOR", "1"), COLOUR[0], COLOUR[1]][..],
            ),
            (&["--demo", "--no-color", "theme", "list"][..], &COLOUR[..]),
            (
                &["--demo", "--color", "never", "theme", "list"][..],
                &COLOUR[..],
            ),
        ] {
            let (code, out) = run_to_exit(args, env, 100);
            assert_eq!(code, 0);
            assert!(!out.contains('\u{1b}'), "{args:?}: {out:?}");
        }
    }

    #[test]
    fn color_always_forces_colour_and_a_narrow_terminal_still_fits() {
        let (code, out) = run_to_exit(&["--demo", "--color", "always", "theme", "list"], &[], 50);
        assert_eq!(code, 0);
        assert!(out.contains("\u{1b}["));
        for line in strip(&out).lines() {
            assert!(line.trim_end().chars().count() <= 50, "{line:?}");
        }
        assert!(out.contains('…'), "{out:?}");
    }

    #[test]
    fn json_on_a_terminal_is_indented() {
        let (code, out) = run_to_exit(&["--demo", "theme", "list", "--json", "id"], &[], 100);
        assert_eq!(code, 0);
        assert!(out.contains("\n  {"), "{out:?}");
    }

    #[test]
    fn unbuilt_commands_exit_2_on_a_terminal_too() {
        let (code, out) = run_to_exit(&["--demo", "doctor"], &[], 100);
        assert_eq!(code, 2);
        assert!(out.contains("Not built yet."));
    }
}
