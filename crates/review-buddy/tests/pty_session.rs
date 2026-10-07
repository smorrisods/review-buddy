//! Drives the real binary twice in one sandbox: rotate the Detail pane with `P`, quit, launch
//! again and see the remembered arrangement on the rendered screen. Also checks the file's
//! mode, that `ui.remember_layout = false` neither reads nor writes it, that the environment
//! beats it, and that `--demo` leaves nothing behind.
#![cfg(all(unix, feature = "live"))]

#[path = "support/pty.rs"]
mod pty;

use std::path::{Path, PathBuf};
use std::time::Duration;

fn session_file(home: &Path) -> PathBuf {
    home.join("XDG_STATE_HOME/review-buddy/session.toml")
}

fn write_config(home: &Path, text: &str) {
    let dir = home.join("XDG_CONFIG_HOME/review-buddy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), text).unwrap();
}

fn launch(home: &Path, extra: &[(&str, &str)]) -> pty::Pty {
    let mut cmd = pty::command(home);
    cmd.env("PATH", "");
    cmd.env_remove("REVIEW_BUDDY_DETAIL_POSITION");
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let s = pty::Pty::spawn(cmd, 160, 40);
    s.expect("q quit", "the footer is up, so keys are being read");
    s
}

fn quit(mut s: pty::Pty) {
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)), "q didn't exit");
}

fn row_col(screen: &str, needle: &str) -> (usize, usize) {
    for (row, line) in screen.lines().enumerate() {
        if let Some(byte) = line.find(needle) {
            return (row, line[..byte].chars().count());
        }
    }
    panic!("no {needle:?} on\n{screen}");
}

const CONFIG: &str = "[ui]\nmouse = true\n";

#[test]
fn p_then_quit_then_relaunch_keeps_the_arrangement() {
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), CONFIG);

    let mut s = launch(home.path(), &[]);
    s.send_expect(b"P", "Detail: top (counter-clockwise)", "P goes to top");
    s.send_expect(b"P", "Detail: left (counter-clockwise)", "P goes to left");
    let (_, queue) = row_col(&s.screen(), "╭ queue");
    let (_, detail) = row_col(&s.screen(), "╭ detail");
    assert!(detail < queue, "detail is left of the queue");
    quit(s);

    let file = session_file(home.path());
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("detail_position = \"left\""), "{text}");
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(file.parent().unwrap()), 0o700);
    }

    let s = launch(home.path(), &[]);
    let screen = s.screen();
    let (_, queue) = row_col(&screen, "╭ queue");
    let (_, detail) = row_col(&screen, "╭ detail");
    assert!(
        detail < queue,
        "the remembered left placement came back:\n{screen}"
    );
    quit(s);

    let mut s = launch(home.path(), &[("REVIEW_BUDDY_DETAIL_POSITION", "bottom")]);
    let (queue, _) = row_col(&s.screen(), "╭ queue");
    let (detail, _) = row_col(&s.screen(), "╭ detail");
    assert!(detail > queue, "the environment beats the remembered file");
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)));
}

#[test]
fn the_debounced_write_lands_before_quit() {
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), CONFIG);
    let mut s = launch(home.path(), &[]);
    s.send_expect(b"p", "Detail pane closed", "p closes Detail");
    let file = session_file(home.path());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !file.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("detail = \"closed\""), "{text}");
    quit(s);
}

#[test]
fn nothing_is_written_when_nothing_changed() {
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), CONFIG);
    let s = launch(home.path(), &[]);
    quit(s);
    assert!(!session_file(home.path()).exists());
}

#[test]
fn remember_layout_false_neither_reads_nor_writes() {
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), "[ui]\nremember_layout = false\n");
    let file = session_file(home.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "[layout]\ndetail_position = \"left\"\n").unwrap();

    let mut s = launch(home.path(), &[]);
    let (_, queue) = row_col(&s.screen(), "╭ queue");
    let (_, detail) = row_col(&s.screen(), "╭ detail");
    assert!(detail > queue, "the file wasn't read");
    s.send_expect(b"P", "Detail: top (counter-clockwise)", "P rotates anyway");
    quit(s);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "[layout]\ndetail_position = \"left\"\n",
        "the file wasn't written"
    );
}

#[test]
fn a_corrupt_session_file_is_ignored() {
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), CONFIG);
    let file = session_file(home.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "\u{1}\u{2} [[[ not toml").unwrap();
    let s = launch(home.path(), &[]);
    assert!(s.sees("queue"));
    quit(s);
}

#[cfg(feature = "demo")]
#[test]
fn demo_never_reads_or_writes_the_session_file() {
    let home = tempfile::tempdir().unwrap();
    let file = session_file(home.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "[layout]\ndetail_position = \"left\"\n").unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("PATH", "");
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect("q quit", "the footer is up");
    let (_, queue) = row_col(&s.screen(), "╭ queue");
    let (_, detail) = row_col(&s.screen(), "╭ liminal-hq/review-buddy#214");
    assert!(detail > queue, "demo ignored the remembered file");
    s.send_expect(b"P", "Detail: top (counter-clockwise)", "P rotates");
    s.send(b"p");
    std::thread::sleep(Duration::from_millis(900));
    quit(s);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "[layout]\ndetail_position = \"left\"\n"
    );

    let fresh = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(fresh.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("PATH", "");
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect("q quit", "the footer is up");
    s.send_expect(b"P", "Detail: top", "P rotates");
    std::thread::sleep(Duration::from_millis(900));
    quit(s);
    assert!(!session_file(fresh.path()).exists());
    assert!(!fresh.path().join("XDG_STATE_HOME").exists());
}
