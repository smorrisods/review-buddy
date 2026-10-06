//! `auth login|logout|token`, `source add|test` and `config get|list` through the real binary.
//!
//! GitHub and GitLab are wiremock stubs named by `api_url`. The OS keyring is replaced by the
//! in-memory test seam (`REVIEW_BUDDY_TEST_KEYRING`), which the tests seed with `host=token`.
#![cfg_attr(not(feature = "live"), allow(dead_code, unused_imports))]

#[path = "support/cli.rs"]
mod sandbox;

use assert_cmd::Command;
use sandbox::Sandbox;
use serde_json::{json, Value};
use wiremock::matchers::{header, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "ghp_cli_secret_value_42";

fn config(api_url: &str, auth: &str) -> String {
    format!(
        "# my config\n[ui]\ntheme = \"dusk\"   # evenings\n\n[[source]]\nname = \"work\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{api_url}\"\nauth = \"{auth}\"\n"
    )
}

fn run(cmd: &mut Command) -> (String, String, i32) {
    let out = cmd.output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap(),
    )
}

fn keyring(cmd: &mut Command, seed: &str) {
    cmd.env("REVIEW_BUDDY_TEST_KEYRING", seed);
}

async fn github(status: u16) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .and(header("authorization", format!("Bearer {SECRET}").as_str()))
        .respond_with(
            ResponseTemplate::new(status)
                .insert_header("x-oauth-scopes", "repo, read:org")
                .set_body_json(json!({"login": "smorris", "name": null})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/rate_limit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": {"core": {"limit": 5000, "remaining": 4900, "reset": 1700000000},
                          "graphql": {"limit": 5000, "remaining": 4900, "reset": 1700000000}}
        })))
        .mount(&server)
        .await;
    server
}

async fn gitlab() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"username": "sam", "name": null})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/personal_access_tokens/self"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    server
}

fn assert_no_secret(label: &str, texts: &[&str]) {
    for text in texts {
        assert!(!text.contains(SECRET), "{label} leaked the token:\n{text}");
    }
}

mod config_commands {
    use super::*;

    #[test]
    fn get_prints_the_value_and_json_carries_the_origin() {
        let sb = Sandbox::new();
        let file = sb.write_config("[ui]\ntheme = \"dusk\"\n");
        let (out, _, code) = run(sb
            .cmd()
            .args(["config", "get", "ui.theme", "--config"])
            .arg(&file));
        assert_eq!((code, out.as_str()), (0, "dusk\n"));

        let (out, _, _) = run(sb
            .cmd()
            .args([
                "config",
                "get",
                "ui.theme",
                "--json",
                "key,value,origin",
                "--config",
            ])
            .arg(&file));
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value[0]["value"], "dusk");
        assert_eq!(value[0]["origin"], file.display().to_string());

        let (out, _, _) = run(sb
            .cmd()
            .args(["config", "get", "ui.jax", "--json", "origin", "--config"])
            .arg(&file));
        assert!(out.contains("default"), "{out}");
    }

    #[test]
    fn list_is_stable_and_has_three_columns() {
        let sb = Sandbox::new();
        let file = sb.write_config(&config("http://x.test", "token"));
        let (first, _, code) = run(sb.cmd().args(["config", "list", "--config"]).arg(&file));
        assert_eq!(code, 0);
        let (second, _, _) = run(sb.cmd().args(["config", "list", "--config"]).arg(&file));
        assert_eq!(first, second);
        assert!(first.lines().all(|l| l.split('\t').count() == 3), "{first}");
        let theme = first.lines().find(|l| l.starts_with("ui.theme\t")).unwrap();
        assert!(theme.starts_with("ui.theme\tdusk\t"), "{theme}");
        assert!(first.contains("source.work.host\tghe.test\t"));
        assert!(first.lines().next().unwrap().starts_with("ui.theme"));
    }

    #[test]
    fn an_unknown_key_exits_2_with_a_suggestion() {
        let sb = Sandbox::new();
        let (out, err, code) = run(sb.cmd().args(["config", "get", "ui.thme"]));
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.contains("Did you mean ui.theme?"), "{err}");
    }

    #[test]
    fn env_overrides_name_themselves_as_the_origin() {
        let sb = Sandbox::new();
        let (out, _, _) = run(sb
            .cmd()
            .env("REVIEW_BUDDY_THEME", "dusk")
            .args(["config", "list"]));
        assert!(out.contains("ui.theme\tdusk\t$REVIEW_BUDDY_THEME"), "{out}");
    }

    #[cfg(feature = "demo")]
    #[test]
    fn demo_reads_defaults_only() {
        let sb = Sandbox::new();
        let (out, _, code) = run(sb.demo().args(["config", "get", "ui.theme"]));
        assert_eq!((code, out.as_str()), (0, "liminal-hq\n"));
    }
}

