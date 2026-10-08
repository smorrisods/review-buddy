//! The Conversation tab in the real binary under `--demo`: the threads and their full bodies,
//! moving between them and folding, and `⏎` and `c` leading into the diff on the thread's line.
#![cfg(all(unix, feature = "demo"))]

#[path = "support/pty.rs"]
mod pty;

fn launch(home: &std::path::Path) -> pty::Pty {
    let mut cmd = pty::command(home);
    cmd.args([
        "--demo",
        "--frozen-time",
        review_buddy::demo::DEFAULT_FROZEN,
    ]);
    let s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s
}

#[test]
fn reading_and_folding_the_conversation_of_the_tidy_zsh_change() {
    let home = tempfile::tempdir().unwrap();
    let mut s = launch(home.path());
    s.send_expect(b"jjj", "dotfiles#31", "the zsh change is in the queue");
    s.send_expect(
        b"]]]",
        "5 comments in 3 threads",
        "the Conversation tab opens",
    );
    s.expect("folded, z expands", "the resolved thread starts folded");
    s.send_expect(b"l", "n and N move between threads", "Detail takes focus");
    s.send_expect(
        b"n",
        "Looks good overall.",
        "the long comment reads in full",
    );
    s.expect("rehash-completions", "all of the long body is there");
    s.send_expect(
        b"z",
        "ada: Looks good overall.",
        "z folds the thread to its first line",
    );
    s.send_expect(b"Z", "2 comments · folded", "Z unfolds or folds the rest");
}

#[test]
fn enter_opens_the_diff_on_the_threads_line_and_c_starts_a_reply() {
    let home = tempfile::tempdir().unwrap();
    let mut s = launch(home.path());
    s.send_expect(
        b"]]]",
        "menus.rs:44",
        "the thread is headed by its location",
    );
    s.send_expect(b"l", "n and N move between threads", "Detail takes focus");
    s.send_expect(b"\r", "pub struct Menu", "enter opens the diff");
    s.send_expect(b"\x1b", "Conversation 3", "esc returns to the queue");
    s.send_expect(
        b"c",
        "Reply · menus.rs line 44",
        "c opens a reply on the thread",
    );
}
