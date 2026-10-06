//! Change selectors: what the user typed, and which source, repository and change it means.
//!
//! Everything here is pure. Repo inference takes the remote and branch as plain values, and
//! nothing makes a network call to decide which repository was meant.

use rb_core::{ForgeKind, Source, SourceId};
use thiserror::Error;

/// A repository named without a source, optionally with the host that picks the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub host: Option<String>,
    pub path: String,
}

impl RepoRef {
    /// Parses `[host/]owner/repo`. A first segment containing a dot or a port is a host, so GitLab
    /// subgroups (`platform/infra/terraform`) stay paths.
    pub fn parse(text: &str) -> Result<Self, SelectorError> {
        let text = text.trim().trim_matches('/');
        let bad = || SelectorError::BadRepo(text.to_string());
        let segments: Vec<&str> = text.split('/').collect();
        if segments.iter().any(|s| s.is_empty() || has_space(s)) {
            return Err(bad());
        }
        match segments.as_slice() {
            [host, rest @ ..] if (host.contains('.') || host.contains(':')) && !rest.is_empty() => {
                if rest.len() < 2 {
                    return Err(bad());
                }
                Ok(Self {
                    host: Some(host.to_ascii_lowercase()),
                    path: rest.join("/"),
                })
            }
            [_, _, ..] => Ok(Self {
                host: None,
                path: segments.join("/"),
            }),
            _ => Err(bad()),
        }
    }
}

/// True when `wanted` names the same host as `source`. A port on only one side is ignored,
/// since an ssh remote never carries the web port.
pub fn host_matches(source: &str, wanted: &str) -> bool {
    if source.eq_ignore_ascii_case(wanted) {
        return true;
    }
    let split = |h: &str| match h.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => {
            (name.to_string(), Some(port.to_string()))
        }
        _ => (h.to_string(), None),
    };
    let ((a, ap), (b, bp)) = (split(source), split(wanted));
    a.eq_ignore_ascii_case(&b) && (ap.is_none() || bp.is_none())
}

/// The relative URL root of a self-hosted instance, from its API address:
/// `https://example.com/gitlab/api/v4` is `gitlab`. Empty when there is none.
pub fn url_root(api_url: &str) -> String {
    let rest = api_url.split_once("://").map_or(api_url, |(_, r)| r);
    let path = rest.split_once('/').map_or("", |(_, p)| p);
    let path = path.trim_matches('/');
    let path = ["api/v3", "api/v4"]
        .iter()
        .find_map(|s| path.strip_suffix(s))
        .unwrap_or(path);
    path.trim_matches('/').to_string()
}

/// `repo` without the instance's relative URL root.
fn strip_root<'a>(repo: &'a str, root: &str) -> &'a str {
    repo.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .filter(|rest| !root.is_empty() && rest.contains('/'))
        .unwrap_or(repo)
}

fn has_space(s: &str) -> bool {
    s.chars().any(char::is_whitespace)
}

/// A web URL that points at one change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlRef {
    pub host: String,
    pub kind: ForgeKind,
    pub repo: String,
    pub number: u64,
}

/// What the user typed, understood but not yet matched against sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Url(UrlRef),
    Qualified {
        source: Option<String>,
        repo: String,
        number: u64,
    },
    Number(u64),
    Branch(String),
    Current,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SelectorError {
    #[error("Couldn't read `{input}` as a change. {hint}")]
    BadReference { input: String, hint: String },
    #[error("Couldn't read `{0}` as a repository.\nUse owner/repo, optionally with the host first, like github.com/owner/repo.")]
    BadRepo(String),
    #[error("Couldn't tell which repository you meant.\nPass --repo owner/repo, or run this inside a git repository whose remote matches a source.")]
    NoRepository,
    #[error("Couldn't tell which branch you're on.\nPass a number or a URL, e.g. review-buddy pr view 214.")]
    NoBranch,
    #[error("There's no source called {name}.\nConfigured sources: {known}.")]
    UnknownSource { name: String, known: String },
    #[error("No configured source uses {0}.\nSee review-buddy source list for what's configured.")]
    NoSourceForHost(String),
    #[error(
        "No configured source covers {repo}.\nTry --source, or check review-buddy source list."
    )]
    NoSourceForRepo { repo: String },
    #[error("More than one source could own {what}: {candidates}.\nPass --source with one of those names.")]
    Ambiguous { what: String, candidates: String },
}

fn bad_reference(input: &str, hint: &str) -> SelectorError {
    SelectorError::BadReference {
        input: input.to_string(),
        hint: hint.to_string(),
    }
}

const FORMS_HINT: &str = "Try a URL, owner/repo#214, a number, or a branch name.";