mod source_add {
    use super::*;

    const ADD: [&str; 8] = [
        "source",
        "add",
        "--host",
        "ghe.test",
        "--name",
        "work",
        "--org",
        "liminal-hq",
    ];

    #[test]
    fn appends_after_a_preview_and_keeps_comments() {
        let sb = Sandbox::new();
        let file = sb.write_config("# mine\n[ui]\ntheme = \"dusk\"   # evenings\n");
        let (out, _, code) = run(sb
            .cmd()
            .args(ADD)
            .args(["--yes", "--no-test", "--config"])
            .arg(&file));
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("[[source]]"), "{out}");
        assert!(out.contains("Added source work"), "{out}");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("# mine\n[ui]\ntheme = \"dusk\"   # evenings\n"));
        assert!(text.contains("name = \"work\""));
        assert!(text.contains("scope = { orgs = [\"liminal-hq\"] }"));
        let (out, _, _) = run(sb.cmd().args(["source", "list", "--config"]).arg(&file));
        assert!(out.contains("work\tgithub\tghe.test"), "{out}");
    }

    #[test]
    fn without_a_terminal_it_needs_yes_and_writes_nothing() {
        let sb = Sandbox::new();
        let file = sb.write_config("[ui]\njax = true\n");
        let (_, err, code) = run(sb.cmd().args(ADD).arg("--config").arg(&file));
        assert_eq!(code, 3, "{err}");
        assert!(err.contains("[[source]]") && err.contains("--yes"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "[ui]\njax = true\n"
        );
    }

    #[test]
    fn it_creates_the_default_config_and_refuses_duplicates() {
        let sb = Sandbox::new();
        let args = ["source", "add", "--name", "gh", "--yes", "--no-test"];
        let (_, _, code) = run(sb.cmd().args(args));
        assert_eq!(code, 0);
        let target = sb
            .path()
            .join("config")
            .join("review-buddy")
            .join("config.toml");
        assert!(std::fs::read_to_string(target)
            .unwrap()
            .contains("host = \"github.com\""));
        let (_, err, code) = run(sb.cmd().args(args));
        assert_eq!(code, 2);
        assert!(err.contains("already a source called gh"), "{err}");
    }

    #[test]
    fn bad_flags_are_usage_errors_that_touch_nothing() {
        let sb = Sandbox::new();
        for bad in [
            &["source", "add", "--auth", "magic", "--yes"][..],
            &["source", "add", "--auth", "command", "--yes"],
            &["source", "add", "--kind", "gitlab", "--org", "x", "--yes"],
            &[
                "source",
                "add",
                "--host",
                "https://github.com",
                "--yes",
                "--no-test",
            ],
        ] {
            let (_, err, code) = run(sb.cmd().args(bad));
            assert_eq!(
                code,
                if bad.contains(&"https://github.com") {
                    1
                } else {
                    2
                },
                "{bad:?}\n{err}"
            );
        }
        assert!(!sb.path().join("config").exists());
    }

    #[cfg(feature = "demo")]
    #[test]
    fn demo_previews_and_writes_nothing() {
        let sb = Sandbox::new();
        let file = sb.path().join("never.toml");
        let (out, _, code) = run(sb.demo().args(ADD).arg("--yes").arg("--config").arg(&file));
        assert_eq!(code, 0);
        assert!(
            out.contains("[[source]]") && out.contains("(demo)"),
            "{out}"
        );
        assert!(!file.exists());
    }

    #[cfg(feature = "live")]
    #[tokio::test]
    async fn the_sign_in_test_runs_after_adding_unless_skipped() {
        let server = github(200).await;
        let sb = Sandbox::new();
        let file = sb.path().join("fresh.toml");
        let (out, err, code) = run(sb
            .cmd()
            .env("RB_SECRET", SECRET)
            .args([
                "source",
                "add",
                "--host",
                "ghe.test",
                "--name",
                "work",
                "--auth",
                "env:RB_SECRET",
                "--api-url",
                &server.uri(),
                "--yes",
                "--config",
            ])
            .arg(&file));
        assert_eq!(code, 0, "{out}\n{err}");
        assert!(
            out.contains("✓ ghe.test works: signed in as smorris"),
            "{out}"
        );
        assert_no_secret(
            "source add",
            &[&out, &err, &std::fs::read_to_string(&file).unwrap()],
        );
    }

    #[cfg(feature = "live")]
    #[tokio::test]
    async fn a_missing_token_after_adding_says_how_to_store_one() {
        let sb = Sandbox::new();
        let file = sb.path().join("fresh.toml");
        let mut cmd = sb.cmd();
        keyring(&mut cmd, "");
        let (_, err, code) = run(cmd
            .args([
                "source", "add", "--host", "ghe.test", "--auth", "token", "--yes", "--config",
            ])
            .arg(&file));
        assert_eq!(code, 4, "{err}");
        assert!(
            err.contains("Added ghe.test") && err.contains("auth login --host ghe.test"),
            "{err}"
        );
        assert!(file.exists());
    }
}

