//! Every v0.1.0 command run end to end through the real binary, piped, in a sandbox.
//!
//! Demo cases use `--demo --frozen-time 2026-10-05T10:00` so times and counts are stable, and
//! pin stdout with `insta`. Output is normalised (LF endings, no temp paths, no version) so the
//! snapshots match on Linux, macOS and Windows. The exit codes in `cli_e2e_exit.rs`, and the terminal-only behaviour in `cli_e2e_tty.rs`.
#![cfg(feature = "demo")]

#[path = "support/cli.rs"]
mod sandbox;
use sandbox::{normalise, Sandbox};

/// Global flags that pick one source and one repository, so a bare number is unambiguous.
const PR: [&str; 4] = ["--source", "liminal-hq", "-R", "liminal-hq/review-buddy"];

fn run(sandbox: &Sandbox, args: &[&str]) -> (String, String, i32) {
    let out = sandbox.demo().args(args).output().unwrap();
    (
        normalise(&String::from_utf8(out.stdout).unwrap()),
        normalise(&String::from_utf8(out.stderr).unwrap()),
        out.status.code().unwrap(),
    )
}

fn pr(rest: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = PR.iter().map(|s| s.to_string()).collect();
    args.extend(rest.iter().map(|s| s.to_string()));
    args
}

fn plain(rest: &[&str]) -> Vec<String> {
    rest.iter().map(|s| s.to_string()).collect()
}

/// `(snapshot name, arguments, expected exit code)`.
fn table() -> Vec<(&'static str, Vec<String>, i32)> {
    vec![
        ("queue", plain(&["queue"]), 0),
        (
            "queue_all_buckets",
            plain(&["queue", "--show", "reviewing,assigned,authored,noise"]),
            0,
        ),
        ("pr_list", plain(&["pr", "list"]), 0),
        ("pr_view", pr(&["pr", "view", "214"]), 0),
        ("pr_diff", pr(&["pr", "diff", "214"]), 0),
        ("pr_diff_stat", pr(&["pr", "diff", "214", "--stat"]), 0),
        (
            "pr_diff_name_only",
            pr(&["pr", "diff", "214", "--name-only"]),
            0,
        ),
        ("pr_checks_running", pr(&["pr", "checks", "214"]), 8),
        ("pr_checks_passing", pr(&["pr", "checks", "209"]), 0),
        ("pr_open", pr(&["pr", "open", "214"]), 0),
        ("auth_status", plain(&["auth", "status"]), 0),
        ("source_list", plain(&["source", "list"]), 0),
        ("theme_list", plain(&["theme", "list"]), 0),
    ]
}

#[test]
fn every_command_is_pinned_when_piped() {
    let sandbox = Sandbox::new();
    for (name, args, code) in table() {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (stdout, stderr, got) = run(&sandbox, &args);
        assert_eq!(
            got, code,
            "{name}: exit code\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            !stdout.contains('\u{1b}'),
            "{name}: escape codes in piped output"
        );
        let mut text = stdout;
        if !stderr.is_empty() {
            text.push_str("--- stderr ---\n");
            text.push_str(&stderr);
        }
        insta::assert_snapshot!(name, text);
    }
}

#[test]
fn doctor_reports_sections_without_machine_paths() {
    let sandbox = Sandbox::new();
    let (stdout, _, code) = run(&sandbox, &["doctor"]);
    assert_eq!(code, 0);
    let head = stdout.split("Paths").next().unwrap().to_string();
    insta::assert_snapshot!("doctor_summary", head);
    assert!(stdout.contains("<demo>/config/"), "{stdout}");
    assert!(!stdout.contains('\\'), "{stdout}");
}

#[test]
fn config_paths_lists_each_directory_inside_the_sandbox() {
    let sandbox = Sandbox::new();
    let (stdout, _, code) = run(&sandbox, &["config", "paths"]);
    assert_eq!(code, 0);
    let names: Vec<&str> = stdout
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    insta::assert_snapshot!("config_paths_names", names.join("\n"));
    for line in stdout.lines().take(6) {
        assert!(line.contains("<demo>/"), "{line}");
    }
}

#[test]
fn open_needs_a_terminal_when_piped() {
    let sandbox = Sandbox::new();
    let (stdout, stderr, code) = run(
        &sandbox,
        &[
            "open",
            "214",
            "-R",
            "liminal-hq/review-buddy",
            "--source",
            "liminal-hq",
        ],
    );
    assert_eq!((code, stdout.as_str()), (2, ""));
    assert!(stderr.contains("needs an interactive terminal"), "{stderr}");
}

#[test]
fn triage_explain_is_not_built_yet() {
    let sandbox = Sandbox::new();
    let (stdout, stderr, code) = run(&sandbox, &["triage", "explain", "214"]);
    assert_eq!((code, stdout.as_str()), (2, ""));
    assert!(stderr.contains("Not built yet"), "{stderr}");
}

