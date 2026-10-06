use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;

use rb_core::{AuthMode, ForgeKind, Scope, Source, SourceId};
use toml_edit::{ImDocument, Item};

use super::error::ConfigError;
use super::schema::{AuthSetting, Kind, SourceConfig};

/// Converts a validated `[[source]]` entry into the domain type.
///
/// `env:VAR` and `command` auth both resolve to a token, so they map to
/// `AuthMode::Token`. A missing `auth` maps to `Cli`; the auth layer falls back
/// to the keyring when the CLI isn't signed in.
pub fn source_from_config(s: &SourceConfig) -> Source {
    let scope = &s.scope;
    let (owners, repos) = match s.kind {
        Kind::Github => (&scope.orgs, &scope.repos),
        Kind::Gitlab => (&scope.groups, &scope.projects),
    };
    Source {
        id: SourceId::new(&s.name),
        kind: match s.kind {
            Kind::Github => ForgeKind::GitHub,
            Kind::Gitlab => ForgeKind::GitLab,
        },
        host: s.host.clone(),
        label: s.name.clone(),
        scope: Scope {
            owners: owners.clone(),
            repos: repos.clone(),
            user: scope.user,
        },
        auth: match s.auth {
            None | Some(AuthSetting::Cli) => AuthMode::Cli,
            Some(_) => AuthMode::Token,
        },
        in_all: s.in_all,
        include_drafts: s.include_drafts,
        tag_colour: s.tag_colour.clone(),
    }
}

struct Problem {
    index: usize,
    key: Option<&'static str>,
    message: String,
}

pub(super) fn validate(
    path: &Path,
    text: &str,
    sources: &[SourceConfig],
) -> Result<(), ConfigError> {
    let Some(p) = first_problem(sources) else {
        return Ok(());
    };
    let span = locate(text, p.index, p.key);
    Err(ConfigError::invalid(
        path,
        text,
        span,
        format!("[[source]] #{}: {}", p.index + 1, p.message),
    ))
}

fn first_problem(sources: &[SourceConfig]) -> Option<Problem> {
    let mut seen = HashSet::new();
    for (index, s) in sources.iter().enumerate() {
        let fail = |key, message: &str| {
            Some(Problem {
                index,
                key,
                message: message.to_string(),
            })
        };
        if s.name.trim().is_empty() {
            return fail(Some("name"), "`name` can't be empty.");
        }
        if !seen.insert(s.name.to_lowercase()) {
            return fail(
                Some("name"),
                &format!(
                    "another source is already called `{}`, so give each one its own name.",
                    s.name
                ),
            );
        }
        if s.host.is_empty() || s.host.contains("://") || s.host.contains('/') {
            return fail(
                Some("host"),
                &format!(
                    "`host` should be a bare host name like `github.com`, not `{}`.",
                    s.host
                ),
            );
        }
        if let Some(Err(e)) = s
            .tag_colour
            .as_deref()
            .map(str::parse::<rb_theme::TagColour>)
        {
            return fail(Some("tag_colour"), &format!("{e}."));
        }
        if let Some(url) = &s.api_url {
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return fail(Some("api_url"), "`api_url` should start with https://.");
            }
        }
        if s.auth == Some(AuthSetting::Command)
            && s.token_command.as_deref().is_none_or(str::is_empty)
        {
            return fail(
                Some("auth"),
                "`auth = \"command\"` needs a `token_command`.",
            );
        }
        let scope = &s.scope;
        let (wrong, right) = match s.kind {
            Kind::Github => (
                !scope.groups.is_empty() || !scope.projects.is_empty(),
                "orgs, repos or user",
            ),
            Kind::Gitlab => (
                !scope.orgs.is_empty() || !scope.repos.is_empty() || scope.user,
                "groups or projects",
            ),
        };
        if wrong {
            return fail(
                Some("scope"),
                &format!("this scope doesn't fit the source kind, use {right}."),
            );
        }
    }
    None
}

