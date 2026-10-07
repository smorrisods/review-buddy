//! Drives the real binary in a pseudo-terminal with the terminal pane: demo mode's scripted pane
//! (which must never start a process), and live mode against a stub GitHub server with a real
//! shell, a real local clone and a real managed worktree, all under a sandboxed home.
#![cfg(all(unix, any(feature = "demo", feature = "live")))]

#[path = "support/pty.rs"]
mod pty;

#[allow(dead_code)]
const ESC: &[u8] = b"\x1b";
#[allow(dead_code)]
const CTRL_BACKSLASH: &[u8] = b"\x1c";

#[cfg(feature = "demo")]
#[test]
fn demo_opens_a_scripted_pane_that_answers_typing_and_starts_nothing() {
    use std::time::Duration;
    let home = tempfile::tempdir().unwrap();
    let mut cmd = pty::command(home.path());
    cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
    cmd.env("PATH", "");
    let mut s = pty::Pty::spawn(cmd, 160, 40);
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    s.expect("q quit", "the footer is up, so keys are being read");

    s.send_until(b"t", "This pane is scripted", "t opens the scripted pane");
    s.send_expect(
        b"help\r",
        "A few commands are scripted",
        "typing is answered in-process",
    );
    s.send_expect(
        b"env\r",
        "RB_REPO=liminal-hq/review-buddy",
        "the change is in the transcript",
    );
    s.expect("esc esc", "the footer says how to leave");

    s.send(CTRL_BACKSLASH);
    s.send(ESC);
    s.expect(
        "q quit",
        "the chord then esc returns the keyboard to the app",
    );
    s.send_expect(b"t", "terminal (focused)", "t focuses the pane again");
    s.send(ESC);
    std::thread::sleep(Duration::from_millis(100));
    s.send_expect(ESC, "q quit", "esc esc is the second way out");

    assert!(
        !home.path().join("XDG_STATE_HOME").exists(),
        "demo mode wrote no state"
    );
    s.send(b"q");
}

#[cfg(feature = "live")]
mod live {
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

    struct Searching;

    impl Match for Searching {
        fn matches(&self, request: &Request) -> bool {
            String::from_utf8_lossy(&request.body).contains("\"q\"")
        }
    }

