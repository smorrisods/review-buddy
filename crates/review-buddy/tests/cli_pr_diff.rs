#![cfg(feature = "demo")]

use assert_cmd::Command;
use predicates::prelude::*;

fn diff() -> Command {
    let mut cmd = Command::cargo_bin("review-buddy").unwrap();
    cmd.env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .args([
            "--demo",
            "--frozen-time",
            "2026-10-05T10:00",
            "-s",
            "liminal-hq",
            "-R",
            "liminal-hq/review-buddy",
            "pr",
            "diff",
            "214",
        ]);
    cmd
}

#[test]
fn piped_output_is_a_plain_git_patch() {
    diff()
        .assert()
        .success()
        .stdout(predicate::str::starts_with("diff --git a/"))
        .stdout(predicate::str::contains(
            "--- a/src/ui/menus.rs\n+++ b/src/ui/menus.rs\n@@ ",
        ))
        .stdout(predicate::str::contains("+    selected: usize,"))
        .stdout(predicate::str::contains("\u{1b}").not());
}

#[test]
fn mr_alias_and_patch_flag_work() {
    diff().arg("--patch").assert().success();
}

#[test]
fn color_always_colours_a_pipe() {
    diff()
        .args(["--color", "always"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{1b}["));
}

#[test]
fn patch_flag_stays_plain_even_with_colour() {
    diff()
        .args(["--color", "always", "--patch"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{1b}").not());
}

#[test]
fn no_color_env_keeps_a_pipe_plain() {
    diff()
        .env("NO_COLOR", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{1b}").not());
}

#[test]
fn name_only_lists_paths() {
    diff()
        .arg("--name-only")
        .assert()
        .success()
        .stdout(predicate::str::contains("src/ui/menus.rs\n"))
        .stdout(predicate::str::contains("@@").not());
}

#[test]
fn stat_shows_bars_and_a_total() {
    diff()
        .arg("--stat")
        .assert()
        .success()
        .stdout(predicate::str::contains("src/ui/menus.rs   | "))
        .stdout(predicate::str::contains("+"))
        .stdout(predicate::str::contains(" changed, "));
}

#[test]
fn file_filters_and_repeats() {
    diff()
        .args(["--name-only", "--file", "src/ui/menus.rs"])
        .assert()
        .success()
        .stdout("src/ui/menus.rs\n");
    diff()
        .args(["--file", "src/ui/menus.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("diff --git").count(1));
}

#[test]
fn unknown_file_is_a_usage_error() {
    diff()
        .args(["--file", "nope.rs"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("No changed file matches nope.rs"));
}

#[test]
fn missing_repo_for_a_bare_number_is_a_usage_error() {
    Command::cargo_bin("review-buddy")
        .unwrap()
        .args(["--demo", "pr", "diff", "214"])
        .assert()
        .code(2);
}
