//! Changes to `[[source]]` entries in the layered write target.
//!
//! Every function opens the file with [`ConfigEditor`], edits the parsed document in place so
//! comments, ordering and untouched keys survive, validates the whole result, then writes it
//! atomically with mode `0600`.

use std::path::Path;

use rb_core::ForgeKind;
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::config::{ConfigEditor, ConfigError, LoadedConfig};
use crate::setup::{AuthKind, SourceSpec};
use crate::ui::layout::Size;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("There's already a source called {0}. Pick another name.")]
    Duplicate(String),
    #[error(
        "Couldn't find a source called {0} in the config file. Reload settings and try again."
    )]
    Missing(String),
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// Appends a new source. See [`crate::setup::add_source`].
pub fn add(path: &Path, spec: &SourceSpec) -> Result<(), EditError> {
    crate::setup::add_source(path, spec).map_err(|e| match e {
        crate::setup::AddError::Duplicate(name) => EditError::Duplicate(name),
        crate::setup::AddError::Config(e) => EditError::Config(e),
    })
}

/// Turns a source on or off. Turning it on removes the `enabled` key (on is the default).
pub fn set_enabled(path: &Path, name: &str, enabled: bool) -> Result<(), EditError> {
    change(path, name, |table| {
        if enabled {
            table.remove("enabled");
        } else {
            put(table, "enabled", Value::from(false));
        }
    })
}

/// Writes a source's `hide_repos`; an empty list removes the key. Comments and every other key
/// stay as they were.
pub fn set_hide_repos(path: &Path, name: &str, entries: &[String]) -> Result<(), EditError> {
    change(path, name, |table| {
        if entries.is_empty() {
            table.remove("hide_repos");
        } else {
            put(table, "hide_repos", string_array(entries));
        }
    })
}

/// Writes `hide_repos` for each `(source, entries)` that the file at `path` defines. Returns
/// the names written and the names of sources the file doesn't define.
pub fn save_hide_repos(
    path: &Path,
    sources: &[(String, Vec<String>)],
) -> Result<(Vec<String>, Vec<String>), EditError> {
    let (mut saved, mut elsewhere) = (Vec::new(), Vec::new());
    for (name, entries) in sources {
        match set_hide_repos(path, name, entries) {
            Ok(()) => saved.push(name.clone()),
            Err(EditError::Missing(_)) if entries.is_empty() => {}
            Err(EditError::Missing(_)) => elsewhere.push(name.clone()),
            Err(other) => return Err(other),
        }
    }
    Ok((saved, elsewhere))
}

/// Writes `ui.queue_width` and `ui.queue_height`; a size of `None` (automatic) removes its key.
pub fn save_layout_sizes(
    path: &Path,
    queue_width: Option<Size>,
    queue_height: Option<Size>,
) -> Result<(), EditError> {
    let mut editor = ConfigEditor::open(path)?;
    for (key, size) in [("queue_width", queue_width), ("queue_height", queue_height)] {
        match size {
            Some(Size::Cells(n)) => editor.set("ui", key, i64::from(n)),
            Some(size) => editor.set("ui", key, size.to_string()),
            None => {
                editor.remove("ui", key);
            }
        }
    }
    finish(editor)
}

/// Removes a source's whole `[[source]]` table, and the comments attached to it.
pub fn remove(path: &Path, name: &str) -> Result<(), EditError> {
    let mut editor = ConfigEditor::open(path)?;
    let tables = sources_mut(editor.document_mut());
    let index = find(tables, name).ok_or_else(|| EditError::Missing(name.to_string()))?;
    tables.remove(index);
    if tables.is_empty() {
        editor.document_mut().remove("source");
    }
    finish(editor)
}

/// Updates the keys of `before`'s entry that differ in `after`. Keys the form doesn't show
/// (`repos`, `in_all`, `tag_colour`, comments) are left exactly as they were.
pub fn update(path: &Path, before: &SourceSpec, after: &SourceSpec) -> Result<(), EditError> {
    if before.name != after.name {
        let existing = ConfigEditor::open(path)?;
        let text = existing.text();
        let loaded =
            LoadedConfig::from_texts(&[(path.to_path_buf(), text)], &rb_paths::MapEnv::new(""))?;
        let clash = loaded.config.sources.iter().any(|s| {
            s.name.eq_ignore_ascii_case(&after.name) && !s.name.eq_ignore_ascii_case(&before.name)
        });
        if clash {
            return Err(EditError::Duplicate(after.name.clone()));
        }
    }
    change(path, &before.name, |table| apply(table, before, after))
}

