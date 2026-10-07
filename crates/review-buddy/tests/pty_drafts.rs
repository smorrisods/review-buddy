//! Drives the real binary twice in one sandbox against a stub GitHub server: comment, leave the
//! diff keeping the draft, quit, relaunch and see it restored; then edit and delete the comment.
//! Nothing is ever written to the stub, and the draft file is private.
#![cfg(all(unix, feature = "live"))]

#[path = "support/pty.rs"]
mod pty;
mod support;

use std::time::Duration;

use support::*;
use wiremock::MockServer;

fn config(server: &MockServer) -> String {
    format!(
        r#"[[source]]
name = "stub"
kind = "github"
host = "ghe.test"
api_url = "{}"
auth = "env:RB_STUB_TOKEN"
"#,
        server.uri()
    )
}

fn launch(home: &std::path::Path, config: &str) -> pty::Pty {
    let dir = home.join("XDG_CONFIG_HOME/review-buddy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), config).unwrap();
    let mut cmd = pty::command(home);
    cmd.env("RB_STUB_TOKEN", "ghp_pty_stub_token");
    pty::Pty::spawn(cmd, 160, 40)
}

fn quit(mut s: pty::Pty) {
    s.send(b"\x03");
    assert!(s.wait_exit(Duration::from_secs(10)), "ctrl-c didn't exit");
}

fn drafts_dir(home: &std::path::Path) -> std::path::PathBuf {
    home.join("XDG_STATE_HOME/review-buddy/drafts")
}

fn files(home: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(drafts_dir(home))
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn pause() {
    std::thread::sleep(Duration::from_millis(300));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_comment_survives_leaving_and_quitting_and_can_be_edited_and_deleted() {
    let server = MockServer::start().await;
    serve_happy_writes(&server).await;
    serve_searches(&server).await;
    serve_reads(&server).await;
    let home = tempfile::tempdir().unwrap();
    let cfg = config(&server);

    let mut s = launch(home.path(), &cfg);
    s.expect("Widgets get springs", "screen");
    s.send(b"\r");
    s.expect("new_name.rs", "screen");
    s.send(b"c");
    pause();
    s.send(b"Looks right to me");
    pause();
    s.send(b"\r");
    s.expect("Added to your pending review", "screen");
    s.send(b"\x1b");
    s.expect("Keep this comment as a draft?", "screen");
    s.send(b"\r");
    s.expect("Widgets get springs", "screen");
    s.expect("✎ 1", "screen");
    quit(s);

    let saved = files(home.path());
    assert_eq!(saved.len(), 1, "{saved:?}");
    let text = std::fs::read_to_string(&saved[0]).unwrap();
    assert!(
        text.contains("Looks right to me") && !text.contains("ghp_"),
        "{text}"
    );
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&saved[0]), 0o600);
        assert_eq!(mode(&drafts_dir(home.path())), 0o700);
    }

    let mut s = launch(home.path(), &cfg);
    s.expect("✎ 1", "screen");
    s.send(b"\r");
    s.expect("Draft restored · 1 comment", "screen");
    s.send(b"e");
    s.expect("Edit comment", "screen");
    s.send(b" really");
    pause();
    s.send(b"\r");
    s.expect("Comment updated", "screen");
    s.send(b"d");
    s.expect("Delete this pending comment?", "screen");
    s.send(b"y");
    s.expect("Deleted your comment", "screen");
    s.send(b"\x1b");
    s.expect("Widgets get springs", "screen");
    quit(s);

    assert!(
        files(home.path()).is_empty(),
        "deleting the last comment removes the file"
    );
    assert!(
        writes(&server).await.is_empty(),
        "nothing reached the forge"
    );
}
