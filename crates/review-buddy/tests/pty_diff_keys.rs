//! Keyboard line ranges and `←`/`→` file flipping in the real binary, in a pseudo-terminal,
//! under `--demo`.
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

#[test]
fn shift_arrows_select_lines_and_arrows_flip_files() {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.send_until(b"\r", "@@ -10,7 +10,9 @@", "the diff opens");

    s.send_expect(b"\x1b[1;2B", "2 lines selected", "⇧↓ anchors and extends");
    s.send_expect(b"\x1b[1;2B", "3 lines selected", "⇧↓ extends again");
    s.send_expect(b"\x1b[1;2A", "2 lines selected", "⇧↑ shrinks");
    s.send(b"\x1b");
    std::thread::sleep(std::time::Duration::from_millis(300));
    s.send_expect(b"V", "V", "V starts a selection");
    s.send_expect(b"jj", "3 lines selected", "plain moves extend in V mode");
    s.send_expect(b"c", "Comment · menus.rs lines", "c comments on the range");
    s.send_expect(b"\x1b", "@@ -10,7 +10,9 @@", "esc closes the composer");

    s.send_expect(b"\x1b[C", "menubar.rs", "→ flips to the next file");
    s.send_expect(b"\x1b[D", "menus.rs", "← flips back");
    s.send_expect(
        b"\x1b[D",
        "That's the first file.",
        "← at the first file says so",
    );
}
