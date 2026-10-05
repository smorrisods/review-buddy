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
