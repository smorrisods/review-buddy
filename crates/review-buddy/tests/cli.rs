use assert_cmd::Command;
use predicates::prelude::*;

fn rb() -> Command {
    Command::cargo_bin("review-buddy").unwrap()
}

#[test]
fn help_succeeds() {
    rb().arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn version_succeeds() {
    rb().arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn demo_only_flags_require_demo() {
    for args in [
        ["--demo-scene", "inbox"],
        ["--frozen-time", "2026-01-01T00:00:00Z"],
        ["--jax-mood", "calm"],
        ["--size", "160x40"],
    ] {
        rb().args(args)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("--demo"));
    }
}

#[test]
fn demo_flags_pass_validation_with_demo() {
    rb().args(["--demo", "--demo-scene", "inbox", "--size", "160x40"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("interactive terminal"));
}

#[test]
fn unknown_flag_is_a_usage_error() {
    rb().arg("--nope")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--nope"));
}

#[test]
fn unimplemented_commands_say_so_and_exit_2() {
    let cases: [&[&str]; 5] = [
        &["open", "https://github.com/a/b/pull/1"],
        &["theme", "list"],
        &["doctor"],
        &["config", "paths"],
        &["triage", "explain", "https://github.com/a/b/pull/1"],
    ];
    for args in cases {
        rb().args(args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("isn't built yet"))
            .stderr(predicate::str::contains("--help"));
    }
}

#[test]
fn the_interface_needs_a_terminal() {
    for args in [&[][..], &["--demo"][..]] {
        rb().args(args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("interactive terminal"))
            .stderr(predicate::str::contains("--help"));
    }
}