fn parse_number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|n| *n > 0)
}

/// Parses a selector. `None` or blank means the current branch.
pub fn parse(input: Option<&str>) -> Result<Selector, SelectorError> {
    let Some(input) = input.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Selector::Current);
    };
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return parse_url(input).map(Selector::Url);
    }
    if has_space(input) {
        return Err(bad_reference(input, FORMS_HINT));
    }
    if let Some(n) = input
        .strip_prefix(['#', '!'])
        .map(|rest| parse_number(rest).ok_or_else(|| bad_reference(input, FORMS_HINT)))
    {
        return Ok(Selector::Number(n?));
    }
    if let Some(n) = parse_number(input) {
        return Ok(Selector::Number(n));
    }
    if input.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad_reference(input, FORMS_HINT));
    }
    let Some(at) = input.rfind(['#', '!']) else {
        return Ok(Selector::Branch(input.to_string()));
    };
    let (prefix, suffix) = (&input[..at], &input[at + 1..]);
    let number = parse_number(suffix).ok_or_else(|| bad_reference(input, FORMS_HINT))?;
    let (source, repo) = match prefix.split_once(':') {
        Some((source, repo)) if !source.is_empty() && !repo.is_empty() => {
            (Some(source.to_string()), repo)
        }
        Some(_) => return Err(bad_reference(input, FORMS_HINT)),
        None => (None, prefix),
    };
    if repo.split('/').any(str::is_empty) {
        return Err(bad_reference(input, FORMS_HINT));
    }
    Ok(Selector::Qualified {
        source,
        repo: repo.to_string(),
        number,
    })
}

fn parse_url(input: &str) -> Result<UrlRef, SelectorError> {
    let unsupported = || {
        bad_reference(
            input,
            "Use a pull request or merge request address, like https://github.com/owner/repo/pull/214.",
        )
    };
    let rest = input.split_once("://").map_or(input, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = rest.split_once('/').ok_or_else(unsupported)?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err(unsupported());
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if let Some(at) = segments.iter().position(|s| *s == "merge_requests") {
        let number = segments.get(at + 1).and_then(|n| parse_number(n));
        let repo: Vec<&str> = segments[..at]
            .iter()
            .copied()
            .filter(|s| *s != "-")
            .collect();
        if let (Some(number), true) = (number, repo.len() >= 2) {
            return Ok(UrlRef {
                host,
                kind: ForgeKind::GitLab,
                repo: repo.join("/"),
                number,
            });
        }
    }
    if let Some(at) = segments.iter().position(|s| *s == "pull") {
        let number = segments.get(at + 1).and_then(|n| parse_number(n));
        if let (Some(number), true) = (number, at >= 2) {
            return Ok(UrlRef {
                host,
                kind: ForgeKind::GitHub,
                repo: segments[..at].join("/"),
                number,
            });
        }
    }
    Err(unsupported())
}

/// Which change inside the resolved repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Which {
    Number(u64),
    Branch(String),
}

/// A selector matched to a source and repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub source: SourceId,
    pub kind: ForgeKind,
    pub host: String,
    pub repo: String,
    pub which: Which,
}

/// What resolution may look at besides the selector itself.
#[derive(Debug, Clone, Default)]
pub struct Inference<'a> {
    pub sources: &'a [Source],
    /// `--source` names; empty means every source.
    pub only_sources: &'a [String],
    /// `--repo`, which beats the git remote.
    pub repo_flag: Option<RepoRef>,
    /// The cwd repository's remote, already mapped through `insteadOf`.
    pub git_remote: Option<RepoRef>,
    pub current_branch: Option<String>,
    /// Relative URL roots by source host, for instances served below a path
    /// (`https://example.com/gitlab`). URLs and remotes on that host carry the root.
    pub url_roots: &'a [(String, String)],
}

impl Inference<'_> {
    fn pool(&self) -> Result<Vec<&Source>, SelectorError> {
        if self.only_sources.is_empty() {
            return Ok(self.sources.iter().collect());
        }
        self.only_sources
            .iter()
            .map(|name| find_source(self.sources, name))
            .collect()
    }

    fn repo_hint(&self) -> Result<&RepoRef, SelectorError> {
        self.repo_flag
            .as_ref()
            .or(self.git_remote.as_ref())
            .ok_or(SelectorError::NoRepository)
    }
}

