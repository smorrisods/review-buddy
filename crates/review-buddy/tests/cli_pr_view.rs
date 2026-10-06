//! `pr view`, `pr open` and `open`, run piped against the demo data.
#![cfg(feature = "demo")]

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

fn demo(args: &[&str]) -> (Command, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("review-buddy").unwrap();
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .envs(std::env::var_os("SystemRoot").map(|v| ("SystemRoot", v)))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("XDG_CONFIG_DIRS", home.path().join("etc"))
        .env("REVIEW_BUDDY_PAGER", "review-buddy-no-such-pager")
        .args(["--demo", "--frozen-time", "2026-10-05T10:00"])
        .args(["-s", "liminal-hq", "-R", "liminal-hq/review-buddy"])
        .args(args);
    (cmd, home)
}

fn stdout(args: &[&str]) -> String {
    let (mut cmd, _home) = demo(args);
    String::from_utf8(cmd.assert().success().get_output().stdout.clone()).unwrap()
}

#[test]
fn piped_view_is_plain_text_without_a_pager() {
    let text = stdout(&["pr", "view", "214"]);
    assert!(!text.contains('\u{1b}'));
    assert!(text.starts_with("Add a menu bar and keyboard-driven menus\n"));
    for needle in [
        "liminal-hq/review-buddy#214 · open",
        "ada wants ada/menus → main",
        "+32 −1 · 3 files · opened 3d ago",
        "Bucket    Waiting on you · your review is requested",
        "Reviewers smorris (requested), jo (commented)",
        "1 running · 2 passing",
        "- `Menu::select_next`",
    ] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
    assert!(!text.contains("Comments ("));
}

#[test]
fn the_selector_can_be_a_url_or_a_branch() {
    let by_url = stdout(&[
        "pr",
        "view",
        "https://github.com/liminal-hq/review-buddy/pull/214",
    ]);
    let by_branch = stdout(&["pr", "view", "ada/menus"]);
    assert_eq!(by_url, stdout(&["pr", "view", "214"]));
    assert_eq!(by_branch, by_url);
}

#[test]
fn comments_appends_the_conversation() {
    let text = stdout(&["pr", "view", "214", "--comments"]);
    assert!(text.contains("Comments (3)"));
    assert!(text.contains("src/ui/menus.rs:44"));
    assert!(text.contains("jo · 1d ago"));
}

#[test]
fn mr_is_an_alias_and_gitlab_changes_use_bang_refs() {
    let (mut cmd, _home) = demo(&["mr", "view", "1182"]);
    cmd.args(["-s", "platform", "-R", "platform/flow"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("platform/flow!1182"));
}

#[test]
fn json_fields_and_jq() {
    let value: Value = serde_json::from_str(&stdout(&[
        "pr",
        "view",
        "214",
        "--json",
        "number,ref,bucket",
    ]))
    .unwrap();
    assert_eq!(value["number"], 214);
    assert_eq!(value["ref"], "liminal-hq/review-buddy#214");
    assert_eq!(value["bucket"], "wait");
    assert_eq!(value.as_object().unwrap().len(), 3);

    assert_eq!(
        stdout(&["pr", "view", "214", "--jq", ".createdAt"]),
        "2026-10-02T10:00:00Z\n"
    );
    let comments: Value =
        serde_json::from_str(&stdout(&["pr", "view", "214", "--json", "comments"])).unwrap();
    assert_eq!(comments["comments"].as_array().unwrap().len(), 3);
}

#[test]
fn json_without_fields_lists_them() {
    let text = stdout(&["pr", "view", "--json"]);
    assert!(text.lines().any(|l| l == "body") && text.lines().any(|l| l == "comments"));
}

#[test]
fn unknown_json_fields_exit_2() {
    let (mut cmd, _home) = demo(&["pr", "view", "214", "--json", "nope"]);
    cmd.assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Unknown field `nope`"));
}

#[test]
fn a_missing_change_exits_1_and_says_what_to_do() {
    let (mut cmd, _home) = demo(&["pr", "view", "9999"]);
    cmd.assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Couldn't find"));
}

#[test]
fn an_unknown_branch_exits_2_naming_what_was_tried() {
    let (mut cmd, _home) = demo(&["pr", "view", "no/such-branch"]);
    cmd.assert()
        .code(2)
        .stderr(predicate::str::contains("no/such-branch"))
        .stderr(predicate::str::contains("pr view 214"));
}

#[test]
fn web_and_pr_open_print_the_link_in_demo_mode() {
    for args in [&["pr", "view", "214", "--web"][..], &["pr", "open", "214"]] {
        let (mut cmd, _home) = demo(args);
        cmd.assert()
            .success()
            .stdout("https://github.com/liminal-hq/review-buddy/pull/214\n")
            .stderr(predicate::str::contains("(demo)"));
    }
}

#[test]
fn open_needs_a_terminal_but_checks_the_selector_first() {
    let (mut cmd, _home) = demo(&["open", "214"]);
    cmd.assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("interactive terminal"));

    let (mut bad, _home) = demo(&["open", "9999"]);
    bad.assert()
        .code(1)
        .stderr(predicate::str::contains("Couldn't find"));
}

#[test]
fn without_a_matching_source_it_says_so_and_exits_2() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("review-buddy")
        .unwrap()
        .env_clear()
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .args(["pr", "view", "https://github.com/a/b/pull/1"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());
}
