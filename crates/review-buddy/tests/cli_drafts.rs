//! `review-buddy drafts list|discard|clear` and the drafts folder in `config paths`, run in a
//! sandbox whose state directory is under the temporary home.

use predicates::prelude::*;
use rb_core::{ChangeId, DraftComment, ForgeKind, ReviewDraft, Side, SourceId};
use review_buddy::drafts::{self, StoredDraft};

#[path = "support/cli.rs"]
mod sandbox;
use sandbox::Sandbox;

fn dir(s: &Sandbox) -> std::path::PathBuf {
    s.path().join("state").join("review-buddy").join("drafts")
}

fn save(s: &Sandbox, number: u64) {
    let draft = StoredDraft::new(
        ChangeId {
            source_id: SourceId::new("work"),
            kind: ForgeKind::GitHub,
            repo: "acme/widgets".into(),
            number,
        },
        format!("Change {number}"),
        "abc".into(),
        1_760_000_000,
        &ReviewDraft {
            body: String::new(),
            comments: vec![DraftComment {
                path: "a.rs".into(),
                side: Side::New,
                start_line: None,
                line: 1,
                body: "x".into(),
            }],
        },
        None,
    );
    drafts::save(&dir(s), &draft).unwrap();
}

#[test]
fn list_is_tab_separated_when_piped_and_has_json() {
    let s = Sandbox::new();
    save(&s, 7);
    save(&s, 8);
    let out = s.cmd().args(["drafts", "list"]).assert().success();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let rows: Vec<&str> = text.lines().collect();
    assert_eq!(rows.len(), 2, "{text}");
    assert!(text.contains("work\tacme/widgets#7\t1\t"), "{text}");

    s.cmd()
        .args(["drafts", "list", "--json", "ref,comments,title"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ref\":\"acme/widgets#8\""));
    s.cmd()
        .args(["drafts", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("writtenAt"));
}

#[test]
fn list_with_nothing_saved_prints_nothing_and_succeeds() {
    let s = Sandbox::new();
    s.cmd()
        .args(["drafts", "list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn demo_lists_the_demo_worlds_comments_without_touching_disk() {
    let s = Sandbox::new();
    s.cmd()
        .args(["--demo", "--frozen-time", sandbox::FROZEN, "drafts", "list"])
        .assert()
        .success();
    assert!(!dir(&s).exists());
}

#[test]
fn discard_needs_yes_without_a_terminal_and_removes_only_that_draft() {
    let s = Sandbox::new();
    save(&s, 7);
    save(&s, 8);
    s.cmd()
        .args(["drafts", "discard", "acme/widgets#7"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("--yes"));
    assert_eq!(drafts::load_all(&dir(&s)).len(), 2);
    s.cmd()
        .args(["drafts", "discard", "acme/widgets#7", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Discarded your draft on acme/widgets#7",
        ));
    let left = drafts::load_all(&dir(&s));
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id.number, 8);
    s.cmd()
        .args(["drafts", "discard", "acme/widgets#7", "--yes"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no saved draft"));
}

#[test]
fn clear_needs_yes_then_removes_everything_and_demo_removes_nothing() {
    let s = Sandbox::new();
    save(&s, 7);
    save(&s, 8);
    s.cmd().args(["drafts", "clear"]).assert().code(3);
    s.cmd()
        .args(["--demo", "drafts", "clear", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing was changed"));
    assert_eq!(drafts::load_all(&dir(&s)).len(), 2);
    s.cmd()
        .args(["drafts", "clear", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 2 saved drafts"));
    s.cmd()
        .args(["drafts", "clear", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing was changed"));
}

#[test]
fn config_paths_lists_the_drafts_folder_and_ui_drafts_is_a_setting() {
    let s = Sandbox::new();
    s.cmd()
        .args(["config", "paths", "--json", "draftsDir"])
        .assert()
        .success()
        .stdout(predicate::str::contains("drafts"));
    s.cmd()
        .args(["config", "get", "ui.drafts"])
        .assert()
        .success()
        .stdout(predicate::str::contains("local"));
    let off = s.write_config("[ui]\ndrafts = \"off\"\n");
    s.cmd()
        .arg("--config")
        .arg(&off)
        .args(["config", "get", "ui.drafts"])
        .assert()
        .success()
        .stdout(predicate::str::contains("off"));
    let bad = s.write_config("[ui]\ndrafts = \"cloud\"\n");
    s.cmd()
        .arg("--config")
        .arg(&bad)
        .args(["config", "get", "ui.drafts"])
        .assert()
        .failure();
}