fn find_source<'a>(sources: &'a [Source], name: &str) -> Result<&'a Source, SelectorError> {
    sources
        .iter()
        .find(|s| s.id.as_str() == name || s.label == name)
        .ok_or_else(|| SelectorError::UnknownSource {
            name: name.to_string(),
            known: if sources.is_empty() {
                "none".to_string()
            } else {
                sources
                    .iter()
                    .map(|s| s.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        })
}

fn covers(source: &Source, repo: &str) -> bool {
    let scope = &source.scope;
    let under = |owner: &str| {
        repo.strip_prefix(owner)
            .is_some_and(|rest| rest.starts_with('/'))
    };
    scope.is_everything()
        || scope.user
        || scope.owners.iter().any(|o| under(o))
        || scope.repos.iter().any(|r| r == repo)
}

/// The repo path as `source` would name it: a bare name is completed from the source's scope.
fn expand(source: &Source, repo: &str) -> Option<String> {
    if repo.contains('/') {
        return covers(source, repo).then(|| repo.to_string());
    }
    let from_repos = source
        .scope
        .repos
        .iter()
        .find(|r| r.rsplit('/').next() == Some(repo));
    if let Some(full) = from_repos {
        return Some(full.clone());
    }
    match source.scope.owners.as_slice() {
        [owner] => Some(format!("{owner}/{repo}")),
        _ => None,
    }
}

fn pick(
    pool: &[&Source],
    repo: &str,
    host: Option<&str>,
    kind: Option<ForgeKind>,
    roots: &[(String, String)],
) -> Result<(SourceId, ForgeKind, String, String), SelectorError> {
    let on_host: Vec<&&Source> = pool
        .iter()
        .filter(|s| host.is_none_or(|h| host_matches(&s.host, h)))
        .filter(|s| kind.is_none_or(|k| s.kind == k))
        .collect();
    if on_host.is_empty() {
        return Err(match host {
            Some(h) => SelectorError::NoSourceForHost(h.to_string()),
            None => SelectorError::NoSourceForRepo {
                repo: repo.to_string(),
            },
        });
    }
    let matches: Vec<(&Source, String)> = on_host
        .into_iter()
        .filter_map(|s| {
            let root = roots
                .iter()
                .find(|(h, _)| h.eq_ignore_ascii_case(&s.host))
                .map_or("", |(_, r)| r.as_str());
            let repo = if host.is_some() {
                strip_root(repo, root)
            } else {
                repo
            };
            expand(s, repo).map(|full| (&**s, full))
        })
        .collect();
    match matches.as_slice() {
        [] => Err(SelectorError::NoSourceForRepo {
            repo: repo.to_string(),
        }),
        [(s, full)] => Ok((s.id.clone(), s.kind, s.host.clone(), full.clone())),
        [(first, full), rest @ ..]
            if rest
                .iter()
                .all(|(s, f)| s.kind == first.kind && s.host == first.host && f == full) =>
        {
            Ok((
                first.id.clone(),
                first.kind,
                first.host.clone(),
                full.clone(),
            ))
        }
        many => Err(SelectorError::Ambiguous {
            what: repo.to_string(),
            candidates: many
                .iter()
                .map(|(s, _)| format!("{} ({})", s.id, s.host))
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

/// Matches a parsed selector to one source, repository and change.
pub fn resolve(selector: &Selector, inference: &Inference<'_>) -> Result<Target, SelectorError> {
    let pool = inference.pool()?;
    let target = |picked: (SourceId, ForgeKind, String, String), which| Target {
        source: picked.0,
        kind: picked.1,
        host: picked.2,
        repo: picked.3,
        which,
    };
    match selector {
        Selector::Url(url) => {
            let picked = pick(
                &pool,
                &url.repo,
                Some(&url.host),
                Some(url.kind),
                inference.url_roots,
            )?;
            if url.kind == ForgeKind::GitHub && picked.3.split('/').count() != 2 {
                return Err(SelectorError::NoSourceForRepo {
                    repo: url.repo.clone(),
                });
            }
            Ok(target(picked, Which::Number(url.number)))
        }
        Selector::Qualified {
            source: Some(name),
            repo,
            number,
        } => {
            let source = find_source(inference.sources, name)?;
            let picked = pick(&[source], repo, None, None, inference.url_roots)?;
            Ok(target(picked, Which::Number(*number)))
        }
        Selector::Qualified {
            source: None,
            repo,
            number,
        } => {
            let picked = pick(&pool, repo, None, None, inference.url_roots)?;
            Ok(target(picked, Which::Number(*number)))
        }
        Selector::Number(number) => {
            let hint = inference.repo_hint()?;
            let picked = pick(
                &pool,
                &hint.path,
                hint.host.as_deref(),
                None,
                inference.url_roots,
            )?;
            Ok(target(picked, Which::Number(*number)))
        }
        Selector::Branch(branch) => {
            let hint = inference.repo_hint()?;
            let picked = pick(
                &pool,
                &hint.path,
                hint.host.as_deref(),
                None,
                inference.url_roots,
            )?;
            Ok(target(picked, Which::Branch(branch.clone())))
        }
        Selector::Current => {
            let branch = inference
                .current_branch
                .clone()
                .ok_or(SelectorError::NoBranch)?;
            let hint = inference.repo_hint()?;
            let picked = pick(
                &pool,
                &hint.path,
                hint.host.as_deref(),
                None,
                inference.url_roots,
            )?;
            Ok(target(picked, Which::Branch(branch)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::{AuthMode, Scope};

    fn source(id: &str, kind: ForgeKind, host: &str, owners: &[&str]) -> Source {
        Source {
            id: SourceId::new(id),
            kind,
            host: host.into(),
            label: id.into(),
            scope: Scope {
                owners: owners.iter().map(|s| s.to_string()).collect(),
                ..Scope::default()
            },
            auth: AuthMode::Cli,
            in_all: true,
            include_drafts: true,
            tag_colour: None,
        }
    }

    fn sources() -> Vec<Source> {
        vec![
            source(
                "liminal-hq",
                ForgeKind::GitHub,
                "github.com",
                &["liminal-hq"],
            ),
            source(
                "platform",
                ForgeKind::GitLab,
                "gitlab.work.ca",
                &["platform"],
            ),
        ]
    }

    fn qualified(source: Option<&str>, repo: &str, number: u64) -> Selector {
        Selector::Qualified {
            source: source.map(str::to_string),
            repo: repo.into(),
            number,
        }
    }

    #[test]
    fn nothing_is_the_current_branch() {
        assert_eq!(parse(None), Ok(Selector::Current));
        assert_eq!(parse(Some("  ")), Ok(Selector::Current));
    }

    #[test]
    fn github_and_gitlab_urls() {
        assert_eq!(
            parse(Some("https://github.com/liminal-hq/spindle/pull/214")),
            Ok(Selector::Url(UrlRef {
                host: "github.com".into(),
                kind: ForgeKind::GitHub,
                repo: "liminal-hq/spindle".into(),
                number: 214
            }))
        );
        assert_eq!(
            parse(Some(
                "https://gitlab.work.ca/platform/flow/-/merge_requests/88"
            )),
            Ok(Selector::Url(UrlRef {
                host: "gitlab.work.ca".into(),
                kind: ForgeKind::GitLab,
                repo: "platform/flow".into(),
                number: 88
            }))
        );
    }

    #[test]
    fn urls_tolerate_subgroups_suffixes_queries_and_old_gitlab_paths() {
        let gh = parse(Some("https://GitHub.com/a/b/pull/7/files?w=1#diff-x")).unwrap();
        assert!(
            matches!(&gh, Selector::Url(u) if u.number == 7 && u.repo == "a/b" && u.host == "github.com")
        );
        let gl = parse(Some(
            "https://gitlab.work.ca/platform/infra/terraform/-/merge_requests/3/diffs",
        ))
        .unwrap();
        assert!(
            matches!(&gl, Selector::Url(u) if u.repo == "platform/infra/terraform" && u.number == 3)
        );
        let old = parse(Some("https://gitlab.work.ca/g/p/merge_requests/9")).unwrap();
        assert!(matches!(&old, Selector::Url(u) if u.repo == "g/p" && u.number == 9));
        let auth = parse(Some("https://me@github.com/a/b/pull/1")).unwrap();
        assert!(matches!(&auth, Selector::Url(u) if u.host == "github.com"));
    }

    #[test]
    fn every_gitlab_url_shape_gives_the_project_and_number() {
        for (url, repo, number) in [
            ("https://gitlab.com/g/p/-/merge_requests/12", "g/p", 12),
            (
                "https://gitlab.com/g/s/p/-/merge_requests/12/diffs",
                "g/s/p",
                12,
            ),
            (
                "https://gitlab.com/g/s/t/p/-/merge_requests/12/commits",
                "g/s/t/p",
                12,
            ),
            ("https://gitlab.com/g/p/merge_requests/12", "g/p", 12),
            ("https://gitlab.com/g/p/merge_requests/12/diffs", "g/p", 12),
            (
                "https://gitlab.com/g/p/-/merge_requests/12?tab=pipelines",
                "g/p",
                12,
            ),
            (
                "https://gitlab.com/g/p/-/merge_requests/12#note_5",
                "g/p",
                12,
            ),
            ("https://gitlab.com/g/p/-/merge_requests/12/", "g/p", 12),
            ("http://localhost:8080/g/p/-/merge_requests/3", "g/p", 3),
            (
                "https://tok@GitLab.Example.com/g/p/-/merge_requests/3",
                "g/p",
                3,
            ),
        ] {
            let Ok(Selector::Url(u)) = parse(Some(url)) else {
                panic!("{url} didn't parse");
            };
            assert_eq!(
                (u.repo.as_str(), u.number, u.kind),
                (repo, number, ForgeKind::GitLab),
                "{url}"
            );
        }
    }

    #[test]
    fn gitlab_refs_take_subgroups_and_either_sigil() {
        assert_eq!(
            parse(Some("platform/infra/terraform!3")),
            Ok(qualified(None, "platform/infra/terraform", 3))
        );
        assert_eq!(
            parse(Some("platform:platform/infra/terraform#3")),
            Ok(qualified(Some("platform"), "platform/infra/terraform", 3))
        );
        assert_eq!(parse(Some("!1182")), Ok(Selector::Number(1182)));
        let repo = RepoRef::parse("gitlab.com/platform/infra/terraform").unwrap();
        assert_eq!(repo.host.as_deref(), Some("gitlab.com"));
        assert_eq!(repo.path, "platform/infra/terraform");
        assert_eq!(
            RepoRef::parse("platform/infra/terraform").unwrap().path,
            "platform/infra/terraform"
        );
    }

    #[test]
    fn group_scopes_cover_their_subgroups_and_not_lookalikes() {
        let lab = source("lab", ForgeKind::GitLab, "gitlab.test", &["platform/infra"]);
        assert!(covers(&lab, "platform/infra/terraform"));
        assert!(covers(&lab, "platform/infra/a/b"));
        assert!(!covers(&lab, "platform/other"));
        assert!(!covers(&lab, "platform/infrastructure/x"));
        let top = source("top", ForgeKind::GitLab, "gitlab.test", &["platform"]);
        assert!(covers(&top, "platform/infra/terraform"));
    }

    #[test]
    fn urls_below_a_web_root_resolve_against_the_project_path() {
        let sources = vec![source("lab", ForgeKind::GitLab, "gl.test", &["platform"])];
        let roots = vec![("gl.test".to_string(), "gitlab".to_string())];
        let inf = Inference {
            sources: &sources,
            url_roots: &roots,
            ..Inference::default()
        };
        let sel = parse(Some(
            "https://gl.test/gitlab/platform/infra/terraform/-/merge_requests/12",
        ))
        .unwrap();
        assert_eq!(
            resolve(&sel, &inf).unwrap().repo,
            "platform/infra/terraform"
        );
        let plain = Inference {
            sources: &sources,
            ..Inference::default()
        };
        assert!(resolve(&sel, &plain).is_err());
    }

    #[test]
    fn the_web_root_comes_from_the_api_address() {
        assert_eq!(url_root("https://gl.test/gitlab/api/v4"), "gitlab");
        assert_eq!(url_root("https://gl.test/a/b/api/v4/"), "a/b");
        assert_eq!(url_root("https://gl.test/api/v4"), "");
        assert_eq!(url_root("https://gl.test"), "");
        assert_eq!(url_root("http://127.0.0.1:4000"), "");
    }

    #[test]
    fn unsupported_urls_say_what_works() {
        for url in [
            "https://github.com/a/b",
            "https://github.com/a/b/issues/3",
            "https://github.com/a/b/pull/x",
            "https://github.com/a/b/pull/0",
            "https://gitlab.work.ca/p/-/merge_requests/3",
            "https://github.com",
            "https://",
        ] {
            let err = parse(Some(url)).unwrap_err();
            assert!(
                err.to_string()
                    .contains("pull request or merge request address"),
                "{url}: {err}"
            );
        }
    }

    #[test]
    fn urls_keep_ports_and_path_prefixes() {
        let gh = parse(Some("http://ghe.corp.test:8080/ghe/acme/widgets/pull/7")).unwrap();
        assert!(matches!(&gh, Selector::Url(u)
            if u.host == "ghe.corp.test:8080" && u.repo == "ghe/acme/widgets" && u.number == 7));
        let gl = parse(Some(
            "https://git.corp.test:8929/gitlab/plat/infra/tf/-/merge_requests/3",
        ))
        .unwrap();
        assert!(matches!(&gl, Selector::Url(u)
            if u.host == "git.corp.test:8929" && u.repo == "gitlab/plat/infra/tf"));
    }

    fn enterprise_sources() -> Vec<Source> {
        vec![
            source("ghe", ForgeKind::GitHub, "ghe.corp.test:8080", &["acme"]),
            source("lab", ForgeKind::GitLab, "git.corp.test", &["plat"]),
        ]
    }

    #[test]
    fn urls_on_self_hosted_hosts_drop_the_relative_root() {
        let sources = enterprise_sources();
        let roots = vec![
            ("ghe.corp.test:8080".to_string(), "ghe".to_string()),
            ("git.corp.test".to_string(), "gitlab".to_string()),
        ];
        let inf = Inference {
            sources: &sources,
            url_roots: &roots,
            ..Inference::default()
        };
        let sel = parse(Some("http://ghe.corp.test:8080/ghe/acme/widgets/pull/7")).unwrap();
        let t = resolve(&sel, &inf).unwrap();
        assert_eq!(
            (t.source, t.repo.as_str(), t.which),
            (SourceId::new("ghe"), "acme/widgets", Which::Number(7))
        );
        let sel = parse(Some(
            "https://git.corp.test:8929/gitlab/plat/infra/tf/-/merge_requests/3",
        ))
        .unwrap();
        let t = resolve(&sel, &inf).unwrap();
        assert_eq!(
            (t.source, t.repo.as_str()),
            (SourceId::new("lab"), "plat/infra/tf")
        );
    }

    #[test]
    fn a_github_url_with_extra_segments_and_no_root_is_not_a_repository() {
        let sources = enterprise_sources();
        let inf = Inference {
            sources: &sources,
            ..Inference::default()
        };
        let sel = parse(Some("http://ghe.corp.test:8080/acme/widgets/extra/pull/7")).unwrap();
        assert!(matches!(
            resolve(&sel, &inf),
            Err(SelectorError::NoSourceForRepo { .. })
        ));
    }

    #[test]
    fn remote_hints_match_with_or_without_a_port_and_root() {
        let sources = enterprise_sources();
        let roots = vec![("git.corp.test".to_string(), "gitlab".to_string())];
        let hint = |host: &str, path: &str| {
            Some(RepoRef {
                host: Some(host.into()),
                path: path.into(),
            })
        };
        for (remote, repo) in [
            (hint("git.corp.test", "plat/infra/tf"), "plat/infra/tf"),
            (hint("git.corp.test:8929", "plat/infra/tf"), "plat/infra/tf"),
            (
                hint("git.corp.test", "gitlab/plat/infra/tf"),
                "plat/infra/tf",
            ),
        ] {
            let inf = Inference {
                sources: &sources,
                url_roots: &roots,
                git_remote: remote,
                ..Inference::default()
            };
            let t = resolve(&Selector::Number(5), &inf).unwrap();
            assert_eq!((t.source, t.repo.as_str()), (SourceId::new("lab"), repo));
        }
        let inf = Inference {
            sources: &sources,
            git_remote: hint("ghe.corp.test", "acme/widgets"),
            ..Inference::default()
        };
        let t = resolve(&Selector::Number(5), &inf).unwrap();
        assert_eq!(t.source, SourceId::new("ghe"));
    }

    #[test]
    fn host_matching_and_url_roots() {
        assert!(host_matches("GitHub.com", "github.com"));
        assert!(host_matches("ghe.test:8443", "ghe.test"));
        assert!(host_matches("ghe.test", "ghe.test:22"));
        assert!(!host_matches("ghe.test:8443", "ghe.test:9443"));
        assert!(!host_matches("ghe.test", "other.test"));
        assert_eq!(url_root("https://x.test/gitlab/api/v4/"), "gitlab");
        assert_eq!(url_root("http://x.test:8080/a/b/api/v3"), "a/b");
        assert_eq!(url_root("https://x.test/api/v4"), "");
        assert_eq!(url_root("https://api.github.com"), "");
    }

    #[test]
    fn repo_flag_accepts_a_host_with_a_port() {
        assert_eq!(
            RepoRef::parse("localhost:8080/a/b"),
            Ok(RepoRef {
                host: Some("localhost:8080".into()),
                path: "a/b".into()
            })
        );
    }

    #[test]
    fn qualified_forms() {
        assert_eq!(
            parse(Some("liminal-hq/spindle#214")),
            Ok(qualified(None, "liminal-hq/spindle", 214))
        );
        assert_eq!(
            parse(Some("platform/flow!88")),
            Ok(qualified(None, "platform/flow", 88))
        );
        assert_eq!(
            parse(Some("platform:platform/flow!88")),
            Ok(qualified(Some("platform"), "platform/flow", 88))
        );
        assert_eq!(
            parse(Some("liminal-hq:spindle#214")),
            Ok(qualified(Some("liminal-hq"), "spindle", 214))
        );
        assert_eq!(
            parse(Some("spindle!214")),
            Ok(qualified(None, "spindle", 214))
        );
    }

    #[test]
    fn numbers_accept_either_sigil() {
        for text in ["214", "#214", "!214"] {
            assert_eq!(parse(Some(text)), Ok(Selector::Number(214)));
        }
    }

    #[test]
    fn branches_pass_through() {
        assert_eq!(
            parse(Some("feat/titleset-menus")),
            Ok(Selector::Branch("feat/titleset-menus".into()))
        );
        assert_eq!(parse(Some("main")), Ok(Selector::Branch("main".into())));
    }

    #[test]
    fn malformed_references_are_calm_errors() {
        for text in [
            "#",
            "!x",
            "#0",
            "a/b#",
            "a/b#x",
            "a//b#3",
            ":a/b#3",
            "s:#3",
            "two words",
            "0",
        ] {
            let err = parse(Some(text)).expect_err(text);
            let msg = err.to_string();
            assert!(msg.contains("Couldn't read"), "{text}: {msg}");
            assert!(msg.contains("Try a URL"), "{text}: {msg}");
        }
    }

    #[test]
    fn repo_flag_forms() {
        assert_eq!(
            RepoRef::parse("liminal-hq/spindle"),
            Ok(RepoRef {
                host: None,
                path: "liminal-hq/spindle".into()
            })
        );
        assert_eq!(
            RepoRef::parse("platform/infra/terraform"),
            Ok(RepoRef {
                host: None,
                path: "platform/infra/terraform".into()
            })
        );
        assert_eq!(
            RepoRef::parse("GitLab.work.ca/platform/flow"),
            Ok(RepoRef {
                host: Some("gitlab.work.ca".into()),
                path: "platform/flow".into()
            })
        );
        for bad in ["spindle", "", "a//b", "github.com/solo", "a b/c"] {
            assert!(
                matches!(RepoRef::parse(bad), Err(SelectorError::BadRepo(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn urls_pick_the_source_by_host() {
        let sources = sources();
        let inf = Inference {
            sources: &sources,
            ..Inference::default()
        };
        let sel = parse(Some(
            "https://gitlab.work.ca/platform/flow/-/merge_requests/88",
        ))
        .unwrap();
        let target = resolve(&sel, &inf).unwrap();
        assert_eq!(target.source, SourceId::new("platform"));
        assert_eq!(target.kind, ForgeKind::GitLab);
        assert_eq!(target.which, Which::Number(88));

        let sel = parse(Some("https://example.com/a/b/pull/1")).unwrap();
        assert_eq!(
            resolve(&sel, &inf),
            Err(SelectorError::NoSourceForHost("example.com".into()))
        );
    }

    #[test]
    fn source_qualified_completes_a_bare_repo_from_the_scope() {
        let sources = sources();
        let inf = Inference {
            sources: &sources,
            ..Inference::default()
        };
        let target = resolve(&qualified(Some("liminal-hq"), "spindle", 214), &inf).unwrap();
        assert_eq!(target.repo, "liminal-hq/spindle");
        assert_eq!(target.host, "github.com");
        let err = resolve(&qualified(Some("nope"), "spindle", 1), &inf).unwrap_err();
        assert!(err
            .to_string()
            .contains("Configured sources: liminal-hq, platform"));
    }

    #[test]
    fn repo_qualified_must_match_exactly_one_source() {
        let mut sources = sources();
        let inf = Inference {
            sources: &sources,
            ..Inference::default()
        };
        assert_eq!(
            resolve(&qualified(None, "liminal-hq/spindle", 214), &inf)
                .unwrap()
                .source,
            SourceId::new("liminal-hq")
        );
        assert!(matches!(
            resolve(&qualified(None, "stranger/thing", 1), &inf),
            Err(SelectorError::NoSourceForRepo { .. })
        ));

        sources.push(source(
            "personal",
            ForgeKind::GitHub,
            "github.com",
            &["liminal-hq"],
        ));
        let inf = Inference {
            sources: &sources,
            ..Inference::default()
        };
        assert_eq!(
            resolve(&qualified(None, "liminal-hq/spindle", 214), &inf)
                .unwrap()
                .source,
            SourceId::new("liminal-hq"),
            "same forge and host: the first source wins"
        );

        sources.push(source(
            "mirror",
            ForgeKind::GitHub,
            "ghe.example",
            &["liminal-hq"],
        ));
        let inf = Inference {
            sources: &sources,
            ..Inference::default()
        };
        let err = resolve(&qualified(None, "liminal-hq/spindle", 214), &inf).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("liminal-hq (github.com), personal (github.com), mirror (ghe.example)"),
            "{msg}"
        );
        assert!(msg.contains("--source"));

        let only = ["personal".to_string()];
        let inf = Inference {
            sources: &sources,
            only_sources: &only,
            ..Inference::default()
        };
        assert_eq!(
            resolve(&qualified(None, "liminal-hq/spindle", 214), &inf)
                .unwrap()
                .source,
            SourceId::new("personal")
        );
    }

    #[test]
    fn bare_repo_name_needs_a_single_owner_scope() {
        let sources = sources();
        let inf = Inference {
            sources: &sources,
            ..Inference::default()
        };
        assert!(matches!(
            resolve(&qualified(None, "spindle", 214), &inf),
            Err(SelectorError::Ambiguous { .. })
        ));
        let github = &sources[..1];
        let inf = Inference {
            sources: github,
            ..Inference::default()
        };
        let target = resolve(&qualified(None, "spindle", 214), &inf).unwrap();
        assert_eq!(target.repo, "liminal-hq/spindle");
        let everything = vec![source("all", ForgeKind::GitHub, "github.com", &[])];
        let inf = Inference {
            sources: &everything,
            ..Inference::default()
        };
        assert!(matches!(
            resolve(&qualified(None, "spindle", 214), &inf),
            Err(SelectorError::NoSourceForRepo { .. })
        ));
    }

    #[test]
    fn numbers_need_a_repository() {
        let sources = sources();
        let inf = Inference {
            sources: &sources,
            ..Inference::default()
        };
        assert_eq!(
            resolve(&Selector::Number(214), &inf),
            Err(SelectorError::NoRepository)
        );
        let flagged = Inference {
            sources: &sources,
            repo_flag: Some(RepoRef::parse("liminal-hq/spindle").unwrap()),
            ..Inference::default()
        };
        let target = resolve(&Selector::Number(214), &flagged).unwrap();
        assert_eq!(target.which, Which::Number(214));
        assert_eq!(target.repo, "liminal-hq/spindle");
    }

    #[test]
    fn the_flag_beats_the_remote_and_the_host_picks_the_source() {
        let sources = sources();
        let inf = Inference {
            sources: &sources,
            repo_flag: Some(RepoRef::parse("gitlab.work.ca/platform/flow").unwrap()),
            git_remote: Some(RepoRef::parse("liminal-hq/spindle").unwrap()),
            ..Inference::default()
        };
        let target = resolve(&Selector::Number(5), &inf).unwrap();
        assert_eq!(target.source, SourceId::new("platform"));
        let wrong_host = Inference {
            sources: &sources,
            repo_flag: Some(RepoRef::parse("ghe.example.com/a/b").unwrap()),
            ..Inference::default()
        };
        assert_eq!(
            resolve(&Selector::Number(5), &wrong_host),
            Err(SelectorError::NoSourceForHost("ghe.example.com".into()))
        );
    }

    #[test]
    fn branch_and_current_branch_use_the_inferred_repo() {
        let sources = sources();
        let inf = Inference {
            sources: &sources,
            git_remote: Some(RepoRef::parse("liminal-hq/spindle").unwrap()),
            current_branch: Some("feat/menus".into()),
            ..Inference::default()
        };
        let named = resolve(&Selector::Branch("fix/x".into()), &inf).unwrap();
        assert_eq!(named.which, Which::Branch("fix/x".into()));
        let current = resolve(&Selector::Current, &inf).unwrap();
        assert_eq!(current.which, Which::Branch("feat/menus".into()));

        let detached = Inference {
            current_branch: None,
            ..inf.clone()
        };
        assert_eq!(
            resolve(&Selector::Current, &detached),
            Err(SelectorError::NoBranch)
        );
        let nowhere = Inference {
            sources: &sources,
            current_branch: Some("x".into()),
            ..Inference::default()
        };
        assert_eq!(
            resolve(&Selector::Current, &nowhere),
            Err(SelectorError::NoRepository)
        );
    }

    #[test]
    fn unknown_source_flag_lists_the_known_ones() {
        let sources = sources();
        let only = ["typo".to_string()];
        let inf = Inference {
            sources: &sources,
            only_sources: &only,
            ..Inference::default()
        };
        let err = resolve(&Selector::Number(1), &inf).unwrap_err();
        assert!(matches!(err, SelectorError::UnknownSource { .. }));
        let none = Inference {
            sources: &[],
            only_sources: &only,
            ..Inference::default()
        };
        assert!(resolve(&Selector::Number(1), &none)
            .unwrap_err()
            .to_string()
            .contains("none"));
    }

    #[test]
    fn explicit_repo_scope_matches_listed_repos_only() {
        let mut scoped = source("one", ForgeKind::GitHub, "github.com", &[]);
        scoped.scope.repos = vec!["a/b".into()];
        let sources = [scoped];
        let inf = Inference {
            sources: &sources,
            ..Inference::default()
        };
        assert!(resolve(&qualified(None, "a/b", 1), &inf).is_ok());
        assert!(resolve(&qualified(None, "a/c", 1), &inf).is_err());
        assert_eq!(resolve(&qualified(None, "b", 1), &inf).unwrap().repo, "a/b");
    }
}
