use assert_cmd::Command;
use predicates::prelude::*;

fn rb() -> Command {
    let mut cmd = Command::cargo_bin("review-buddy").unwrap();
    cmd.env_clear()
        .env("USERPROFILE", std::env::temp_dir())
        .env("HOME", std::env::temp_dir());
    if let Some(root) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", root);
    }
    cmd
}

#[test]
fn each_shell_prints_a_script_to_stdout() {
    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        rb().args(["completion", shell])
            .assert()
            .success()
            .stdout(predicate::str::contains("queue"))
            .stdout(predicate::str::contains("pr"))
            .stdout(predicate::str::contains("auth"))
            .stdout(predicate::str::contains("demo"))
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn unknown_shell_is_a_usage_error() {
    rb().args(["completion", "tcsh"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());
}

#[test]
fn missing_shell_is_a_usage_error() {
    rb().arg("completion").assert().code(2);
}
