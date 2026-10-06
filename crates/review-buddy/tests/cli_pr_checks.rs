#![cfg(feature = "demo")]

use assert_cmd::Command;
use predicates::prelude::*;

fn checks(selector: &str, extra: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("review-buddy")
        .unwrap()
        .args(["--demo", "--source", "liminal-hq", "pr", "checks", selector])
        .args(extra)
        .assert()
}

const RUNNING: &str = "liminal-hq/review-buddy#214";
const PASSING: &str = "liminal-hq/review-buddy#209";

#[test]
fn passing_checks_exit_0_with_words_when_piped() {
    checks(PASSING, &[])
        .code(0)
        .stdout("pass\tfmt\t\npass\tclippy\t\npass\ttest (linux)\t\n");
}

#[test]
fn running_checks_exit_8_and_hint_at_watch() {
    checks(RUNNING, &[])
        .code(8)
        .stdout(
            predicate::str::contains("running\ttest (linux)")
                .and(predicate::str::contains("pass\tfmt\t18"))
                .and(predicate::str::contains("skipped\ttest (windows)"))
                .and(predicate::str::contains("cancelled\tbench\t40")),
        )
        .stderr(predicate::str::contains("--watch"));
}

#[test]
fn watch_waits_for_the_demo_checks_to_settle() {
    checks(RUNNING, &["--watch", "--interval", "1"])
        .code(0)
        .stdout(predicate::str::contains("running").not())
        .stderr(predicate::str::contains("Checking again in 1s"));
}

#[test]
fn failing_checks_exit_1() {
    Command::cargo_bin("review-buddy")
        .unwrap()
        .args([
            "--demo",
            "--source",
            "platform",
            "pr",
            "checks",
            "platform/flow!1182",
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("fail\tunit tests"))
        .stderr(predicate::str::contains("1 check failed."));
}

#[test]
fn required_drops_the_optional_checks() {
    checks(RUNNING, &["--required"])
        .code(8)
        .stdout(predicate::str::contains("coverage").not())
        .stdout(predicate::str::contains("bench").not())
        .stdout(predicate::str::contains("fmt"));
}

#[test]
fn fail_fast_is_accepted_with_watch() {
    checks(PASSING, &["--fail-fast", "--watch"]).code(0);
}

#[test]
fn json_lists_state_and_duration() {
    checks(RUNNING, &["--json", "name,state,durationSecs"])
        .code(8)
        .stdout(predicate::str::contains(r#""durationSecs":18"#))
        .stdout(predicate::str::contains(r#""state":"running""#));
}

#[test]
fn jq_filters_the_checks() {
    checks(
        RUNNING,
        &[
            "--json",
            "name,state",
            "--jq",
            ".[] | select(.state == \"running\") | .name",
        ],
    )
    .code(8)
    .stdout("test (linux)\n");
}

#[test]
fn an_unknown_change_exits_1_calmly() {
    checks("liminal-hq/review-buddy#99999", &[])
        .code(1)
        .stderr(predicate::str::contains("Couldn't find"));
}