#[cfg(feature = "live")]
mod auth_commands {
    use super::*;

    #[tokio::test]
    async fn login_tests_the_token_and_never_prints_or_writes_it() {
        let server = github(200).await;
        let sb = Sandbox::new();
        let file = sb.write_config(&config(&server.uri(), "token"));
        let mut cmd = sb.cmd();
        keyring(&mut cmd, "");
        let (out, err, code) = run(cmd
            .args(["auth", "login", "--with-token", "--config"])
            .arg(&file)
            .write_stdin(format!("{SECRET}\n")));
        assert_eq!(code, 0, "{out}\n{err}");
        assert!(
            out.contains("✓ Signed in to ghe.test as smorris · scopes repo, read:org"),
            "{out}"
        );
        assert!(out.contains("review-buddy/ghe.test"), "{out}");
        let on_disk = std::fs::read_to_string(&file).unwrap();
        assert_eq!(on_disk, config(&server.uri(), "token"));
        assert_no_secret("auth login", &[&out, &err, &on_disk]);
        for entry in walk(sb.path()) {
            let text = std::fs::read(&entry).unwrap_or_default();
            assert!(
                !String::from_utf8_lossy(&text).contains(SECRET),
                "{}",
                entry.display()
            );
        }
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
        out
    }

    #[tokio::test]
    async fn a_rejected_token_exits_4_with_a_fix() {
        let server = github(401).await;
        let sb = Sandbox::new();
        let file = sb.write_config(&config(&server.uri(), "token"));
        let mut cmd = sb.cmd();
        keyring(&mut cmd, "");
        let (out, err, code) = run(cmd
            .args(["auth", "login", "--with-token", "--config"])
            .arg(&file)
            .write_stdin(format!("{SECRET}\n")));
        assert_eq!((code, out.as_str()), (4, ""), "{err}");
        assert!(
            err.contains("rejected that token") && err.contains("auth login --host ghe.test"),
            "{err}"
        );
        assert_no_secret("failed login", &[&out, &err]);
    }

    #[test]
    fn login_without_a_terminal_or_with_an_empty_stdin_is_a_usage_error() {
        let sb = Sandbox::new();
        let file = sb.write_config(&config("http://127.0.0.1:9", "token"));
        let (_, err, code) = run(sb.cmd().args(["auth", "login", "--config"]).arg(&file));
        assert_eq!(code, 2);
        assert!(err.contains("--with-token"), "{err}");
        let (_, err, code) = run(sb
            .cmd()
            .args(["auth", "login", "--with-token", "--config"])
            .arg(&file)
            .write_stdin("\n"));
        assert_eq!(code, 2);
        assert!(err.contains("No token arrived"), "{err}");
    }

