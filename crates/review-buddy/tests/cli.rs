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
    let cases: [&[&str]; 9] = [
        &["open", "https://github.com/a/b/pull/1"],
        &["doctor"],
        &["triage", "explain", "https://github.com/a/b/pull/1"],
        &["queue"],
        &["pr", "list"],
        &["pr", "checks", "--watch"],
        &["auth", "status"],
        &["source", "list"],
        &["config", "get", "ui.theme"],
    ];
    for args in cases {
        rb().args(args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(
                "Not built yet. It's planned for v0.",
            ))
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

#[cfg(feature = "demo")]
mod global_flags {
    use super::*;
    use serde_json::Value;

    /// A command with a throwaway HOME and XDG tree, and every variable the CLI reads cleared.
    fn sandbox() -> (Command, tempfile::TempDir) {
        let home = tempfile::tempdir().unwrap();
        let mut cmd = rb();
        cmd.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("XDG_CONFIG_DIRS", home.path().join("etc"));
        (cmd, home)
    }

    fn demo() -> (Command, tempfile::TempDir) {
        let (mut cmd, home) = sandbox();
        cmd.args(["--demo", "--frozen-time", "2026-10-05T10:00"]);
        (cmd, home)
    }

    fn stdout_of(mut cmd: Command) -> String {
        String::from_utf8(cmd.assert().success().get_output().stdout.clone()).unwrap()
    }

    fn json_of(cmd: Command) -> Value {
        serde_json::from_str(&stdout_of(cmd)).unwrap()
    }

    #[test]
    fn the_help_lists_every_global_flag() {
        let out = stdout_of({
            let mut c = rb();
            c.arg("--help");
            c
        });
        for flag in [
            "--source",
            "--repo",
            "--json",
            "--jq",
            "--web",
            "--color",
            "--no-color",
            "--demo",
            "--frozen-time",
            "--yes",
            "--config",
        ] {
            assert!(out.contains(flag), "{flag}");
        }
        assert!(!out.contains("  mr "), "mr stays a hidden alias");
    }

    #[test]
    fn global_flags_work_before_or_after_the_command() {
        let (mut before, _h1) = demo();
        before.args(["--json", "configDir", "config", "paths"]);
        let (mut after, _h2) = demo();
        after.args(["config", "paths", "--json", "configDir"]);
        let (b, a) = (json_of(before), json_of(after));
        assert_eq!(
            b.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["configDir"]
        );
        assert_eq!(
            a.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["configDir"]
        );
    }

    #[test]
    fn every_other_global_flag_is_accepted_after_the_command() {
        let flags: [&[&str]; 9] = [
            &["--source", "work"],
            &["-s", "work"],
            &["--repo", "a/b"],
            &["-R", "github.com/a/b"],
            &["--web"],
            &["-w"],
            &["--yes"],
            &["-y"],
            &["--no-color"],
        ];
        for extra in flags {
            let (mut cmd, _h) = demo();
            cmd.args(["theme", "list"]).args(extra);
            cmd.assert().success();
        }
    }

    #[test]
    fn source_and_repo_have_environment_defaults() {
        let (mut cmd, _h) = demo();
        cmd.env("REVIEW_BUDDY_SOURCE", "work")
            .env("REVIEW_BUDDY_REPO", "a/b")
            .env("REVIEW_BUDDY_PAGER", "cat")
            .env("REVIEW_BUDDY_PROMPT_DISABLED", "1")
            .args(["theme", "list"]);
        cmd.assert().success();
        let (mut empty, _h) = demo();
        empty.env("REVIEW_BUDDY_SOURCE", "").args(["theme", "list"]);
        empty.assert().success();
    }

    #[test]
    fn json_with_no_fields_lists_them_and_exits_0() {
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list", "--json"]);
        assert_eq!(stdout_of(cmd), "id\nname\nappearance\nbuiltin\ncurrent\n");
    }

    #[test]
    fn json_selects_fields_in_the_order_asked() {
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list", "--json", "name,id"]);
        let out = stdout_of(cmd);
        assert!(
            out.starts_with(r#"[{"name":"Liminal HQ","id":"liminal-hq"}"#),
            "{out}"
        );
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list", "--json=id"]);
        let rows = json_of(cmd);
        assert_eq!(rows.as_array().unwrap().len(), 4);
    }

    #[test]
    fn an_unknown_json_field_is_a_usage_error() {
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list", "--json", "bogus"])
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("Unknown field `bogus`"))
            .stderr(predicate::str::contains("id, name"));
    }

    #[test]
    fn jq_filters_and_implies_every_field() {
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list", "--jq", ".[].id"]);
        assert_eq!(
            stdout_of(cmd),
            "liminal-hq\ndusk\nafterglow-dark\nafterglow-light\n"
        );
        let (mut cmd, _h) = demo();
        cmd.args([
            "theme",
            "list",
            "-q",
            "map(select(.appearance == \"light\")) | length",
        ]);
        assert_eq!(stdout_of(cmd), "1\n");
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list", "--json", "id", "--jq", ".[0].id"]);
        assert_eq!(stdout_of(cmd), "liminal-hq\n");
    }

    #[test]
    fn a_bad_jq_expression_is_a_usage_error() {
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list", "--jq", ".[ |"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("Couldn't use the jq expression"));
    }

    #[test]
    fn piped_output_is_tsv_with_no_colour_even_when_asked_for_always() {
        for extra in [
            &[][..],
            &["--color", "always"],
            &["--color", "never"],
            &["--no-color"],
        ] {
            let (mut cmd, _h) = demo();
            cmd.args(["theme", "list"]).args(extra);
            let out = stdout_of(cmd);
            assert!(!out.contains('\u{1b}'), "{extra:?}");
            let rows: Vec<&str> = out.lines().collect();
            assert_eq!(rows.len(), 4);
            assert_eq!(rows[0], "liminal-hq\tLiminal HQ\tdark\tcurrent");
            assert!(rows.iter().all(|r| r.split('\t').count() == 4));
        }
    }

    #[test]
    fn colour_flags_are_validated() {
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list", "--color", "sometimes"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("sometimes"));
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list", "--color", "always", "--no-color"])
            .assert()
            .code(2);
        let (mut cmd, _h) = demo();
        cmd.env("NO_COLOR", "1")
            .args(["theme", "list"])
            .assert()
            .success();
    }

    #[test]
    fn frozen_time_needs_demo_and_a_readable_time() {
        let (mut cmd, _h) = sandbox();
        cmd.args(["theme", "list", "--frozen-time", "2026-10-05T10:00"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("--demo"));
        let (mut cmd, _h) = sandbox();
        cmd.args(["--demo", "--frozen-time", "never o'clock", "theme", "list"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("2026-10-05T10:00"));
    }

    #[test]
    fn commands_never_enter_the_tui() {
        let (mut cmd, _h) = demo();
        cmd.args(["theme", "list"]);
        let out = stdout_of(cmd);
        assert!(!out.contains("\u{1b}[?1049h"));
        let (mut cmd, _h) = demo();
        cmd.args(["queue"])
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty());
    }

    #[test]
    fn mr_is_an_alias_of_pr() {
        for noun in ["pr", "mr"] {
            let (mut cmd, _h) = demo();
            cmd.args([noun, "view", "!1182"])
                .assert()
                .code(2)
                .stderr(predicate::str::contains("Not built yet"));
        }
    }

    #[test]
    fn config_paths_in_demo_mode_touches_nothing_real() {
        let (mut cmd, home) = demo();
        cmd.args(["config", "paths"]);
        let out = stdout_of(cmd);
        assert!(!out.contains(home.path().to_str().unwrap()), "{out}");
        assert!(out.lines().any(|l| l.starts_with("config-dir\t")));
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    }

    #[test]
    fn config_paths_follows_xdg_and_reports_loaded_files() {
        let (mut cmd, home) = sandbox();
        let config = home.path().join("config/review-buddy");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("config.toml"), "[ui]\ntheme = \"dusk\"\n").unwrap();
        cmd.args(["config", "paths"]);
        let out = stdout_of(cmd);
        let config_file = config.join("config.toml");
        let line = out
            .lines()
            .find(|l| l.starts_with("config-file\t") && l.contains(config_file.to_str().unwrap()))
            .unwrap_or_else(|| panic!("{out}"));
        assert!(line.ends_with("\tloaded"), "{line}");
        assert!(out.contains(&format!("write-target\t{}", config_file.display())));

        let (mut cmd, _home2) = sandbox();
        cmd.env("XDG_CONFIG_HOME", home.path().join("config"))
            .args(["theme", "list", "--jq", ".[] | select(.current) | .id"]);
        assert_eq!(stdout_of(cmd), "dusk\n");
    }

    #[test]
    fn config_paths_honours_the_config_flag_and_json() {
        let (mut cmd, home) = sandbox();
        let file = home.path().join("elsewhere.toml");
        std::fs::write(&file, "").unwrap();
        cmd.args([
            "config",
            "paths",
            "--json",
            "configFiles,writeTarget",
            "--config",
        ])
        .arg(&file);
        let value = json_of(cmd);
        assert_eq!(value["writeTarget"], file.to_str().unwrap());
        let files = value["configFiles"].as_array().unwrap();
        assert!(files
            .iter()
            .any(|f| f["path"] == file.to_str().unwrap() && f["exists"] == true));
    }

    #[test]
    fn config_paths_lists_json_fields() {
        let (mut cmd, _h) = demo();
        cmd.args(["config", "paths", "--json"]);
        let out = stdout_of(cmd);
        for field in ["configDir", "cacheDir", "configFiles", "writeTarget"] {
            assert!(out.lines().any(|l| l == field), "{field}");
        }
    }

    #[test]
    fn a_broken_config_does_not_stop_config_paths() {
        let (mut cmd, home) = sandbox();
        let file = home.path().join("bad.toml");
        std::fs::write(&file, "this is = = not toml").unwrap();
        cmd.args(["config", "paths", "--config"]).arg(&file);
        cmd.assert().success();
    }

    #[test]
    fn usage_errors_exit_2_with_a_short_message() {
        let (mut cmd, _h) = demo();
        let output = cmd
            .args(["theme", "wat"])
            .assert()
            .code(2)
            .get_output()
            .clone();
        let err = String::from_utf8(output.stderr).unwrap();
        assert!(!err.contains("panicked") && !err.contains("RUST_BACKTRACE"));
    }
}

#[cfg(not(feature = "demo"))]
#[test]
fn builds_without_demo_say_so_for_commands() {
    rb().args(["--demo", "theme", "list"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("demo"));
}