    async fn serve(server: &MockServer) {
        let fixture = format!(
            "{}/../rb-github/tests/fixtures/review_requested_p2.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let body: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(Searching)
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.test",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A clone of `acme/web` whose `origin` is rewritten to a local bare repository holding the
    /// change's head at `refs/pull/101/head`, so the fetch needs no network.
    fn clone_with_change_head(root: &Path) -> std::path::PathBuf {
        let remote = root.join("remote.git");
        let clone = root.join("clone");
        std::fs::create_dir_all(&remote).unwrap();
        std::fs::create_dir_all(&clone).unwrap();
        git(&remote, &["init", "--bare", "-q"]);
        git(&clone, &["init", "-q"]);
        std::fs::write(clone.join("marker.txt"), "base\n").unwrap();
        git(&clone, &["add", "."]);
        git(&clone, &["commit", "-q", "-m", "base"]);
        git(
            &clone,
            &["remote", "add", "origin", "https://ghe.test/acme/web.git"],
        );
        git(
            &clone,
            &[
                "config",
                &format!("url.{}.insteadOf", remote.display()),
                "https://ghe.test/acme/web.git",
            ],
        );
        std::fs::write(clone.join("marker.txt"), "change-head\n").unwrap();
        git(&clone, &["commit", "-q", "-am", "the change"]);
        git(
            &clone,
            &[
                "push",
                "-q",
                remote.to_str().unwrap(),
                "HEAD:refs/pull/101/head",
            ],
        );
        git(&clone, &["reset", "-q", "--hard", "HEAD~1"]);
        clone
    }

    fn write_config(home: &Path, server: &MockServer, terminal: &str) {
        let dir = home.join("XDG_CONFIG_HOME/review-buddy");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            format!(
                "[[source]]\nname = \"stub\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{}\"\nauth = \"env:RB_STUB_TOKEN\"\n\n[refresh]\ninterval = \"off\"\n\n[ui.terminal]\n{terminal}\n",
                server.uri()
            ),
        )
        .unwrap();
    }

    fn start(home: &Path, cwd: &Path) -> pty::Pty {
        let mut cmd = pty::command(home);
        cmd.env("RB_STUB_TOKEN", "ghp_pty_stub_token");
        cmd.cwd(cwd);
        pty::Pty::spawn(cmd, 160, 40)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_confirmed_worktree_hosts_a_real_shell_with_the_change_in_its_environment() {
        let server = MockServer::start().await;
        serve(&server).await;
        let tmp = tempfile::tempdir().unwrap();
        let clone = clone_with_change_head(tmp.path());
        let home = tmp.path().join("home");
        write_config(
            &home,
            &server,
            r#"command = ["/bin/sh", "-c", "pwd; cat marker.txt; echo env=$RB_SOURCE/$RB_REPO#$RB_NUMBER; echo url=$RB_URL; read x; echo got-$x; sleep 30"]"#,
        );
        let mut s = start(&home, &clone);
        s.expect("Add retry to sync", "the stub rows load");

        s.send_until(
            b"t",
            "Open a terminal for acme/web#101",
            "t opens the start prompt",
        );
        s.expect(
            "Create a separate checkout of acme/web#101",
            "the preview says what will happen",
        );
        s.expect("never removes it for you", "the preview explains cleanup");
        let worktrees = home.join("XDG_STATE_HOME/review-buddy/worktrees");
        assert!(!worktrees.exists(), "nothing exists before the confirm");

        s.send(b"y");
        s.expect(
            "change-head",
            "the shell starts in a worktree holding the change's head",
        );
        s.expect(
            "env=stub/acme/web#101",
            "RB_SOURCE, RB_REPO and RB_NUMBER are set",
        );
        s.expect("url=https://ghe.test/acme/web/pull/101", "RB_URL is set");
        assert!(worktrees.join("stub/acme__web/101/marker.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(clone.join("marker.txt")).unwrap(),
            "base\n"
        );

        s.send_expect(b"hi\r", "got-hi", "typing reaches the shell");
        s.send(CTRL_BACKSLASH);
        s.send(ESC);
        s.expect("q quit", "the keyboard is back with the app");
        s.send(b"q");
        assert!(
            worktrees.join("stub/acme__web/101").is_dir(),
            "the worktree is never removed for you"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_prompt_defaults_to_no_and_the_current_directory_needs_no_clone() {
        let server = MockServer::start().await;
        serve(&server).await;
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&cwd).unwrap();
        let home = tmp.path().join("home");
        write_config(
            &home,
            &server,
            r#"command = ["/bin/sh", "-c", "echo ready-in-$(basename $(pwd)); exec cat"]"#,
        );
        let mut s = start(&home, &cwd);
        s.expect("Add retry to sync", "the stub rows load");

        s.send_until(
            b"t",
            "didn't find a local clone of acme/web",
            "no clone is explained",
        );
        s.send(b"\r");
        s.expect("q quit", "Enter on the default answer cancels");
        assert!(!s.sees("ready-in-"), "nothing started");

        s.send_expect(b"t", "Open a terminal for acme/web#101", "t asks again");
        s.send(b"c");
        s.expect(
            "ready-in-elsewhere",
            "the current directory starts a shell where Review Buddy was started",
        );
        s.send_expect(b"abc\r", "abc", "input reaches the child");

        s.send(CTRL_BACKSLASH);
        s.send(b"x");
        s.expect("still running", "a running pane asks before it is stopped");
        s.send(CTRL_BACKSLASH);
        s.send(b"x");
        s.expect("Terminal closed", "the second request stops it");
        assert!(!home.join("XDG_STATE_HOME/review-buddy/worktrees").exists());
        s.send(b"q");
    }
}
