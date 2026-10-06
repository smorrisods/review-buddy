//! Drives the real binary in demo mode: number keys switch the source view.
#![cfg(all(unix, feature = "demo"))]

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
fn number_keys_switch_the_source_view() {
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
    // Even if the demo tried to open something, nothing real could answer.
    cmd.env("PATH", "");
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

    let mut send = |bytes: &[u8], seen: &mut Vec<u8>| {
        seen.clear();
        writer.write_all(bytes).unwrap();
        writer.flush().unwrap();
    };

    for (key, title) in [
        (b"2", "3 in this view."),
        (b"3", "smorris/dotfiles#31"),
        (b"4", "platform/flow!"),
    ] {
        send(key, &mut seen);
        assert!(
            wait_for(&rx, &mut seen, title, limit),
            "{title}: {:?}",
            String::from_utf8_lossy(&seen)
        );
    }
    send(b"q", &mut seen);
    let deadline = Instant::now() + limit;
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "q didn't exit");
        std::thread::sleep(Duration::from_millis(50));
    }
}
