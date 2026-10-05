//! `queue` and `pr list` against the demo fixtures, with the clock frozen.
#![cfg(feature = "demo")]

use assert_cmd::Command;
use predicates::prelude::*;

const FROZEN: &str = "2026-10-05T10:00";

fn rb(args: &[&str]) -> Command {
    let mut cmd = Command::cargo_bin("review-buddy").unwrap();
    cmd.args(["--demo", "--frozen-time", FROZEN])
        .args(args)
        .env_remove("NO_COLOR")
        .env_remove("REVIEW_BUDDY_SOURCE")
        .env_remove("REVIEW_BUDDY_REPO");
    cmd
}

fn stdout(args: &[&str]) -> String {
    let out = rb(args).assert().success().get_output().stdout.clone();
    String::from_utf8(out).unwrap()
}

fn rows(text: &str) -> Vec<Vec<&str>> {
    text.lines().map(|l| l.split('\t').collect()).collect()
}

#[test]
fn queue_pipes_stable_tsv_in_bucket_order() {
    let text = stdout(&["queue"]);
    let rows = rows(&text);
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|r| r.len() == 7));
    let buckets: Vec<&str> = rows.iter().map(|r| r[0]).collect();
    assert_eq!(buckets, ["wait", "wait", "look", "look", "later"]);
    assert_eq!(rows[0][1], "liminal-hq");
    assert_eq!(rows[0][2], "liminal-hq/review-buddy#214");
    assert_eq!(rows[0][3], "running");
    assert_eq!(rows[0][4], "ada");
    assert_eq!(rows[0][5], "2026-10-05T08:00:00Z");
    assert_eq!(rows[0][6], "Add a menu bar and keyboard-driven menus");
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn queue_hides_noise_and_drafts_unless_asked() {
    assert!(!stdout(&["queue"]).contains("renovate"));
    let noise = stdout(&["queue", "--bucket", "noise"]);
    assert_eq!(noise.lines().count(), 2);
    assert!(noise.lines().all(|l| l.starts_with("noise\t")));
}

#[test]
fn queue_show_replaces_the_configured_filters() {
    let text = stdout(&["queue", "--show", "reviewing,assigned,authored,noise"]);
    assert!(text.contains("renovate"));
    let only_reviewing = stdout(&["queue", "--show", "reviewing"]);
    assert!(only_reviewing
        .lines()
        .all(|l| !l.contains("smorris/dotfiles")));
}

#[test]
fn queue_bucket_is_repeatable() {
    let text = stdout(&["queue", "--bucket", "wait", "--bucket", "later"]);
    let buckets: Vec<&str> = text
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    assert_eq!(buckets, ["wait", "wait", "later"]);
}

#[test]
fn queue_rejects_unknown_show_names() {
    rb(&["queue", "--show", "everything"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--show with"));
}

#[test]
fn queue_json_carries_bucket_and_reason() {
    let text = stdout(&["queue", "--json", "ref,bucket,bucketReason,ci"]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let first = &value[0];
    assert_eq!(first["ref"], "liminal-hq/review-buddy#214");
    assert_eq!(first["bucket"], "wait");
    assert_eq!(first["bucketReason"], "your review is requested");
    assert_eq!(first["ci"], "running");
    assert_eq!(first.as_object().unwrap().len(), 4);
}

#[test]
fn queue_jq_runs_over_the_json() {
    let text = stdout(&[
        "queue",
        "--json",
        "number,bucket",
        "--jq",
        ".[] | select(.bucket == \"later\") | .number",
    ]);
    assert_eq!(text.trim(), "12");
}

#[test]
fn json_with_no_fields_lists_them() {
    let text = stdout(&["queue", "--json"]);
    assert!(text.lines().any(|l| l == "bucket"));
    assert!(text.lines().any(|l| l == "bucketReason"));
    rb(&["pr", "list", "--json", "nope"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Unknown field `nope`"));
}

#[test]
fn pr_list_pipes_tsv_newest_first() {
    let text = stdout(&["pr", "list"]);
    let rows = rows(&text);
    assert_eq!(rows.len(), 7);
    assert!(rows.iter().all(|r| r.len() == 7));
    assert_eq!(rows[0][1], "liminal-hq/review-buddy#214");
    assert_eq!(rows[0][2], "open");
    let times: Vec<&str> = rows.iter().map(|r| r[5]).collect();
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(times, sorted);
}

#[test]
fn pr_list_limit_and_author_and_search() {
    assert_eq!(stdout(&["pr", "list", "-L", "2"]).lines().count(), 2);
    let by = stdout(&["pr", "list", "--author", "priya"]);
    assert_eq!(by.lines().count(), 1);
    assert!(by.contains("platform/flow!1182"));
    let found = stdout(&["pr", "list", "--search", "menu"]);
    assert_eq!(found.lines().count(), 1);
    assert!(found.contains("#214"));
}

#[test]
fn pr_list_at_me_resolves_per_source() {
    let text = stdout(&["pr", "list", "--author", "@me"]);
    assert!(text.contains("smorris/dotfiles#31"));
    assert!(!text.contains("review-buddy#214"));
    let assigned = rb(&["pr", "list", "--assignee", "@me", "--json", "number"])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&assigned.get_output().stdout).unwrap();
    assert!(value.is_array());
}

#[test]
fn pr_list_explains_other_assignees() {
    rb(&["pr", "list", "--assignee", "grace"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--assignee @me"));
}

#[test]
fn pr_list_json_has_url_and_bucket() {
    let text = stdout(&[
        "pr",
        "list",
        "-L",
        "1",
        "--json",
        "url,state,isDraft,bucket,reviewers,labels",
    ]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(value[0]["url"].as_str().unwrap().starts_with("https://"));
    assert_eq!(value[0]["state"], "open");
    assert_eq!(value[0]["isDraft"], false);
    assert!(value[0]["reviewers"].is_array());
}

#[test]
fn empty_results_print_nothing_when_piped() {
    rb(&["pr", "list", "--search", "no-such-change"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());
    rb(&["queue", "--source", "smorris", "--show", "reviewing"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());
    rb(&[
        "pr",
        "list",
        "--search",
        "no-such-change",
        "--json",
        "number",
    ])
    .assert()
    .success()
    .stdout("[]\n");
}

#[test]
fn source_flag_narrows_the_queue() {
    let text = stdout(&["queue", "--source", "platform"]);
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("platform/flow!1182"));
}
