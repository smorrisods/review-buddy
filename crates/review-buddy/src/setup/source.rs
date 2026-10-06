//! A `[[source]]` entry as setup writes it, and the one-call way to append it to a config file.
//!
//! `source add` (and anything else that wants to connect an account) goes through
//! [`add_source`], so the comments, ordering and `0600` mode of the file survive.

use std::path::Path;

use rb_core::ForgeKind;
use toml_edit::Value;

use crate::config::{ConfigEditor, ConfigError, LoadedConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthKind {
    /// Reuse the `gh` or `glab` sign-in.
    Cli,
    /// A token kept in the OS keyring under `review-buddy/<host>`.
    Token,
    Env(String),
    Command(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpec {
    pub name: String,
    pub kind: ForgeKind,
    pub host: String,
    pub api_url: Option<String>,
    pub auth: AuthKind,
    /// GitHub only: include your own repositories.
    pub scope_user: bool,
    /// GitHub organisations or GitLab groups. With `scope_user` unset and this empty, a source
    /// covers everything the account can see.
    pub owners: Vec<String>,
}

fn quoted(text: &str) -> String {
    Value::from(text).to_string()
}

fn list(items: &[String]) -> String {
    let parts: Vec<_> = items.iter().map(|i| quoted(i)).collect();
    format!("[{}]", parts.join(", "))
}

impl SourceSpec {
    /// The `[[source]]` table as text, with a short comment on how it signs in.
    pub fn to_toml(&self) -> String {
        let kind = match self.kind {
            ForgeKind::GitHub => "github",
            ForgeKind::GitLab => "gitlab",
        };
        let mut out = String::from("[[source]]\n");
        out.push_str(&format!("name = {}\n", quoted(&self.name)));
        out.push_str(&format!("kind = \"{kind}\"\n"));
        out.push_str(&format!("host = {}\n", quoted(&self.host)));
        if let Some(url) = &self.api_url {
            out.push_str(&format!("api_url = {}\n", quoted(url)));
        }
        let cli = match self.kind {
            ForgeKind::GitHub => "reuse your `gh` sign-in",
            ForgeKind::GitLab => "reuse your `glab` sign-in",
        };
        match &self.auth {
            AuthKind::Cli => out.push_str(&format!("auth = \"cli\"                # {cli}\n")),
            AuthKind::Token => out.push_str(&format!(
                "auth = \"token\"              # kept in the OS keyring as review-buddy/{}\n",
                self.host
            )),
            AuthKind::Env(var) => {
                out.push_str(&format!("auth = {}\n", quoted(&format!("env:{var}"))));
            }
            AuthKind::Command(command) => {
                out.push_str("auth = \"command\"\n");
                out.push_str(&format!("token_command = {}\n", quoted(command)));
            }
        }
        let owners_key = match self.kind {
            ForgeKind::GitHub => "orgs",
            ForgeKind::GitLab => "groups",
        };
        let mut scope = Vec::new();
        if !self.owners.is_empty() {
            scope.push(format!("{owners_key} = {}", list(&self.owners)));
        }
        if self.scope_user && self.kind == ForgeKind::GitHub {
            scope.push("user = true".to_string());
        }
        if !scope.is_empty() {
            out.push_str(&format!("scope = {{ {} }}\n", scope.join(", ")));
        }
        out.push_str(&format!("tag_colour = \"{kind}\"\n"));
        out
    }
}

/// Why a source couldn't be added.
#[derive(Debug, thiserror::Error)]
pub enum AddError {
    #[error("There's already a source called {0}. Pick another name, or edit it in config.toml.")]
    Duplicate(String),
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// Appends one `[[source]]` to the config file at `path`, creating the file when it isn't there.
/// Everything already in the file is kept byte for byte. The result is validated, including
/// source names and hosts, before anything is written.
pub fn add_source(path: &Path, spec: &SourceSpec) -> Result<(), AddError> {
    let editor = ConfigEditor::open(path)?;
    let mut text = editor.text();
    let existing = LoadedConfig::from_texts(
        &[(path.to_path_buf(), text.clone())],
        &rb_paths::MapEnv::new(""),
    )?;
    if existing
        .config
        .sources
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(&spec.name))
    {
        return Err(AddError::Duplicate(spec.name.clone()));
    }
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(&spec.to_toml());
    LoadedConfig::from_texts(
        &[(path.to_path_buf(), text.clone())],
        &rb_paths::MapEnv::new(""),
    )?;
    ConfigEditor::from_text(path, &text)?.save()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> SourceSpec {
        SourceSpec {
            name: name.into(),
            kind: ForgeKind::GitHub,
            host: "github.com".into(),
            api_url: None,
            auth: AuthKind::Cli,
            scope_user: true,
            owners: vec!["liminal-hq".into()],
        }
    }

    #[test]
    fn renders_each_auth_form_and_scope() {
        let text = spec("a").to_toml();
        assert!(text.contains("auth = \"cli\""));
        assert!(text.contains("scope = { orgs = [\"liminal-hq\"], user = true }"));
        let gl = SourceSpec {
            kind: ForgeKind::GitLab,
            host: "gitlab.work.ca".into(),
            auth: AuthKind::Token,
            scope_user: true,
            owners: vec!["platform".into()],
            api_url: Some("https://gitlab.work.ca/api/v4".into()),
            ..spec("w")
        };
        let text = gl.to_toml();
        assert!(text.contains("review-buddy/gitlab.work.ca"));
        assert!(
            text.contains("scope = { groups = [\"platform\"] }"),
            "{text}"
        );
        assert!(text.contains("api_url"));
        let env = SourceSpec {
            auth: AuthKind::Env("GH_TOKEN".into()),
            owners: Vec::new(),
            scope_user: false,
            ..spec("e")
        };
        assert!(env.to_toml().contains("auth = \"env:GH_TOKEN\""));
        assert!(!env.to_toml().contains("scope"));
        let cmd = SourceSpec {
            auth: AuthKind::Command("pass show gh".into()),
            ..spec("c")
        };
        assert!(cmd.to_toml().contains("token_command = \"pass show gh\""));
    }

    #[test]
    fn add_source_appends_and_keeps_everything_else() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("config.toml");
        let before = "# mine\n[ui]\ntheme = \"dusk\"   # evening\n";
        std::fs::write(&p, before).unwrap();
        add_source(&p, &spec("github.com")).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        assert!(after.starts_with(before));
        let loaded =
            LoadedConfig::from_texts(&[(p.clone(), after)], &rb_paths::MapEnv::new("")).unwrap();
        assert_eq!(loaded.config.sources.len(), 1);
        assert_eq!(loaded.config.ui.theme, "dusk");
    }

    #[test]
    fn add_source_creates_the_file_and_refuses_a_duplicate_name() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("new/config.toml");
        add_source(&p, &spec("github.com")).unwrap();
        let again = add_source(&p, &spec("GitHub.com")).unwrap_err();
        assert!(matches!(again, AddError::Duplicate(_)));
        let loaded = LoadedConfig::from_texts(
            &[(p.clone(), std::fs::read_to_string(&p).unwrap())],
            &rb_paths::MapEnv::new(""),
        )
        .unwrap();
        assert_eq!(loaded.config.sources.len(), 1);
    }

    #[test]
    fn add_source_refuses_an_invalid_source_and_leaves_the_file() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("config.toml");
        std::fs::write(&p, "[ui]\njax = true\n").unwrap();
        let bad = SourceSpec {
            host: "https://github.com".into(),
            ..spec("x")
        };
        assert!(matches!(add_source(&p, &bad), Err(AddError::Config(_))));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "[ui]\njax = true\n");
    }
}
