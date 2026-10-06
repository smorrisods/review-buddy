//! Selector and output polish, run piped against the demo data.
#![cfg(feature = "demo")]

#[path = "support/cli.rs"]
mod sandbox;
use sandbox::Sandbox;

#[test]
fn a_bare_repo_selector_works_without_a_source_flag() {
    let sandbox = Sandbox::new();
    let out = sandbox
        .demo()
        .args([
            "-R",
            "liminal-hq/review-buddy",
            "pr",
            "view",
            "214",
            "--json",
            "number",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("214"));
}

#[test]
fn pr_open_rejects_json_calmly() {
    let sandbox = Sandbox::new();
    let out = sandbox
        .demo()
        .args([
            "pr",
            "open",
            "214",
            "-R",
            "liminal-hq/review-buddy",
            "--json",
            "url",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("pr view --json url"), "{stderr}");
}

#[test]
fn auth_status_without_sources_points_at_the_config() {
    let sandbox = Sandbox::new();
    let out = sandbox.cmd().args(["auth", "status"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("review-buddy source add"), "{stdout}");
    assert!(stdout.contains("--setup"), "{stdout}");
}
