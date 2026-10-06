//! Guards the snapshot directory: no unreviewed `.snap.new` files, and no snapshot without a
//! test that still produces it. A cheap file scan, so CI catches leftovers without running insta.

use std::fs;
use std::path::{Path, PathBuf};

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn is_unreviewed(name: &str) -> bool {
    name.ends_with(".snap.new") || name.ends_with(".pending-snap")
}

#[test]
fn no_unreviewed_snapshots_are_left_behind() {
    let mut files = Vec::new();
    files_under(&Path::new(env!("CARGO_MANIFEST_DIR")).join("."), &mut files);
    let stray: Vec<_> = files
        .iter()
        .filter(|p| {
            !p.components().any(|c| c.as_os_str() == "target")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(is_unreviewed)
        })
        .collect();
    assert!(
        stray.is_empty(),
        "run `cargo insta review` (or delete them): {stray:?}"
    );
}

/// A snapshot is named `<test file>__<name>.snap`; its test file must exist and still mention
/// `<name>` (as a function or a macro-generated identifier).
fn orphans() -> Vec<String> {
    let dir = tests_dir();
    let mut snaps = Vec::new();
    files_under(&dir.join("snapshots"), &mut snaps);
    let mut orphans = Vec::new();
    for snap in snaps {
        let file = snap.file_name().unwrap().to_string_lossy().into_owned();
        let Some(stem) = file.strip_suffix(".snap") else {
            continue;
        };
        let Some((module, name)) = stem.split_once("__") else {
            orphans.push(format!("{file}: not named <test file>__<name>"));
            continue;
        };
        match fs::read_to_string(dir.join(format!("{module}.rs"))) {
            Ok(source) if source.contains(name) => {}
            Ok(_) => orphans.push(format!("{file}: tests/{module}.rs has no `{name}`")),
            Err(_) => orphans.push(format!("{file}: tests/{module}.rs does not exist")),
        }
    }
    orphans
}

#[test]
fn every_snapshot_has_a_test() {
    let orphans = orphans();
    assert!(
        orphans.is_empty(),
        "orphaned snapshots, delete them:\n{}",
        orphans.join("\n")
    );
}
