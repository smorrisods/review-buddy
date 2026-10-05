use std::fmt::Write;
use std::path::PathBuf;

use rb_core::{FilePatch, FileStatus};
use rb_diff::{parse_patch, DiffBody, FallbackReason, FileDiff, LineKind};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn dump(patch: &str) -> String {
    let parsed = parse_patch(patch);
    let mut out = String::new();
    for hunk in &parsed.hunks {
        writeln!(
            out,
            "{} (header position {})",
            hunk.header.render(),
            hunk.header_position
        )
        .unwrap();
        for l in &hunk.lines {
            let sign = match l.kind {
                LineKind::Context => ' ',
                LineKind::Added => '+',
                LineKind::Removed => '-',
            };
            let no = |n: Option<u32>| n.map_or("   ".to_string(), |n| format!("{n:>3}"));
            let eof = if l.no_newline { " [no newline]" } else { "" };
            writeln!(
                out,
                "{} {} p{:<3} {sign}{}{eof}",
                no(l.old_no),
                no(l.new_no),
                l.position,
                l.text.replace('\t', "→")
            )
            .unwrap();
        }
    }
    out
}

fn golden(name: &str) {
    let patch = std::fs::read_to_string(fixture(&format!("{name}.patch"))).unwrap();
    let actual = dump(&patch);
    let expected_path = fixture(&format!("{name}.golden"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&expected_path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&expected_path).expect("golden file");
    assert_eq!(
        actual, expected,
        "{name} drifted; rerun with UPDATE_GOLDEN=1 to review"
    );
}

#[test]
fn menus_rs() {
    golden("menus");
}

#[test]
fn crlf_and_tabs() {
    golden("crlf_tabs");
}

#[test]
fn new_file_without_trailing_newline() {
    golden("new_file");
}

fn file(patch: Option<&str>, status: FileStatus) -> FilePatch {
    FilePatch {
        path: "a.bin".into(),
        old_path: None,
        status,
        adds: 0,
        dels: 0,
        patch: patch.map(str::to_string),
    }
}

#[test]
fn fallbacks() {
    let reason = |f: FilePatch| match FileDiff::from_file_patch(&f).body {
        DiffBody::Fallback(r) => r,
        DiffBody::Text(_) => panic!("expected fallback"),
    };
    assert_eq!(
        reason(file(None, FileStatus::Modified)),
        FallbackReason::Unavailable
    );
    assert_eq!(
        reason(file(
            Some("Binary files a/x and b/x differ\n"),
            FileStatus::Modified
        )),
        FallbackReason::Binary
    );
    assert_eq!(
        reason(file(Some(""), FileStatus::Renamed)),
        FallbackReason::NoTextChanges
    );
    let huge = format!("@@ -1 +1 @@\n+{}\n", "x".repeat(rb_diff::MAX_PATCH_BYTES));
    assert_eq!(
        reason(file(Some(&huge), FileStatus::Modified)),
        FallbackReason::TooLarge
    );
}

#[test]
fn anchors_and_navigation() {
    let patch = std::fs::read_to_string(fixture("menus.patch")).unwrap();
    let diff = FileDiff::from_file_patch(&FilePatch {
        patch: Some(patch),
        ..file(None, FileStatus::Modified)
    });
    let p = diff.parsed().unwrap();
    let id = p.find_by_new(44).unwrap();
    assert_eq!(
        p.line(id).unwrap().text,
        "        let last = self.items.len().saturating_sub(1);"
    );
    let anchor = p.anchor(id).unwrap();
    assert_eq!(p.find_by_anchor(anchor), Some(id));
    let removed = p.find_by_old(43).unwrap();
    assert_eq!(p.line(removed).unwrap().kind, LineKind::Removed);
    assert_eq!(p.find_by_position(p.line(id).unwrap().position), Some(id));

    let first = p.next_hunk_start(None).unwrap();
    let second = p.next_hunk_start(Some(first)).unwrap();
    assert_eq!((first.hunk, second.hunk), (0, 1));
    assert_eq!(p.next_hunk_start(Some(second)), None);
    assert_eq!(p.prev_hunk_start(id), Some(first));
    assert_eq!(p.prev_hunk_start(first), None);
    assert_eq!(p.prev_hunk_start(second), Some(first));
}
