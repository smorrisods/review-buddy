#![cfg(all(unix, feature = "demo"))]

use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

#[test]
fn a_terminal_gets_glyphs_instead_of_words() {
    let home = tempfile::tempdir().unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args([
        "--demo",
        "--no-color",
        "--source",
        "liminal-hq",
        "pr",
        "checks",
        "liminal-hq/review-buddy#214",
    ]);
    cmd.env_clear();
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    cmd.env("HOME", home.path());
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
    let text = String::from_utf8_lossy(&seen);
    assert_eq!(status.exit_code(), 8);
    assert!(text.contains("● pass"), "{text}");
    assert!(text.contains("◐ running"), "{text}");
    assert!(text.contains("1m 34s"), "{text}");
    assert!(!text.contains('\t'), "{text}");
}
