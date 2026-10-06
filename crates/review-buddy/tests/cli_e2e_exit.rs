//! Exit codes 0, 1, 2, 4, 5 and 8 through the real binary, and the commands that need neither
//! the demo nor a network. Demo-only and live-only cases are gated by feature so the file
//! passes under default, `--no-default-features` and `--all-features`.
#![cfg_attr(not(feature = "demo"), allow(dead_code, unused_imports))]

use predicates::prelude::*;

#[path = "support/cli.rs"]
mod sandbox;
use sandbox::{normalise, Sandbox};

#[cfg(feature = "demo")]
fn run(sandbox: &Sandbox, args: &[&str]) -> (String, String, i32) {
    let out = sandbox.demo().args(args).output().unwrap();
    (
        normalise(&String::from_utf8(out.stdout).unwrap()),
        normalise(&String::from_utf8(out.stderr).unwrap()),
        out.status.code().unwrap(),
    )
}

#[test]
fn exit_0_on_success() {
    Sandbox::new().cmd().arg("--version").assert().code(0);
}

#[cfg(feature = "demo")]
#[test]
fn exit_2_for_usage_not_built_and_bad_selectors() {
    let sandbox = Sandbox::new();
    let cases: [(&[&str], &str); 5] = [
        (&["queue", "--show", "everything"], "--show with"),
        (&["pr", "view", "214"], "Pass --repo owner/repo"),
        (
            &[
                "pr",
                "view",
                "nonsense",
                "--source",
                "liminal-hq",
                "-R",
                "liminal-hq/review-buddy",
            ],
            "Pass a number or a URL",
        ),
        (&["pr", "list", "--source", "nowhere"], "source"),
        (&["triage", "explain", "214"], "Not built yet"),
    ];
    for (args, needle) in cases {
        let (stdout, stderr, code) = run(&sandbox, args);
        assert_eq!(code, 2, "{args:?}\n{stdout}\n{stderr}");
        assert!(stderr.contains(needle), "{args:?}: {stderr}");
    }
    sandbox
        .cmd()
        .arg("--no-such-flag")
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());
}

#[cfg(feature = "demo")]
#[test]
fn exit_1_when_the_change_does_not_exist() {
    let sandbox = Sandbox::new();
    let (_, stderr, code) = run(
        &sandbox,
        &[
            "pr",
            "view",
            "999",
            "--source",
            "liminal-hq",
            "-R",
            "liminal-hq/review-buddy",
        ],
    );
    assert_eq!(code, 1);
    assert!(stderr.contains("Couldn't find"), "{stderr}");
}

#[cfg(feature = "demo")]
#[test]
fn exit_8_while_checks_are_running() {
    let sandbox = Sandbox::new();
    let (_, stderr, code) = run(&sandbox, &pr_args("214"));
    assert_eq!(code, 8);
    assert!(stderr.contains("still running"), "{stderr}");
}

#[cfg(feature = "demo")]
fn pr_args(number: &'static str) -> Vec<&'static str> {
    vec![
        "pr",
        "checks",
        number,
        "--source",
        "liminal-hq",
        "-R",
        "liminal-hq/review-buddy",
    ]
}

// No v0.1.0 command writes to a forge, so a refused write (exit 3) can't be reached through
// the binary yet. `cmd::prompt::tests` covers `confirm_write` refusing without `--yes` or a
// terminal, and the mapping of `Cancelled` to 3 is pinned in `cmd::error::tests`.

const SOURCES: &str = "[[source]]\nname = \"work\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"http://127.0.0.1:9\"\nauth = \"env:RB_E2E_TOKEN\"\n\n\
    [[source]]\nname = \"lab\"\nkind = \"gitlab\"\nhost = \"gitlab.test\"\nauth = \"env:RB_E2E_TOKEN\"\n";

#[cfg(feature = "live")]
#[test]
fn exit_4_when_the_token_is_missing() {
    let sandbox = Sandbox::new();
    let config = sandbox.write_config(SOURCES);
    sandbox
        .cmd()
        .arg("--config")
        .arg(&config)
        .args(["pr", "list", "--source", "work"])
        .assert()
        .code(4)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("RB_E2E_TOKEN"))
        .stderr(predicate::str::contains("auth login"));
}

#[cfg(not(feature = "live"))]
#[test]
fn without_network_support_live_commands_exit_2() {
    let sandbox = Sandbox::new();
    let config = sandbox.write_config(SOURCES);
    sandbox
        .cmd()
        .arg("--config")
        .arg(&config)
        .args(["pr", "list", "--source", "work"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no network support"));
}

#[test]
fn theme_list_works_without_a_demo_or_network() {
    let sandbox = Sandbox::new();
    let out = sandbox.cmd().args(["theme", "list"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = normalise(&String::from_utf8(out.stdout).unwrap());
    assert!(
        text.starts_with("liminal-hq\tLiminal HQ\tdark\tcurrent"),
        "{text}"
    );
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn config_paths_stay_inside_the_sandbox() {
    let sandbox = Sandbox::new();
    let out = sandbox.cmd().args(["config", "paths"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    let home = sandbox.path().to_string_lossy().into_owned();
    let first = text.lines().next().unwrap();
    assert!(first.starts_with("config-dir\t"), "{first}");
    assert!(first.contains(&home) || cfg!(windows), "{first}");
}
