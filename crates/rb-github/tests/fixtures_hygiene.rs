//! A cheap scan of `tests/fixtures`: every file must be valid JSON, be named in a test (as a
//! quoted string such as `"approvals"`), and appear in the README's table. It mirrors
//! the GitLab one in `rb-gitlab`, so an orphaned fixture can't linger.

use std::path::Path;

fn tests_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests"))
}

fn test_sources() -> Vec<(String, String)> {
    std::fs::read_dir(tests_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .filter(|p| p.file_name().is_some_and(|n| n != "fixtures_hygiene.rs"))
        .map(|p| {
            (
                p.display().to_string(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect()
}

#[test]
fn every_fixture_is_valid_json_used_by_a_test_and_documented() {
    let sources = test_sources();
    let readme = std::fs::read_to_string(tests_dir().join("fixtures/README.md")).unwrap();
    let mut problems = Vec::new();
    for entry in std::fs::read_dir(tests_dir().join("fixtures")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name == "README.md" {
            continue;
        }
        let Some(stem) = name.strip_suffix(".json") else {
            problems.push(format!("{name}: only .json fixtures belong here"));
            continue;
        };
        if serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap())
            .is_err()
        {
            problems.push(format!("{name}: not valid JSON"));
        }
        let quoted = format!("\"{stem}\"");
        if !sources.iter().any(|(_, text)| text.contains(&quoted)) {
            problems.push(format!("{name}: no test uses it"));
        }
        if !readme.contains(&format!("`{stem}`")) {
            problems.push(format!("{name}: missing from the README table"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
