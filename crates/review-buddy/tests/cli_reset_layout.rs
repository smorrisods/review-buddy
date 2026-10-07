//! `review-buddy config reset-layout` and the session file in `config paths`, run in a sandbox
//! whose state directory is under the temporary home.

use predicates::prelude::*;

#[path = "support/cli.rs"]
mod sandbox;
use sandbox::Sandbox;

fn session_file(s: &Sandbox) -> std::path::PathBuf {
    s.path()
        .join("state")
        .join("review-buddy")
        .join("session.toml")
}

fn remember(s: &Sandbox) -> std::path::PathBuf {
    let file = session_file(s);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "[layout]\ndetail_position = \"left\"\n").unwrap();
    file
}

#[test]
fn without_a_terminal_it_needs_yes_and_changes_nothing() {
    let s = Sandbox::new();
    let file = remember(&s);
    s.cmd()
        .args(["config", "reset-layout"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("--yes"));
    assert!(file.exists());
}

#[test]
fn yes_removes_the_file_and_a_second_run_has_nothing_to_do() {
    let s = Sandbox::new();
    let file = remember(&s);
    s.cmd()
        .args(["config", "reset-layout", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed the remembered layout"));
    assert!(!file.exists());
    s.cmd()
        .args(["config", "reset-layout", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing was changed"));
}

#[test]
fn the_config_file_is_never_touched() {
    let s = Sandbox::new();
    remember(&s);
    let config = s.write_config("[ui]\ndetail_position = \"top\"\n");
    s.cmd()
        .arg("--config")
        .arg(&config)
        .args(["config", "reset-layout", "--yes"])
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(config).unwrap(),
        "[ui]\ndetail_position = \"top\"\n"
    );
}

#[cfg(feature = "demo")]
#[test]
fn demo_says_what_it_would_do_and_removes_nothing() {
    let s = Sandbox::new();
    let file = remember(&s);
    s.demo()
        .args(["config", "reset-layout", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Would remove the remembered layout",
        ))
        .stdout(predicate::str::contains("(demo)"));
    assert!(file.exists());
}

#[test]
fn config_paths_lists_the_session_file() {
    let s = Sandbox::new();
    let out = s
        .cmd()
        .args(["config", "paths"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    let row = text
        .lines()
        .find(|l| l.starts_with("session-file"))
        .unwrap_or_else(|| panic!("no session-file row in\n{text}"));
    assert!(
        row.contains("session.toml") && row.ends_with("not found"),
        "{row}"
    );
    remember(&s);
    s.cmd()
        .args(["config", "paths"])
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"session-file\t.*session\.toml\tpresent").unwrap());
    s.cmd()
        .args(["config", "paths", "--json", "sessionFile"])
        .assert()
        .success()
        .stdout(predicate::str::contains("session.toml"));
}

#[test]
fn remember_layout_is_a_listed_setting_that_defaults_on() {
    let s = Sandbox::new();
    s.cmd()
        .args(["config", "get", "ui.remember_layout"])
        .assert()
        .success()
        .stdout("true\n");
    let config = s.write_config("[ui]\nremember_layout = false\n");
    s.cmd()
        .arg("--config")
        .arg(&config)
        .args(["config", "get", "ui.remember_layout"])
        .assert()
        .success()
        .stdout("false\n");
    s.cmd()
        .arg("--config")
        .arg(s.write_config("[ui]\nremember_layout = \"maybe\"\n"))
        .args(["config", "get", "ui.remember_layout"])
        .assert()
        .failure();
}
