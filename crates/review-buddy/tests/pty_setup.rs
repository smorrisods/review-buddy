//! Drives `review-buddy --setup` in a pseudo-terminal: a fake `gh` on PATH, a stub GitHub named
//! by the existing config's `api_url`, and a sandboxed home. Ends with a written config and the
//! dashboard.
#![cfg(all(unix, feature = "live"))]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use wiremock::matchers::path;
#[path = "support/pty.rs"]
mod pty;

use pty::Pty;
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn spawn(home: &Path, bin: &Path, args: &[&str]) -> Pty {
    let mut cmd = pty::command(home);
    cmd.args(args);
    cmd.env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    Pty::spawn(cmd, 160, 40)
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
    s.expect("Getting started", "first run opens");
    s.send_until(b"\r", "signed in as octo via gh", "the account is found");
    s.send_expect(b"\r", "What to include", "scope step");
    s.send_expect(b"\r", "Afterglow Light", "look step");
    s.send(b"\x1b[C");
    s.send_expect(b"\r", "Keep Jax around", "jax step");
    s.send_expect(b"\r", "Jax around", "summary");
    s.send_expect(b"\r", "Replace it?", "confirm");
    s.send(b"y");
    let wrote = dir.join("config.toml.bak");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !wrote.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(wrote.exists(), "config not written:\n{}", s.screen());
    s.expect("refresh", "the queue loads against the stub");

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
    assert!(s.wait_exit(Duration::from_secs(10)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_first_launch_with_no_config_shows_first_run_and_esc_leaves_a_hint() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    stub_bin(&bin);
    let mut s = spawn(home.path(), &bin, &[]);
    s.expect("Getting started", "first run opens");
    s.send_until(b"\x1b", "review-buddy --setup", "esc leaves a hint");
    assert!(!home
        .path()
        .join("XDG_CONFIG_HOME/review-buddy/config.toml")
        .exists());
    s.send(b"q");
    assert!(s.wait_exit(Duration::from_secs(10)));
}
