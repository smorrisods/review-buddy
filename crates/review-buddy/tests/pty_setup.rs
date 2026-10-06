//! Drives `review-buddy --setup` in a pseudo-terminal: a fake `gh` on PATH, a stub GitHub named
//! by the existing config's `api_url`, and a sandboxed home. Ends with a written config and the
//! dashboard.
#![cfg(all(unix, feature = "live"))]

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Session {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Session {
    fn sees(&mut self, needle: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if String::from_utf8_lossy(&self.seen).contains(needle) {
                return true;
            }
            if let Ok(chunk) = self.rx.recv_timeout(Duration::from_millis(100)) {
                self.seen.extend(chunk);
            }
        }
        String::from_utf8_lossy(&self.seen).contains(needle)
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    fn tail(&self) -> String {
        let text = String::from_utf8_lossy(&self.seen).into_owned();
        text.chars()
            .rev()
            .take(800)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

fn stub_bin(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let script = |name: &str, body: &str| {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    script(
        "gh",
        "case \"$1 $2\" in\n\"auth status\") echo ghe.test; echo '  ✓ Logged in to ghe.test account octo (keyring)';;\n\"auth token\") echo gho_fake;;\n*) exit 1;;\nesac",
    );
    script("glab", "exit 1");
}

fn spawn(home: &Path, bin: &Path, args: &[&str]) -> Session {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(args);
    cmd.env_clear();
    cmd.env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("HOME", home);
    for var in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        cmd.env(var, home.join(var));
    }
    let child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    Session {
        child,
        writer,
        rx,
        seen: Vec::new(),
        _master: pair.master,
    }
}

async fn stub_github() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-oauth-scopes", "repo, read:org")
                .set_body_json(json!({"login": "octo"})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/rate_limit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": {"core": {"limit": 5000, "remaining": 1, "reset": 1}}
        })))
        .mount(&server)
        .await;
    Mock::given(path("/user/orgs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"login": "liminal-hq"}])))
        .mount(&server)
        .await;
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn the_rerun_flow_replaces_the_config_and_opens_the_dashboard() {
    let server = stub_github().await;
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    stub_bin(&bin);
    let dir = home.path().join("XDG_CONFIG_HOME/review-buddy");
    std::fs::create_dir_all(&dir).unwrap();
    let old = format!(
        "# old\n[[source]]\nname = \"ghe.test\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{}\"\nauth = \"cli\"\n",
        server.uri()
    );
    std::fs::write(dir.join("config.toml"), &old).unwrap();

    let mut s = spawn(home.path(), &bin, &["--setup"]);
    assert!(s.sees("Getting started"), "{}", s.tail());
    s.send(b"\r");
    assert!(s.sees("signed in as octo via gh"), "{}", s.tail());
    s.send(b"\r");
    assert!(s.sees("What to include"), "{}", s.tail());
    s.send(b"\r");
    assert!(s.sees("Afterglow Light"), "{}", s.tail());
    s.send(b"\x1b[C");
    std::thread::sleep(Duration::from_millis(400));
    s.send(b"\r");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"\r");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"\r");
    assert!(s.sees("Replace it?"), "{}", s.tail());
    s.send(b"y");
    let wrote = dir.join("config.toml.bak");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !wrote.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(wrote.exists(), "config not written: {}", s.tail());
    assert!(
        s.sees("refresh"),
        "the queue loads against the stub: {}",
        s.tail()
    );

    let written = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(written.contains("theme = \"dusk\""), "{written}");
    assert!(written.contains(&format!("api_url = \"{}\"", server.uri())));
    assert!(written.contains("# Tokens are never stored here"));
    assert!(!written.contains("gho_fake"));
    assert_eq!(
        std::fs::read_to_string(dir.join("config.toml.bak")).unwrap(),
        old
    );
    let mode = std::fs::metadata(dir.join("config.toml"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);

    s.send(b"q");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if s.child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "did not exit");
        let _ = s.rx.recv_timeout(Duration::from_millis(50));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_first_launch_with_no_config_shows_first_run_and_esc_leaves_a_hint() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    stub_bin(&bin);
    let mut s = spawn(home.path(), &bin, &[]);
    assert!(s.sees("Getting started"), "{}", s.tail());
    s.send(b"\x1b");
    assert!(s.sees("review-buddy --setup"), "{}", s.tail());
    assert!(!home
        .path()
        .join("XDG_CONFIG_HOME/review-buddy/config.toml")
        .exists());
    s.send(b"q");
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "did not exit");
        let _ = s.rx.recv_timeout(Duration::from_millis(50));
    }
}