    #[test]
    fn logout_needs_yes_without_a_terminal_and_names_what_it_leaves_alone() {
        let sb = Sandbox::new();
        let args = ["auth", "logout", "--host", "ghe.test"];
        let mut cmd = sb.cmd();
        keyring(&mut cmd, &format!("ghe.test={SECRET}"));
        let (out, err, code) = run(cmd.args(args));
        assert_eq!((code, out.as_str()), (3, ""));
        assert!(
            err.contains("--yes") && err.contains("gh and glab"),
            "{err}"
        );

        let mut cmd = sb.cmd();
        keyring(&mut cmd, &format!("ghe.test={SECRET}"));
        let (out, err, code) = run(cmd.args(args).arg("--yes"));
        assert_eq!(code, 0, "{err}");
        assert!(
            out.contains("Removed the stored token for ghe.test"),
            "{out}"
        );
        assert!(
            out.contains("gh and glab sign-ins were left alone"),
            "{out}"
        );
        assert_no_secret("logout", &[&out, &err]);

        let mut cmd = sb.cmd();
        keyring(&mut cmd, "");
        let (out, _, code) = run(cmd.args(args));
        assert_eq!(code, 0);
        assert!(out.contains("nothing was changed"), "{out}");
    }

    #[test]
    fn token_prints_only_the_token_on_stdout_and_the_origin_on_stderr() {
        let sb = Sandbox::new();
        let mut cmd = sb.cmd();
        keyring(&mut cmd, &format!("ghe.test={SECRET}"));
        let (out, err, code) = run(cmd.args(["auth", "token", "--host", "ghe.test"]));
        assert_eq!(
            (code, out.as_str()),
            (0, format!("{SECRET}\n").as_str()),
            "{err}"
        );
        assert_eq!(err, "Token for ghe.test from the OS keyring.\n");

        let mut cmd = sb.cmd();
        keyring(&mut cmd, "");
        let (out, err, code) = run(cmd.args(["auth", "token", "--host", "ghe.test"]));
        assert_eq!((code, out.as_str()), (4, ""));
        assert!(err.contains("auth login --host ghe.test"), "{err}");
    }

    #[test]
    fn token_follows_the_sources_auth_setting() {
        let sb = Sandbox::new();
        let file = sb.write_config(&config("http://127.0.0.1:9", "env:RB_SECRET"));
        let (out, err, code) = run(sb
            .cmd()
            .env("RB_SECRET", SECRET)
            .args(["auth", "token", "--config"])
            .arg(&file));
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, format!("{SECRET}\n"));
        assert!(err.contains("from env:RB_SECRET"), "{err}");
    }

    #[cfg(feature = "demo")]
    #[test]
    fn demo_never_touches_the_keyring_or_prints_a_token() {
        let sb = Sandbox::new();
        let (out, _, code) = run(sb.demo().args(["auth", "login", "--host", "github.com"]));
        assert_eq!(code, 0);
        assert!(
            out.contains("(demo)") && out.contains("Nothing was read or saved"),
            "{out}"
        );
        let (out, _, code) = run(sb.demo().args(["auth", "logout", "--host", "github.com"]));
        assert_eq!(code, 0);
        assert!(out.contains("(demo)"), "{out}");
        let (out, err, code) = run(sb.demo().args(["auth", "token", "--host", "github.com"]));
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.contains("(demo)"), "{err}");
    }
}

#[cfg(feature = "live")]
mod source_test {
    use super::*;

    fn two(gh: &str, gl: &str) -> String {
        format!(
            "[[source]]\nname = \"work\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{gh}\"\nauth = \"env:RB_SECRET\"\n\n[[source]]\nname = \"lab\"\nkind = \"gitlab\"\nhost = \"gitlab.test\"\napi_url = \"{gl}\"\nauth = \"env:RB_SECRET\"\n"
        )
    }