#[test]
fn completion_is_a_script_when_built_and_a_calm_exit_2_when_not() {
    let sandbox = Sandbox::new();
    let (stdout, stderr, code) = run(&sandbox, &["completion", "bash"]);
    match code {
        0 => assert!(stdout.contains("review-buddy"), "{stdout}"),
        _ => {
            assert_eq!(code, 2);
            assert!(stderr.contains("Not built yet"), "{stderr}");
        }
    }
}

use serde_json::Value;

fn json(sandbox: &Sandbox, args: &[String]) -> Value {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (stdout, stderr, code) = run(sandbox, &args);
    assert!(code == 0 || code == 8, "{stderr}");
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout}"))
}

/// A bare `--json` lists the fields a command offers, one per line.
#[test]
fn bare_json_lists_the_fields_for_every_command() {
    let sandbox = Sandbox::new();
    let cases: [(&str, Vec<String>); 8] = [
        ("fields_queue", plain(&["queue", "--json"])),
        ("fields_pr_list", plain(&["pr", "list", "--json"])),
        ("fields_pr_view", pr(&["pr", "view", "214", "--json"])),
        ("fields_pr_checks", pr(&["pr", "checks", "214", "--json"])),
        ("fields_auth_status", plain(&["auth", "status", "--json"])),
        ("fields_source_list", plain(&["source", "list", "--json"])),
        ("fields_config_paths", plain(&["config", "paths", "--json"])),
        ("fields_theme_list", plain(&["theme", "list", "--json"])),
    ];
    for (name, args) in cases {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (stdout, _, _) = run(&sandbox, &args);
        insta::assert_snapshot!(name, stdout);
    }
    let (stdout, _, _) = run(&sandbox, &["doctor", "--json"]);
    insta::assert_snapshot!("fields_doctor", stdout);
}

#[test]
fn queue_payload_is_a_compact_array_of_changes() {
    let sandbox = Sandbox::new();
    let value = json(
        &sandbox,
        &plain(&["queue", "--json", "ref,bucket,ci,updatedAt"]),
    );
    insta::assert_snapshot!(
        "payload_queue",
        serde_json::to_string_pretty(&value).unwrap()
    );
}

#[test]
fn pr_view_payload_carries_reviewers_and_ci() {
    let sandbox = Sandbox::new();
    let value = json(
        &sandbox,
        &pr(&[
            "pr",
            "view",
            "214",
            "--json",
            "number,title,state,ci,reviewers,myRole,bucket",
        ]),
    );
    insta::assert_snapshot!(
        "payload_pr_view",
        serde_json::to_string_pretty(&value).unwrap()
    );
}

#[test]
fn pr_checks_payload_still_exits_8_while_running() {
    let sandbox = Sandbox::new();
    let value = json(
        &sandbox,
        &pr(&["pr", "checks", "214", "--json", "name,state,durationSecs"]),
    );
    insta::assert_snapshot!(
        "payload_pr_checks",
        serde_json::to_string_pretty(&value).unwrap()
    );
}

#[test]
fn small_payloads() {
    let sandbox = Sandbox::new();
    let value = json(
        &sandbox,
        &plain(&["source", "list", "--json", "name,kind,host,enabled"]),
    );
    insta::assert_snapshot!(
        "payload_source_list",
        serde_json::to_string_pretty(&value).unwrap()
    );
    let value = json(&sandbox, &plain(&["theme", "list", "--json", "id,current"]));
    insta::assert_snapshot!(
        "payload_theme_list",
        serde_json::to_string_pretty(&value).unwrap()
    );
    let value = json(
        &sandbox,
        &plain(&["auth", "status", "--json", "source,state,method"]),
    );
    insta::assert_snapshot!(
        "payload_auth_status",
        serde_json::to_string_pretty(&value).unwrap()
    );
}

#[test]
fn jq_examples() {
    let sandbox = Sandbox::new();
    let cases: [(Vec<String>, &str); 4] = [
        (
            plain(&["queue", "--json", "ref", "--jq", ".[0].ref"]),
            "liminal-hq/review-buddy#214\n",
        ),
        (plain(&["queue", "--jq", ".[0].bucket"]), "wait\n"),
        (
            pr(&["pr", "view", "214", "--json", "title", "--jq", ".title"]),
            "Add a menu bar and keyboard-driven menus\n",
        ),
        (
            plain(&["theme", "list", "--json", "id", "--jq", ".[].id"]),
            "liminal-hq\ndusk\nafterglow-dark\nafterglow-light\n",
        ),
    ];
    for (args, want) in cases {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (stdout, stderr, code) = run(&sandbox, &args);
        assert_eq!((code, stdout.as_str()), (0, want), "{args:?}\n{stderr}");
    }
}
