//! Turns a finished [`Plan`] into a commented `config.toml` and writes it atomically.
//!
//! The text starts from `config.example.toml`, so the file explains itself, with the sources
//! and the two choices from first run filled in. Nothing is written without the caller having
//! reached the confirm step, and an existing file is replaced only when the plan says so.

use std::path::{Path, PathBuf};

use crate::config::{ConfigEditor, ConfigError, LoadedConfig};

use super::flow::{Plan, Written};

const TEMPLATE: &str = include_str!("../../../../config.example.toml");
const HEADER: &str = "\
# Review Buddy config, written by `review-buddy --setup`. Run it again any time, or edit by hand.
# Split sources into config.d/*.toml if you like; they merge in lexical order.
# Tokens are never stored here; they live in your OS keyring.

";
const SOURCES_START: &str = "# Sources: tab keys";
const SOURCES_END: &str = "[keys]";

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("A config file is already at {}. Nothing was changed.", .0.display())]
    Exists(PathBuf),
    #[error("There's nothing to write yet. Connect at least one account.")]
    NoSources,
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("Couldn't keep a copy of the old file at {}: {reason}", path.display())]
    Backup { path: PathBuf, reason: String },
}

/// The config text for `plan`: the example's comments, this plan's sources, theme and Jax.
pub fn render(plan: &Plan) -> Result<String, ConfigError> {
    let start = TEMPLATE.find(SOURCES_START).unwrap_or(TEMPLATE.len());
    let end = TEMPLATE[start..]
        .find(SOURCES_END)
        .map_or(TEMPLATE.len(), |i| start + i);
    let body = TEMPLATE.find("[ui]").unwrap_or(0);
    let mut text = String::from(HEADER);
    text.push_str(&TEMPLATE[body..start]);
    text.push_str("# Sources: tab keys 2–9 follow this order (1 is always All).\n");
    text.push('\n');
    for source in &plan.sources {
        text.push_str(&source.to_toml());
        text.push('\n');
    }
    text.push_str(&TEMPLATE[end..]);

    let mut editor = ConfigEditor::from_text(&plan.target, &text)?;
    editor.set("ui", "theme", plan.theme.as_str());
    editor.set("ui", "jax", plan.jax);
    Ok(editor.text())
}

/// Validates and writes the config. Atomic, `0600`, and never over an existing file unless
/// `plan.replace` is set; a replaced file is kept beside it as `<name>.bak`.
pub fn write(plan: &Plan) -> Result<Written, WriteError> {
    if plan.sources.is_empty() {
        return Err(WriteError::NoSources);
    }
    let exists = plan.target.exists();
    if exists && !plan.replace {
        return Err(WriteError::Exists(plan.target.clone()));
    }
    let text = render(plan)?;
    LoadedConfig::from_texts(
        &[(plan.target.clone(), text.clone())],
        &rb_paths::MapEnv::new(""),
    )?;
    let backup = if exists {
        Some(keep_copy(&plan.target)?)
    } else {
        None
    };
    ConfigEditor::from_text(&plan.target, &text)?.save()?;
    Ok(Written {
        path: plan.target.clone(),
        backup,
    })
}

fn keep_copy(path: &Path) -> Result<PathBuf, WriteError> {
    let name = path.file_name().map_or_else(
        || "config.toml".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let backup = path.with_file_name(format!("{name}.bak"));
    let fail = |e: &dyn std::fmt::Display| WriteError::Backup {
        path: backup.clone(),
        reason: e.to_string(),
    };
    let old = std::fs::read(path).map_err(|e| fail(&e))?;
    crate::config::write_atomic(&backup, &old).map_err(|e| fail(&e))?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::source::{AuthKind, SourceSpec};
    use rb_core::ForgeKind;

    fn plan(target: &Path) -> Plan {
        Plan {
            target: target.to_path_buf(),
            theme: "dusk".into(),
            jax: false,
            replace: false,
            sources: vec![
                SourceSpec {
                    name: "github.com".into(),
                    kind: ForgeKind::GitHub,
                    host: "github.com".into(),
                    api_url: None,
                    auth: AuthKind::Cli,
                    scope_user: true,
                    owners: vec!["liminal-hq".into()],
                },
                SourceSpec {
                    name: "gitlab.work.ca".into(),
                    kind: ForgeKind::GitLab,
                    host: "gitlab.work.ca".into(),
                    api_url: None,
                    auth: AuthKind::Token,
                    scope_user: false,
                    owners: Vec::new(),
                },
            ],
        }
    }

    fn load(text: &str) -> LoadedConfig {
        LoadedConfig::from_texts(
            &[(PathBuf::from("c.toml"), text.to_string())],
            &rb_paths::MapEnv::new(""),
        )
        .unwrap()
    }

    #[test]
    fn the_rendered_file_is_commented_valid_and_carries_the_choices() {
        let t = tempfile::tempdir().unwrap();
        let text = render(&plan(&t.path().join("config.toml"))).unwrap();
        let loaded = load(&text);
        assert_eq!(loaded.config.ui.theme, "dusk");
        assert!(!loaded.config.ui.jax);
        let names: Vec<_> = loaded
            .config
            .sources
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["github.com", "gitlab.work.ca"]);
        assert!(loaded.config.sources[0].scope.user);
        assert!(text.starts_with("# Review Buddy config, written by"));
        assert!(text.contains("# Tokens are never stored here"));
        assert!(text.contains("[keys]"));
        assert!(text.contains("merge_method = \"squash\"     # merge | squash | rebase"));
        assert!(
            !text.contains("liminal-hq\"\nkind"),
            "example sources are replaced"
        );
        assert!(
            text.contains("theme = \"dusk\"        # liminal-hq"),
            "{text}"
        );
    }

    #[test]
    fn writing_creates_a_private_file_and_leaves_no_temp_files() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("cfg/review-buddy/config.toml");
        let written = write(&plan(&target)).unwrap();
        assert_eq!(written.path, target);
        assert!(written.backup.is_none());
        assert_eq!(
            load(&std::fs::read_to_string(&target).unwrap())
                .config
                .sources
                .len(),
            2
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let names: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn an_existing_file_is_never_overwritten_without_replace() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("config.toml");
        std::fs::write(&target, "# mine\n").unwrap();
        let err = write(&plan(&target)).unwrap_err();
        assert!(matches!(err, WriteError::Exists(_)));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "# mine\n");
    }

    #[test]
    fn replacing_keeps_the_old_file_as_a_backup() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("config.toml");
        std::fs::write(&target, "# mine\n[ui]\njax = false\n").unwrap();
        let mut p = plan(&target);
        p.replace = true;
        let written = write(&p).unwrap();
        let backup = written.backup.unwrap();
        assert_eq!(backup.file_name().unwrap(), "config.toml.bak");
        assert_eq!(
            std::fs::read_to_string(backup).unwrap(),
            "# mine\n[ui]\njax = false\n"
        );
        assert!(std::fs::read_to_string(&target)
            .unwrap()
            .contains("[[source]]"));
    }

    #[test]
    fn an_invalid_plan_writes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("config.toml");
        let mut p = plan(&target);
        p.sources[0].host = "https://github.com".into();
        assert!(matches!(write(&p), Err(WriteError::Config(_))));
        assert!(!target.exists());
        p.sources.clear();
        assert!(matches!(write(&p), Err(WriteError::NoSources)));
    }

    #[test]
    fn the_template_has_both_markers() {
        assert!(TEMPLATE.contains(SOURCES_START));
        assert!(TEMPLATE.contains(SOURCES_END));
    }
}