    #[tokio::test]
    async fn it_signs_in_and_lists_capabilities_per_source() {
        let (gh, gl) = (github(200).await, gitlab().await);
        let sb = Sandbox::new();
        let file = sb.write_config(&two(&gh.uri(), &gl.uri()));
        let (out, err, code) = run(sb
            .cmd()
            .env("RB_SECRET", SECRET)
            .args(["source", "test", "--config"])
            .arg(&file));
        assert_eq!(code, 0, "{out}\n{err}");
        assert!(
            out.contains("✓ ghe.test (work)  signed in as smorris via env:RB_SECRET"),
            "{out}"
        );
        assert!(out.contains("Request changes       yes"), "{out}");
        assert!(
            out.contains("✓ gitlab.test (lab)  signed in as sam"),
            "{out}"
        );
        assert_no_secret("source test", &[&out, &err]);

        let (out, _, _) = run(sb
            .cmd()
            .env("RB_SECRET", SECRET)
            .args([
                "source",
                "test",
                "lab",
                "--json",
                "source,capabilities",
                "--config",
            ])
            .arg(&file));
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["source"], "lab");
        assert_eq!(value[0]["capabilities"]["requestChanges"], false);
    }

    async fn versioned_gitlab(version: Value) -> MockServer {
        let server = gitlab().await;
        Mock::given(path("/version"))
            .respond_with(ResponseTemplate::new(200).set_body_json(version))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn it_reports_the_detected_version_and_what_that_turns_off() {
        let gh = github(200).await;
        let gl = versioned_gitlab(json!({"version": "16.9.2", "enterprise": true})).await;
        let sb = Sandbox::new();
        let file = sb.write_config(&two(&gh.uri(), &gl.uri()));
        let (out, err, code) = run(sb
            .cmd()
            .env("RB_SECRET", SECRET)
            .args(["source", "test", "lab", "--config"])
            .arg(&file));
        assert_eq!(code, 0, "{out}\n{err}");
        assert!(
            out.contains("Detected              GitLab 16.9.2, probed "),
            "{out}"
        );
        assert!(out.contains("Request changes       no"), "{out}");
        assert!(
            out.contains("GitLab 16.9 on gitlab.test doesn't support requesting changes"),
            "{out}"
        );

        let (out, _, _) = run(sb
            .cmd()
            .env("RB_SECRET", SECRET)
            .args([
                "source",
                "test",
                "lab",
                "--json",
                "probe,capabilities",
                "--config",
            ])
            .arg(&file));
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value[0]["probe"]["version"], "16.9.2");
        assert!(value[0]["probe"]["probedAt"]
            .as_str()
            .unwrap()
            .ends_with('Z'));

        let gl = versioned_gitlab(json!({"version": "17.5.1", "enterprise": true})).await;
        let file = sb.write_config(&two(&gh.uri(), &gl.uri()));
        let (out, err, code) = run(sb
            .cmd()
            .env("RB_SECRET", SECRET)
            .args([
                "source",
                "test",
                "lab",
                "--require",
                "request-changes",
                "--config",
            ])
            .arg(&file));
        assert_eq!(code, 0, "{out}\n{err}");
        assert!(out.contains("GitLab 17.5.1"), "{out}");
    }

    #[tokio::test]
    async fn exit_4_when_sign_in_fails_and_5_when_a_required_capability_is_missing() {
        let (gh, gl) = (github(401).await, gitlab().await);
        let sb = Sandbox::new();
        let file = sb.write_config(&two(&gh.uri(), &gl.uri()));
        let (out, err, code) = run(sb
            .cmd()
            .env("RB_SECRET", SECRET)
            .args(["source", "test", "work", "--config"])
            .arg(&file));
        assert_eq!(code, 4, "{out}\n{err}");
        assert!(out.contains("✕ ghe.test (work)  token rejected"), "{out}");

        let (_, err, code) = run(sb
            .cmd()
            .env("RB_SECRET", SECRET)
            .args([
                "source",
                "test",
                "lab",
                "--require",
                "request-changes",
                "--config",
            ])
            .arg(&file));
        assert_eq!(code, 5, "{err}");
        assert!(
            err.contains("gitlab.test doesn't support request changes"),
            "{err}"
        );
    }

    #[test]
    fn an_unknown_source_is_a_usage_error() {
        let sb = Sandbox::new();
        let (_, err, code) = run(sb.cmd().args(["source", "test", "nope"]));
        assert_eq!(code, 2);
        assert!(err.contains("no source called nope"), "{err}");
    }

    #[cfg(feature = "demo")]
    #[test]
    fn demo_lists_capabilities_without_a_network() {
        let sb = Sandbox::new();
        let (out, _, code) = run(sb.demo().args(["source", "test", "liminal-hq"]));
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("signed in as") && out.contains("Viewed files"),
            "{out}"
        );
    }
}