fn apply(table: &mut Table, before: &SourceSpec, after: &SourceSpec) {
    if before.name != after.name {
        put(table, "name", Value::from(after.name.as_str()));
    }
    if before.host != after.host {
        put(table, "host", Value::from(after.host.as_str()));
    }
    if before.api_url != after.api_url {
        match &after.api_url {
            Some(url) => put(table, "api_url", Value::from(url.as_str())),
            None => {
                table.remove("api_url");
            }
        }
    }
    if before.auth != after.auth {
        set_auth(table, &after.auth);
    }
    if before.owners != after.owners || before.scope_user != after.scope_user {
        set_scope(table, after);
    }
}

fn set_auth(table: &mut Table, auth: &AuthKind) {
    match auth {
        AuthKind::Cli => put(table, "auth", Value::from("cli")),
        AuthKind::Token => put(table, "auth", Value::from("token")),
        AuthKind::Env(var) => put(table, "auth", Value::from(format!("env:{var}"))),
        AuthKind::Command(command) => {
            put(table, "auth", Value::from("command"));
            put(table, "token_command", Value::from(command.as_str()));
        }
    }
    if !matches!(auth, AuthKind::Command(_)) {
        table.remove("token_command");
    }
}

fn set_scope(table: &mut Table, spec: &SourceSpec) {
    let key = match spec.kind {
        ForgeKind::GitHub => "orgs",
        ForgeKind::GitLab => "groups",
    };
    if !table.contains_key("scope") {
        let mut inline = InlineTable::new();
        fill_scope(&mut inline, key, spec);
        if !inline.is_empty() {
            table.insert("scope", Item::Value(Value::InlineTable(inline)));
        }
        return;
    }
    let Some(scope) = table.get_mut("scope").and_then(Item::as_table_like_mut) else {
        return;
    };
    if spec.owners.is_empty() {
        scope.remove(key);
    } else {
        scope.insert(key, value(string_array(&spec.owners)));
    }
    if spec.kind == ForgeKind::GitHub {
        if spec.scope_user {
            scope.insert("user", value(true));
        } else {
            scope.remove("user");
        }
    }
    if scope.is_empty() {
        table.remove("scope");
    }
}

fn fill_scope(inline: &mut InlineTable, key: &str, spec: &SourceSpec) {
    if !spec.owners.is_empty() {
        inline.insert(key, string_array(&spec.owners));
    }
    if spec.scope_user && spec.kind == ForgeKind::GitHub {
        inline.insert("user", Value::from(true));
    }
}

fn string_array(items: &[String]) -> Value {
    Value::Array(items.iter().map(String::as_str).collect::<Array>())
}

/// Sets `key`, keeping the trailing comment of the value it replaces.
fn put(table: &mut Table, key: &str, mut new: Value) {
    if let Some(old) = table.get(key).and_then(Item::as_value) {
        *new.decor_mut() = old.decor().clone();
    }
    table.insert(key, Item::Value(new));
}

