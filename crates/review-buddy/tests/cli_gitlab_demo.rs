//! The read commands on the GitLab demo sources, pinned when piped. `mr` is the hidden alias of
//! `pr`; the output is identical, with `!` refs.
#![cfg(feature = "demo")]

#[path = "support/cli.rs"]
mod sandbox;
use sandbox::{normalise, Sandbox};

fn run(args: &[&str]) -> (String, String, i32) {
    let sandbox = Sandbox::new();
    let out = sandbox.demo().args(args).output().unwrap();
    (
        normalise(&String::from_utf8(out.stdout).unwrap()),
        normalise(&String::from_utf8(out.stderr).unwrap()),
        out.status.code().unwrap(),
    )
}

fn pinned(name: &str, args: &[&str], code: i32) {
    let (stdout, stderr, got) = run(args);
    assert_eq!(got, code, "{name}\n{stdout}\n{stderr}");
    assert!(!stdout.contains('\u{1b}'), "{name}: escape codes");
    let mut text = stdout;
    if !stderr.is_empty() {
        text.push_str("--- stderr ---\n");
        text.push_str(&stderr);
    }
    insta::assert_snapshot!(name, text);
}

#[test]
fn gitlab_demo_commands_are_pinned_when_piped() {
    pinned("gitlab_mr_list", &["mr", "list", "-s", "platform"], 0);
    pinned(
        "gitlab_mr_view",
        &["mr", "view", "!1182", "-s", "platform"],
        0,
    );
    pinned(
        "gitlab_mr_view_comments",
        &["mr", "view", "!1182", "-s", "platform", "--comments"],
        0,
    );
    pinned(
        "gitlab_mr_diff",
        &["mr", "diff", "!1182", "-s", "platform"],
        0,
    );
    pinned(
        "gitlab_mr_diff_stat",
        &["mr", "diff", "!1182", "-s", "platform", "--stat"],
        0,
    );
    pinned(
        "gitlab_mr_checks",
        &["mr", "checks", "!1182", "-s", "platform"],
        1,
    );
    pinned(
        "gitlab_mr_open",
        &["mr", "open", "!1182", "-s", "platform"],
        0,
    );
    pinned(
        "gitlab_com_mr_view",
        &["mr", "view", "!12", "-s", "gitlab.com"],
        0,
    );
    pinned("gitlab_com_mr_list", &["mr", "list", "-s", "gitlab.com"], 0);
    pinned("gitlab_queue", &["queue", "-s", "platform"], 0);
}

#[test]
fn mr_and_pr_print_the_same_thing() {
    for tail in [
        &["list", "-s", "platform"][..],
        &["view", "!1182", "-s", "platform", "--comments"],
        &["diff", "!1182", "-s", "platform"],
        &["checks", "!1182", "-s", "platform"],
        &["open", "!1182", "-s", "platform"],
    ] {
        let with = |noun: &'static str| {
            let mut args = vec![noun];
            args.extend(tail);
            run(&args)
        };
        assert_eq!(with("mr"), with("pr"), "{tail:?}");
    }
}

#[test]
fn a_bang_ref_needs_only_the_source_in_the_demo() {
    let (out, err, code) = run(&[
        "mr",
        "view",
        "!1182",
        "-s",
        "platform",
        "--json",
        "ref,forge",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("platform/flow!1182"), "{out}");
    let (_, err, code) = run(&["mr", "view", "!1182"]);
    assert_eq!(code, 2);
    assert!(err.contains("--repo"), "{err}");
}

#[test]
fn mr_is_documented_but_not_listed_in_help() {
    let sandbox = Sandbox::new();
    let out = sandbox.cmd().arg("--help").output().unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(
        !help.lines().any(|l| l.trim_start().starts_with("mr ")),
        "{help}"
    );
    let docs = std::fs::read_to_string(format!("{}/../../docs/cli.md", env!("CARGO_MANIFEST_DIR")))
        .unwrap();
    assert!(docs.contains("`mr view !1182`"));
}
