//! `ui.queue_status` is a setting: `config get` prints it, a file can pick pieces or turn the
//! cluster off, and a piece that doesn't exist is an error that names the options.

use predicates::prelude::*;

#[path = "support/cli.rs"]
mod sandbox;
use sandbox::Sandbox;

#[test]
fn queue_status_defaults_to_every_piece() {
    let s = Sandbox::new();
    s.cmd()
        .args(["config", "get", "ui.queue_status"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#"["review","comments","ci","size"]"#,
        ));
}

#[test]
fn a_file_can_pick_pieces_or_turn_the_cluster_off() {
    let s = Sandbox::new();
    let some = s.write_config("[ui]\nqueue_status = [\"size\", \"review\"]\n");
    s.cmd()
        .arg("--config")
        .arg(&some)
        .args(["config", "get", "ui.queue_status"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"["size","review"]"#));
    let off = s.write_config("[ui]\nqueue_status = []\n");
    s.cmd()
        .arg("--config")
        .arg(&off)
        .args(["config", "get", "ui.queue_status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[]"));
}

#[test]
fn an_unknown_piece_is_an_error() {
    let s = Sandbox::new();
    let bad = s.write_config("[ui]\nqueue_status = [\"reviews\"]\n");
    s.cmd()
        .arg("--config")
        .arg(&bad)
        .args(["config", "get", "ui.queue_status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("review"));
}