fn sources_mut(doc: &mut DocumentMut) -> &mut ArrayOfTables {
    if !doc.contains_key("source") {
        doc.insert("source", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    doc["source"]
        .as_array_of_tables_mut()
        .expect("`source` is an array of tables in a validated config")
}

fn find(tables: &ArrayOfTables, name: &str) -> Option<usize> {
    tables
        .iter()
        .position(|t| t.get("name").and_then(Item::as_str) == Some(name))
}

fn change(path: &Path, name: &str, edit: impl FnOnce(&mut Table)) -> Result<(), EditError> {
    let mut editor = ConfigEditor::open(path)?;
    let tables = sources_mut(editor.document_mut());
    let index = find(tables, name).ok_or_else(|| EditError::Missing(name.to_string()))?;
    edit(tables.get_mut(index).expect("index came from find"));
    finish(editor)
}

fn finish(editor: ConfigEditor) -> Result<(), EditError> {
    let text = editor.text();
    LoadedConfig::from_texts(
        &[(editor.path().to_path_buf(), text)],
        &rb_paths::MapEnv::new(""),
    )?;
    editor.save()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
# my config
[ui]
theme = \"dusk\"   # evening

# work
[[source]]
name = \"work\"            # the day job
kind = \"github\"
host = \"github.com\"
auth = \"cli\"             # reuse gh
scope = { orgs = [\"liminal-hq\"] }
tag_colour = \"cyan\"

[[source]]
name = \"lab\"
kind = \"gitlab\"
host = \"gitlab.work.ca\"
auth = \"token\"

[source.scope]
groups = [\"platform\"]
projects = [\"platform/api\"]
";

    fn spec(name: &str, kind: ForgeKind, host: &str) -> SourceSpec {
        SourceSpec {
            name: name.into(),
            kind,
            host: host.into(),
            api_url: None,
            auth: AuthKind::Cli,
            scope_user: false,
            owners: Vec::new(),
        }
    }

    fn work() -> SourceSpec {
        SourceSpec {
            owners: vec!["liminal-hq".into()],
            ..spec("work", ForgeKind::GitHub, "github.com")
        }
    }

    fn lab() -> SourceSpec {
        SourceSpec {
            auth: AuthKind::Token,
            owners: vec!["platform".into()],
            ..spec("lab", ForgeKind::GitLab, "gitlab.work.ca")
        }
    }

    fn file() -> (tempfile::TempDir, std::path::PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("config.toml");
        std::fs::write(&p, FILE).unwrap();
        (t, p)
    }

    fn sources(path: &Path) -> Vec<crate::config::SourceConfig> {
        let text = std::fs::read_to_string(path).unwrap();
        LoadedConfig::from_texts(&[(path.to_path_buf(), text)], &rb_paths::MapEnv::new(""))
            .unwrap()
            .config
            .sources
    }

    #[test]
    fn hide_repos_round_trips_and_keeps_comments_and_other_keys() {
        let (_t, p) = file();
        let list = vec![
            "liminal-hq/old".to_string(),
            "liminal-hq/legacy-*".to_string(),
        ];
        set_hide_repos(&p, "work", &list).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# my config") && text.contains("# evening"));
        assert!(text.contains("# the day job") && text.contains("# reuse gh"));
        assert!(text.contains("hide_repos = [\"liminal-hq/old\", \"liminal-hq/legacy-*\"]"));
        assert_eq!(sources(&p)[0].hide_repos, list);
        assert!(sources(&p)[1].hide_repos.is_empty());
        assert_eq!(sources(&p)[0].tag_colour.as_deref(), Some("cyan"));

        set_hide_repos(&p, "work", &["liminal-hq/new".to_string()]).unwrap();
        assert_eq!(sources(&p)[0].hide_repos, ["liminal-hq/new"]);
        set_hide_repos(&p, "work", &[]).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), FILE);
    }

    #[test]
    fn saving_hide_repos_rejects_bad_entries_and_reports_sources_it_cannot_find() {
        let (_t, p) = file();
        let bad = set_hide_repos(&p, "work", &["a/*/b".to_string()]).unwrap_err();
        assert!(bad.to_string().contains("hide_repos"), "{bad}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), FILE);

        let (saved, elsewhere) = save_hide_repos(
            &p,
            &[
                ("work".into(), vec!["o/a".into()]),
                ("other".into(), vec!["x/y".into()]),
                ("ghost".into(), vec![]),
            ],
        )
        .unwrap();
        assert_eq!(saved, ["work"]);
        assert_eq!(elsewhere, ["other"]);
    }

    #[test]
    fn layout_sizes_are_written_and_removed_with_comments_kept() {
        let (_t, p) = file();
        save_layout_sizes(&p, Some(Size::Cells(52)), Some(Size::Percent(60))).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            text.contains("# evening") && text.contains("queue_width = 52"),
            "{text}"
        );
        assert!(text.contains("queue_height = \"60%\""), "{text}");
        save_layout_sizes(&p, None, None).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), FILE);
    }

    #[test]
    fn disabling_keeps_every_comment_and_enabling_drops_the_key() {
        let (_t, p) = file();
        set_enabled(&p, "work", false).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# my config") && text.contains("# evening"));
        assert!(text.contains("# the day job") && text.contains("# reuse gh"));
        assert!(text.contains("enabled = false"));
        assert!(!sources(&p)[0].enabled && sources(&p)[1].enabled);
        set_enabled(&p, "work", true).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), FILE);
    }

    #[test]
    fn removing_drops_one_source_and_leaves_the_rest_alone() {
        let (_t, p) = file();
        remove(&p, "work").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# evening") && text.contains("# my config"));
        assert!(!text.contains("the day job"));
        let left = sources(&p);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].name, "lab");
        assert_eq!(left[0].scope.projects, ["platform/api"]);
        remove(&p, "lab").unwrap();
        assert!(sources(&p).is_empty());
        assert!(matches!(remove(&p, "lab"), Err(EditError::Missing(_))));
    }

    #[test]
    fn updating_changes_only_what_differs() {
        let (_t, p) = file();
        let after = SourceSpec {
            name: "day job".into(),
            auth: AuthKind::Env("GH_WORK".into()),
            owners: vec!["liminal-hq".into(), "acme".into()],
            scope_user: true,
            ..work()
        };
        update(&p, &work(), &after).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# the day job"), "{text}");
        assert!(text.contains("auth = \"env:GH_WORK\""), "{text}");
        assert!(text.contains("tag_colour = \"cyan\""));
        assert!(text.contains("# my config") && text.contains("# evening"));
        let s = &sources(&p)[0];
        assert_eq!(s.name, "day job");
        assert_eq!(s.scope.orgs, ["liminal-hq", "acme"]);
        assert!(s.scope.user);
        assert_eq!(s.auth.as_ref().unwrap().to_string(), "env:GH_WORK");
    }

    #[test]
    fn a_nothing_changed_update_is_byte_identical() {
        let (_t, p) = file();
        update(&p, &work(), &work()).unwrap();
        update(&p, &lab(), &lab()).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), FILE);
    }

    #[test]
    fn scope_edits_reach_a_table_style_scope_and_keep_projects() {
        let (_t, p) = file();
        let after = SourceSpec {
            owners: vec!["platform".into(), "infra".into()],
            api_url: Some("https://gitlab.work.ca/api/v4".into()),
            auth: AuthKind::Command("pass show gl".into()),
            ..lab()
        };
        update(&p, &lab(), &after).unwrap();
        let s = &sources(&p)[1];
        assert_eq!(s.scope.groups, ["platform", "infra"]);
        assert_eq!(s.scope.projects, ["platform/api"]);
        assert_eq!(s.token_command.as_deref(), Some("pass show gl"));
        assert_eq!(s.api_url.as_deref(), Some("https://gitlab.work.ca/api/v4"));
        update(&p, &after, &lab()).unwrap();
        let s = &sources(&p)[1];
        assert!(s.token_command.is_none() && s.api_url.is_none());
    }

    #[test]
    fn clearing_the_last_scope_key_removes_an_inline_scope() {
        let (_t, p) = file();
        let after = SourceSpec {
            owners: Vec::new(),
            ..work()
        };
        update(&p, &work(), &after).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("orgs"), "{text}");
        assert!(sources(&p)[0].scope.orgs.is_empty());
    }

    #[test]
    fn renaming_onto_another_source_is_refused_and_nothing_changes() {
        let (_t, p) = file();
        let after = SourceSpec {
            name: "LAB".into(),
            ..work()
        };
        assert!(matches!(
            update(&p, &work(), &after),
            Err(EditError::Duplicate(_))
        ));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), FILE);
    }

    #[test]
    fn an_invalid_result_is_refused_before_writing() {
        let (_t, p) = file();
        let after = SourceSpec {
            host: "https://github.com".into(),
            ..work()
        };
        assert!(matches!(
            update(&p, &work(), &after),
            Err(EditError::Config(_))
        ));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), FILE);
    }

    #[test]
    fn adding_appends_and_the_file_stays_private() {
        let (_t, p) = file();
        add(&p, &spec("ghe", ForgeKind::GitHub, "ghe.test")).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with(FILE));
        assert_eq!(sources(&p).len(), 3);
        assert!(matches!(
            add(&p, &spec("WORK", ForgeKind::GitHub, "github.com")),
            Err(EditError::Duplicate(_))
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