fn locate(text: &str, index: usize, key: Option<&str>) -> Option<Range<usize>> {
    let doc = ImDocument::parse(text.to_string()).ok()?;
    let tables = doc.get("source")?.as_array_of_tables()?;
    let table = tables.get(index)?;
    key.and_then(|k| table.get(k))
        .and_then(Item::span)
        .or_else(|| table.span())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LoadedConfig;
    use rb_paths::MapEnv;
    use std::path::PathBuf;

    fn load(text: &str) -> Result<LoadedConfig, ConfigError> {
        LoadedConfig::from_texts(
            &[(PathBuf::from("/c.toml"), text.to_string())],
            &MapEnv::new("/h"),
        )
    }

    const GH: &str = "[[source]]\nname = \"a\"\nkind = \"github\"\nhost = \"github.com\"\n";

    #[test]
    fn converts_github_and_gitlab_sources() {
        let l = load(&format!(
            "{GH}scope = {{ orgs = [\"o\"], repos = [\"o/r\"], user = true }}\ntag_colour = \"github\"\n\n\
             [[source]]\nname = \"w\"\nkind = \"gitlab\"\nhost = \"gitlab.work.ca\"\nauth = \"env:GL\"\n\
             scope = {{ groups = [\"g\"], projects = [\"g/p\"] }}\ninclude_drafts = true\nin_all = false\n"
        ))
        .unwrap();
        let s = l.config.sources();
        assert_eq!(s[0].id.as_str(), "a");
        assert_eq!(s[0].kind, ForgeKind::GitHub);
        assert_eq!(s[0].scope.owners, ["o"]);
        assert!(s[0].scope.user);
        assert_eq!(s[0].auth, AuthMode::Cli);
        assert_eq!(s[0].tag_colour.as_deref(), Some("github"));
        assert_eq!(s[1].kind, ForgeKind::GitLab);
        assert_eq!(s[1].scope.owners, ["g"]);
        assert_eq!(s[1].scope.repos, ["g/p"]);
        assert_eq!(s[1].auth, AuthMode::Token);
        assert!(s[1].include_drafts && !s[1].in_all);
    }

    #[test]
    fn omitted_scope_means_everything_and_defaults_apply() {
        let l = load(GH).unwrap();
        let s = &l.config.sources()[0];
        assert!(s.scope.is_everything());
        assert!(s.in_all && !s.include_drafts);
    }

    #[test]
    fn disabled_sources_are_skipped() {
        let l = load(&format!("{GH}enabled = false\n")).unwrap();
        assert_eq!(l.config.sources.len(), 1);
        assert!(l.config.sources().is_empty());
    }

    #[test]
    fn auth_forms_parse() {
        let l = load(&format!("{GH}auth = \"env:GITHUB_TOKEN\"\n")).unwrap();
        assert_eq!(
            l.config.sources[0].auth,
            Some(AuthSetting::Env("GITHUB_TOKEN".into()))
        );
        let e = load(&format!("{GH}auth = \"password\"\n")).unwrap_err();
        assert_eq!(e.line(), Some(5));
        assert!(e.to_string().contains("env:VAR_NAME"));
    }

    #[test]
    fn missing_required_keys_are_reported() {
        let e = load("[[source]]\nname = \"a\"\nkind = \"github\"\n").unwrap_err();
        assert!(e.to_string().contains("host"), "{e}");
        assert!(e.line().is_some());
    }

    #[test]
    fn unknown_kind_is_reported_on_its_line() {
        let e =
            load("\n[[source]]\nname = \"a\"\nkind = \"bitbucket\"\nhost = \"x\"\n").unwrap_err();
        assert_eq!(e.line(), Some(4));
    }

    #[test]
    fn duplicate_names_point_at_the_second() {
        let e = load(&format!("{GH}\n{GH}")).unwrap_err();
        assert_eq!(e.line(), Some(7));
        assert!(e.to_string().contains("[[source]] #2"));
    }

    #[test]
    fn command_auth_needs_a_command() {
        let e = load(&format!("{GH}auth = \"command\"\n")).unwrap_err();
        assert_eq!(e.line(), Some(5));
        assert!(e.to_string().contains("token_command"));
        assert!(load(&format!(
            "{GH}auth = \"command\"\ntoken_command = \"pass show x\"\n"
        ))
        .is_ok());
    }

    #[test]
    fn host_must_be_bare() {
        let e =
            load("[[source]]\nname = \"a\"\nkind = \"github\"\nhost = \"https://github.com\"\n")
                .unwrap_err();
        assert_eq!(e.line(), Some(4));
    }

    #[test]
    fn scope_must_fit_the_kind() {
        let e = load(&format!("{GH}scope = {{ groups = [\"g\"] }}\n")).unwrap_err();
        assert_eq!(e.line(), Some(5));
        assert!(e.to_string().contains("orgs, repos or user"));
    }

    #[test]
    fn tag_colour_accepts_roles_and_hex_and_rejects_the_rest() {
        for ok in ["gitlab", "accent", "cyan", "interactive", "#3fb950"] {
            assert!(
                load(&format!("{GH}tag_colour = \"{ok}\"\n")).is_ok(),
                "{ok}"
            );
        }
        let e = load(&format!("{GH}tag_colour = \"teal\"\n")).unwrap_err();
        assert_eq!(e.line(), Some(5));
        let text = e.to_string();
        assert!(text.contains("[[source]] #1") && text.contains("isn't a tag colour"));
        assert!(load(&format!("{GH}tag_colour = \"#12\"\n")).is_err());
    }

    #[test]
    fn api_url_needs_a_scheme() {
        let e = load(&format!("{GH}api_url = \"ghe.corp/api\"\n")).unwrap_err();
        assert_eq!(e.line(), Some(5));
    }
}
