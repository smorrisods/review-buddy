//! Which host a token command means, and a token test that works for GitHub and GitLab alike.

// Without `live` there is no network, so some of this is only reachable from tests.
#![cfg_attr(not(feature = "live"), allow(dead_code, unused_imports))]

use rb_core::ForgeKind;

use super::context::Context;
use super::error::CmdError;
use crate::config::{AuthSetting, Kind, SourceConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub kind: ForgeKind,
    pub api_url: Option<String>,
    /// The configured source this came from, when there is one.
    pub source: Option<String>,
    pub auth: Option<AuthSetting>,
    pub token_command: Option<String>,
}

impl Target {
    pub fn from_source(cfg: &SourceConfig) -> Self {
        Self {
            host: cfg.host.clone(),
            kind: forge_kind(cfg.kind),
            api_url: cfg.api_url.clone(),
            source: Some(cfg.name.clone()),
            auth: cfg.auth.clone(),
            token_command: cfg.token_command.clone(),
        }
    }

    fn bare(host: &str) -> Self {
        Self {
            host: host.to_string(),
            kind: infer_kind(host),
            api_url: None,
            source: None,
            auth: None,
            token_command: None,
        }
    }
}

pub fn forge_kind(kind: Kind) -> ForgeKind {
    match kind {
        Kind::Github => ForgeKind::GitHub,
        Kind::Gitlab => ForgeKind::GitLab,
    }
}

/// GitLab when the host name says so, GitHub otherwise.
pub fn infer_kind(host: &str) -> ForgeKind {
    if host.to_ascii_lowercase().contains("gitlab") {
        ForgeKind::GitLab
    } else {
        ForgeKind::GitHub
    }
}

#[cfg_attr(feature = "live", allow(dead_code))]
pub fn no_network() -> CmdError {
    CmdError::usage("This build has no network support.\nTry --demo.")
}

/// Picks the host from `--host`, `--source`, or the only configured source.
pub fn resolve(ctx: &Context, host: Option<&str>) -> Result<Target, CmdError> {
    let configured = ctx.configured()?;
    let names = |list: &[&SourceConfig]| {
        let names: Vec<_> = list.iter().map(|s| s.name.as_str()).collect();
        if names.is_empty() {
            "none".to_string()
        } else {
            names.join(", ")
        }
    };
    let all: Vec<&SourceConfig> = configured.iter().collect();
    if let Some(wanted) = ctx.args.sources.first() {
        let found = all
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| {
                CmdError::usage(format!(
                    "There's no source called {wanted}.\nConfigured sources: {}.",
                    names(&all)
                ))
            })?;
        if host.is_some_and(|h| !h.eq_ignore_ascii_case(&found.host)) {
            return Err(CmdError::usage(format!(
                "Source {} is on {}, not {}.\nLeave out --host, or pick the matching source.",
                found.name,
                found.host,
                host.unwrap_or_default()
            )));
        }
        return Ok(Target::from_source(found));
    }
    if let Some(host) = host {
        return Ok(all
            .iter()
            .find(|s| s.host.eq_ignore_ascii_case(host))
            .map_or_else(|| Target::bare(host), |s| Target::from_source(s)));
    }
    let enabled: Vec<&SourceConfig> = all.iter().copied().filter(|s| s.enabled).collect();
    match enabled.as_slice() {
        [only] => Ok(Target::from_source(only)),
        [] => Err(CmdError::usage(
            "No host given and no sources are configured.\nPass --host <host>, or add a source with review-buddy source add.",
        )),
        many => Err(CmdError::usage(format!(
            "More than one source is configured ({}).\nPass --host <host> or --source <name>.",
            names(many)
        ))),
    }
}

/// What a token test learned, whichever forge answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tested {
    pub login: String,
    pub scopes: Vec<String>,
}

impl Tested {
    pub fn scope_text(&self) -> String {
        if self.scopes.is_empty() {
            "scopes not reported".to_string()
        } else {
            format!("scopes {}", self.scopes.join(", "))
        }
    }
}

#[cfg(feature = "live")]
pub async fn test_token(
    target: &Target,
    secret: rb_platform::Secret,
) -> Result<Tested, rb_core::Error> {
    let api = target.api_url.as_deref();
    match target.kind {
        ForgeKind::GitHub => {
            let report = rb_github::GithubClient::new(&target.host, api, secret)?
                .test_token()
                .await?;
            Ok(Tested {
                login: report.login,
                scopes: report.scopes,
            })
        }
        ForgeKind::GitLab => {
            let report = rb_gitlab::GitlabClient::new(&target.host, api, secret)?
                .test_token()
                .await?;
            Ok(Tested {
                login: report.login,
                scopes: report.scopes,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::GlobalArgs;
    use crate::cmd::context::Terminal;
    use rb_paths::MapEnv;

    fn ctx(config: &str, sources: &[&str]) -> Context {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        std::fs::write(&file, config).unwrap();
        let args = GlobalArgs {
            config: Some(file),
            sources: sources.iter().map(ToString::to_string).collect(),
            ..GlobalArgs::default()
        };
        let terminal = Terminal {
            stdout_tty: false,
            stdin_tty: false,
            width: None,
        };
        let ctx = Context::from_env(args, terminal, Box::new(MapEnv::new(dir.path()))).unwrap();
        std::mem::forget(dir);
        ctx
    }

    const TWO: &str = "[[source]]\nname = \"work\"\nkind = \"github\"\nhost = \"ghe.test\"\n[[source]]\nname = \"lab\"\nkind = \"gitlab\"\nhost = \"gl.test\"\napi_url = \"https://gl.test/api/v4\"\n";

    #[test]
    fn host_flag_matches_a_source_or_falls_back_to_a_guess() {
        let c = ctx(TWO, &[]);
        let t = resolve(&c, Some("gl.test")).unwrap();
        assert_eq!(
            (t.kind, t.source.as_deref(), t.api_url.is_some()),
            (ForgeKind::GitLab, Some("lab"), true)
        );
        let t = resolve(&c, Some("gitlab.example.org")).unwrap();
        assert_eq!((t.kind, t.source), (ForgeKind::GitLab, None));
        let t = resolve(&c, Some("github.com")).unwrap();
        assert_eq!(t.kind, ForgeKind::GitHub);
    }

    #[test]
    fn source_flag_picks_by_name_and_must_agree_with_host() {
        let t = resolve(&ctx(TWO, &["WORK"]), None).unwrap();
        assert_eq!(t.host, "ghe.test");
        let err = resolve(&ctx(TWO, &["work"]), Some("gl.test")).unwrap_err();
        assert_eq!(err.exit().code(), 2);
        let err = resolve(&ctx(TWO, &["nope"]), None).unwrap_err();
        assert!(err.to_string().contains("Configured sources: work, lab"));
    }

    #[test]
    fn no_host_needs_exactly_one_source() {
        assert_eq!(
            resolve(
                &ctx(&TWO[..TWO.find("[[source]]\nname = \"lab").unwrap()], &[]),
                None
            )
            .unwrap()
            .host,
            "ghe.test"
        );
        let err = resolve(&ctx(TWO, &[]), None).unwrap_err();
        assert!(err.to_string().contains("--host"));
        let err = resolve(&ctx("", &[]), None).unwrap_err();
        assert!(err.to_string().contains("source add"));
    }
}
