//! `review-buddy auth status`, and the sign-in check `doctor` shares.
//!
//! The check resolves a token the way the interface does, asks the forge who it belongs to, and
//! reports what it found. The token itself is never kept in a report, printed or logged.

// Without `live` there is no network check, so the reporting types are only used by demo mode.
#![cfg_attr(not(feature = "live"), allow(dead_code, unused_imports))]

use rb_core::ForgeKind;
use rb_github::TokenReport;
use rb_platform::auth::AuthMode;
use rb_theme::Role;
use serde_json::{json, Value};

use super::context::Context;
use super::error::CmdError;
use super::output::{self, json, Painter};
use super::DEMO_LABEL;
use crate::config::{Kind, SourceConfig};
#[cfg(feature = "live")]
use crate::providers::{Factory, ProviderError};
#[cfg(feature = "live")]
use rb_core::{Error, SourceId};
#[cfg(feature = "live")]
use rb_github::{AuthError, TokenOrigin};

pub const FIELDS: &[&str] = &[
    "source", "kind", "host", "method", "state", "user", "scopes", "expires", "message", "fix",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    Gh,
    Glab,
    Keyring,
    Env(String),
    Command,
    Demo,
}

impl Method {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Gh => "gh",
            Self::Glab => "glab",
            Self::Keyring => "keyring",
            Self::Env(_) => "env",
            Self::Command => "command",
            Self::Demo => "demo",
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Env(var) => format!("env:{var}"),
            other => other.name().to_string(),
        }
    }

    #[cfg(feature = "live")]
    fn from_mode(mode: &AuthMode, kind: Kind) -> Self {
        match mode {
            AuthMode::Cli if kind == Kind::Gitlab => Self::Glab,
            AuthMode::Cli => Self::Gh,
            AuthMode::Token => Self::Keyring,
            AuthMode::Env(var) => Self::Env(var.clone()),
            AuthMode::Command => Self::Command,
        }
    }

    #[cfg(feature = "live")]
    fn from_origin(origin: &TokenOrigin) -> Self {
        match origin {
            TokenOrigin::GhCli => Self::Gh,
            TokenOrigin::Keyring => Self::Keyring,
            TokenOrigin::Env(var) => Self::Env(var.clone()),
            TokenOrigin::Command => Self::Command,
        }
    }

    #[cfg(feature = "live")]
    fn from_gitlab_origin(origin: &rb_gitlab::TokenOrigin) -> Self {
        use rb_gitlab::TokenOrigin as O;
        match origin {
            O::GlabCli => Self::Glab,
            O::Keyring => Self::Keyring,
            O::Env(var) => Self::Env(var.clone()),
            O::Command => Self::Command,
        }
    }

    /// What to do when this method can't produce a working token for `host`.
    #[cfg(feature = "live")]
    fn fix(&self, host: &str) -> String {
        match self {
            Self::Gh => format!(
                "run gh auth login --hostname {host}, or review-buddy auth login --host {host}"
            ),
            Self::Glab => format!(
                "run glab auth login --hostname {host}, or review-buddy auth login --host {host}"
            ),
            Self::Keyring => format!("run review-buddy auth login --host {host}"),
            Self::Env(var) => format!("set {var} to a token, or change auth for this source"),
            Self::Command => "check that token_command prints a token".to_string(),
            Self::Demo => String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum State {
    SignedIn {
        user: String,
        scopes: Vec<String>,
        expires: Option<String>,
    },
    Failed {
        reason: String,
        fix: String,
    },
}

#[derive(Debug, Clone)]
pub struct SourceAuth {
    pub name: String,
    pub kind: ForgeKind,
    pub host: String,
    pub api_url: Option<String>,
    pub method: Option<Method>,
    pub state: State,
    /// What the forge reported about the token; `doctor` shows the rate limits from it.
    pub report: Option<TokenReport>,
}

impl SourceAuth {
    pub fn is_demo(&self) -> bool {
        self.method == Some(Method::Demo)
    }

    pub fn label(&self) -> String {
        if self.name == self.host {
            self.host.clone()
        } else {
            format!("{} ({})", self.host, self.name)
        }
    }

    fn to_json(&self) -> Value {
        let (state, user, scopes, expires, message, fix) = match &self.state {
            State::SignedIn {
                user,
                scopes,
                expires,
            } => (
                "signedIn",
                json!(user),
                json!(scopes),
                json!(expires),
                Value::Null,
                Value::Null,
            ),
            State::Failed { reason, fix } => (
                "failed",
                Value::Null,
                json!([]),
                Value::Null,
                json!(reason),
                json!(fix),
            ),
        };
        json!({
            "source": self.name,
            "kind": kind_name(self.kind),
            "host": self.host,
            "method": self.method.as_ref().map(Method::name),
            "state": state,
            "user": user,
            "scopes": scopes,
            "expires": expires,
            "message": message,
            "fix": fix,
        })
    }
}

pub fn kind_name(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::GitHub => "github",
        ForgeKind::GitLab => "gitlab",
    }
}

fn base(cfg: &SourceConfig, method: Option<Method>, state: State) -> SourceAuth {
    SourceAuth {
        name: cfg.name.clone(),
        kind: match cfg.kind {
            Kind::Github => ForgeKind::GitHub,
            Kind::Gitlab => ForgeKind::GitLab,
        },
        host: cfg.host.clone(),
        api_url: cfg.api_url.clone(),
        method,
        state,
        report: None,
    }
}

fn failed(cfg: &SourceConfig, method: Method, reason: String, fix: String) -> SourceAuth {
    base(cfg, Some(method), State::Failed { reason, fix })
}

#[cfg(feature = "live")]
/// Signs in to one configured source and reports how it went. Token resolution is the
/// factory's, so this reports exactly what the interface would do. Network access happens only
/// for GitHub sources; GitLab sources report that they are checked in a later release.
pub async fn check_source(cfg: &SourceConfig, factory: &Factory) -> SourceAuth {
    let id = SourceId::new(&cfg.name);
    let configured = factory
        .auth_mode(&id)
        .as_ref()
        .map_or(Method::Gh, |mode| Method::from_mode(mode, cfg.kind));
    let host = &cfg.host;
    if cfg.kind == Kind::Gitlab {
        return check_gitlab(cfg, factory, &id, configured).await;
    }
    let (client, origin) = match factory.github_client(&id) {
        Ok(found) => found,
        Err(ProviderError::Auth(AuthError::NoToken { .. })) => {
            return failed(
                cfg,
                configured.clone(),
                "not signed in".into(),
                configured.fix(host),
            )
        }
        Err(ProviderError::Auth(AuthError::Unavailable { reason, .. })) => {
            return failed(
                cfg,
                configured.clone(),
                format!("couldn't read a token: {reason}"),
                configured.fix(host),
            )
        }
        Err(other) => {
            return failed(
                cfg,
                configured,
                other.to_string(),
                "fix `api_url` in the source".into(),
            )
        }
    };
    let method = Method::from_origin(&origin);
    match client.test_token().await {
        Ok(report) => {
            let state = State::SignedIn {
                user: report.login.clone(),
                scopes: report.scopes.clone(),
                expires: report.expires.clone(),
            };
            let mut out = base(cfg, Some(method), state);
            out.report = Some(report);
            out
        }
        Err(err) => {
            let (reason, fix) = explain(&err, &method, host);
            failed(cfg, method, reason, fix)
        }
    }
}

#[cfg(feature = "live")]
fn explain(err: &Error, method: &Method, host: &str) -> (String, String) {
    match err {
        Error::Unauthorized { .. } => ("token rejected".to_string(), method.fix(host)),
        Error::Network { .. } => (
            err.to_string(),
            "check your connection and try again".to_string(),
        ),
        Error::Forbidden { .. } => (
            err.to_string(),
            "check the token's scopes and organisation SSO access".to_string(),
        ),
        _ => (err.to_string(), "try again in a moment".to_string()),
    }
}

#[cfg(feature = "live")]
async fn check_gitlab(
    cfg: &SourceConfig,
    factory: &Factory,
    id: &SourceId,
    configured: Method,
) -> SourceAuth {
    let host = &cfg.host;
    let (client, origin) = match factory.gitlab_client(id) {
        Ok(found) => found,
        Err(ProviderError::GitlabAuth(rb_gitlab::AuthError::NoToken { .. })) => {
            return failed(
                cfg,
                configured.clone(),
                "not signed in".into(),
                configured.fix(host),
            )
        }
        Err(ProviderError::GitlabAuth(rb_gitlab::AuthError::Unavailable { reason, .. })) => {
            return failed(
                cfg,
                configured.clone(),
                format!("couldn't read a token: {reason}"),
                configured.fix(host),
            )
        }
        Err(other) => {
            return failed(
                cfg,
                configured,
                other.to_string(),
                "fix `api_url` in the source".into(),
            )
        }
    };
    let method = Method::from_gitlab_origin(&origin);
    match client.test_token().await {
        Ok(report) => {
            let state = State::SignedIn {
                user: report.login.clone(),
                scopes: report.scopes.clone(),
                expires: report.expires.clone(),
            };
            let mut out = base(cfg, Some(method), state);
            out.report = report.rate.map(|rate| TokenReport {
                login: report.login,
                name: report.name,
                scopes: report.scopes,
                core: rb_github::RateLimit {
                    limit: rate.limit,
                    remaining: rate.remaining,
                    reset: rate.reset,
                },
                graphql: None,
                sso_hint: None,
                expires: report.expires,
            });
            out
        }
        Err(err) => {
            let (reason, fix) = explain(&err, &method, host);
            failed(cfg, method, reason, fix)
        }
    }
}

async fn check_demo(ctx: &Context) -> Result<Vec<SourceAuth>, CmdError> {
    let mut out = Vec::new();
    for source in ctx.sources()? {
        let user = ctx.provider_for(&source)?.whoami().await?;
        out.push(SourceAuth {
            name: source.label.clone(),
            kind: source.kind,
            host: source.host.clone(),
            api_url: None,
            method: Some(Method::Demo),
            state: State::SignedIn {
                user: user.login,
                scopes: Vec::new(),
                expires: None,
            },
            report: None,
        });
    }
    Ok(out)
}

#[cfg(feature = "live")]
/// The sources an auth check covers: every enabled one, or the ones `--source` names.
fn selected(ctx: &Context) -> Result<Vec<&SourceConfig>, CmdError> {
    let all = ctx.configured()?;
    if ctx.args.sources.is_empty() {
        return Ok(all.iter().filter(|s| s.enabled).collect());
    }
    let named = ctx.sources()?;
    Ok(all
        .iter()
        .filter(|s| named.iter().any(|n| n.id.as_str() == s.name))
        .collect())
}

/// Checks every selected source. Under `--demo` this never touches the network or the keyring.
pub fn gather(ctx: &Context) -> Result<Vec<SourceAuth>, CmdError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if ctx.is_demo() {
        return runtime.block_on(check_demo(ctx));
    }
    #[cfg(feature = "live")]
    {
        let configs = selected(ctx)?;
        let factory = ctx.factory();
        Ok(runtime.block_on(async {
            let mut out = Vec::new();
            for cfg in configs {
                out.push(check_source(cfg, &factory).await);
            }
            out
        }))
    }
    #[cfg(not(feature = "live"))]
    {
        Err(CmdError::usage(
            "This build has no network support.\nTry --demo.",
        ))
    }
}

/// One line per source: `✓ github.com  signed in as smorris via gh · scopes repo, read:org`.
pub fn render_lines(auths: &[SourceAuth], painter: &Painter) -> String {
    let mut out = String::new();
    for a in auths {
        let (mark, role, text) = match &a.state {
            State::SignedIn {
                user,
                scopes,
                expires,
            } => {
                let method = a.method.as_ref().map_or_else(String::new, Method::label);
                let mut text = format!("signed in as {user} via {method}");
                if a.is_demo() {
                    text.push_str(&format!(" {DEMO_LABEL}"));
                } else if scopes.is_empty() {
                    text.push_str(" · scopes not reported");
                } else {
                    text.push_str(&format!(" · scopes {}", scopes.join(", ")));
                }
                if let Some(date) = expires {
                    text.push_str(&format!(" · expires {date}"));
                }
                ("✓", Some(Role::Success), text)
            }
            State::Failed { reason, fix } => ("✕", Some(Role::Danger), format!("{reason} · {fix}")),
        };
        let mark = match role {
            Some(role) => painter.paint(role, mark),
            None => mark.to_string(),
        };
        out.push_str(&format!("{mark} {}  {text}\n", a.label()));
    }
    out
}

/// `Err` with exit 4 and each fix when any source couldn't sign in.
pub fn require_signed_in(auths: &[SourceAuth]) -> Result<(), CmdError> {
    let problems: Vec<String> = auths
        .iter()
        .filter_map(|a| match &a.state {
            State::Failed { fix, .. } => Some(format!("  {}: {fix}", a.label())),
            _ => None,
        })
        .collect();
    if problems.is_empty() {
        return Ok(());
    }
    let noun = if problems.len() == 1 {
        "source can't"
    } else {
        "sources can't"
    };
    Err(CmdError::AuthNeeded(format!(
        "{} {noun} sign in. To fix:\n{}",
        problems.len(),
        problems.join("\n")
    )))
}

pub fn auths_json(auths: &[SourceAuth]) -> Value {
    Value::Array(auths.iter().map(SourceAuth::to_json).collect())
}

pub fn status(ctx: &Context) -> Result<(), CmdError> {
    let auths = gather(ctx)?;
    if let Some(text) = json::render(&ctx.args, FIELDS, &auths_json(&auths), ctx.out.tty) {
        output::print(&text?)?;
    } else if auths.is_empty() {
        output::print("No sources are configured yet.\nRun review-buddy --setup or review-buddy source add, or try --demo.\n")?;
    } else {
        output::print(&render_lines(&auths, &ctx.out.painter))?;
    }
    require_signed_in(&auths)
}

#[cfg(all(test, feature = "live"))]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::config::Config;
    use crate::providers::Deps;
    use rb_platform::{CommandOutput, MemorySecretStore, PlatformError, Secret};
    use rb_platform::{CommandRunner, SecretStore};
    use serde_json::json;
    use wiremock::matchers::{header, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SECRET: &str = "ghp_topsecretvalue123";

    struct FakeRunner {
        stdout: Option<&'static str>,
    }

    impl CommandRunner for FakeRunner {
        fn run(
            &self,
            program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            match self.stdout {
                Some(out) => Ok(CommandOutput {
                    success: true,
                    stdout: out.into(),
                    stderr: String::new(),
                }),
                None => Err(PlatformError::CommandFailed {
                    program: program.into(),
                }),
            }
        }
    }

    fn cfg(server: &MockServer, auth: &str) -> SourceConfig {
        let text = format!(
            "name = \"work\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{}\"\nauth = \"{auth}\"\ntoken_command = \"pass gh\"\n",
            server.uri()
        );
        toml::from_str(&text).unwrap()
    }

    async fn mount_ok(server: &MockServer) {
        Mock::given(path("/user"))
            .and(header("authorization", format!("Bearer {SECRET}").as_str()))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-oauth-scopes", "repo, read:org")
                    .insert_header(
                        "github-authentication-token-expiration",
                        "2027-01-12 10:00:00 UTC",
                    )
                    .set_body_json(json!({"login": "smorris", "name": null})),
            )
            .mount(server)
            .await;
        Mock::given(path("/rate_limit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "resources": {
                    "core": {"limit": 5000, "remaining": 4989, "reset": 1700000100},
                    "graphql": {"limit": 5000, "remaining": 4000, "reset": 1700000200}
                }
            })))
            .mount(server)
            .await;
    }

    async fn run(
        cfg: &SourceConfig,
        gh: Option<&'static str>,
        store: &Arc<MemorySecretStore>,
        env: Option<(&'static str, &'static str)>,
    ) -> SourceAuth {
        let deps = Deps {
            runner: Arc::new(FakeRunner { stdout: gh }),
            store: store.clone(),
            getenv: Arc::new(move |k| {
                env.filter(|(name, _)| *name == k)
                    .map(|(_, v)| v.to_string())
            }),
        };
        let config = Config {
            sources: vec![cfg.clone()],
            ..Config::default()
        };
        check_source(cfg, &Factory::from_config(&config, deps)).await
    }

    fn text(auths: &[SourceAuth]) -> String {
        render_lines(auths, &Painter::plain())
    }

    #[tokio::test]
    async fn gh_sign_in_reports_user_method_scopes_and_expiry() {
        let server = MockServer::start().await;
        mount_ok(&server).await;
        let a = run(
            &cfg(&server, "cli"),
            Some("ghp_topsecretvalue123\n"),
            &Arc::new(MemorySecretStore::new()),
            None,
        )
        .await;
        assert_eq!(
            text(std::slice::from_ref(&a)),
            "✓ ghe.test (work)  signed in as smorris via gh · scopes repo, read:org · expires 2027-01-12\n"
        );
        assert!(a.report.is_some());
        assert!(require_signed_in(&[a]).is_ok());
    }

    #[tokio::test]
    async fn keyring_env_and_command_methods_are_named() {
        let server = MockServer::start().await;
        mount_ok(&server).await;
        let store = Arc::new(MemorySecretStore::new());
        store.set("ghe.test", &Secret::new(SECRET)).unwrap();
        let a = run(&cfg(&server, "token"), None, &store, None).await;
        assert_eq!(a.method, Some(Method::Keyring));

        let env = Some(("GH_T", SECRET));
        let a = run(
            &cfg(&server, "env:GH_T"),
            None,
            &Arc::new(MemorySecretStore::new()),
            env,
        )
        .await;
        assert!(text(&[a]).contains("via env:GH_T"));

        let a = run(
            &cfg(&server, "command"),
            Some("ghp_topsecretvalue123"),
            &Arc::new(MemorySecretStore::new()),
            None,
        )
        .await;
        assert_eq!(a.method, Some(Method::Command));
    }

    #[tokio::test]
    async fn missing_token_says_how_to_fix_it_and_exits_4() {
        let server = MockServer::start().await;
        let a = run(
            &cfg(&server, "cli"),
            None,
            &Arc::new(MemorySecretStore::new()),
            None,
        )
        .await;
        let line = text(std::slice::from_ref(&a));
        assert!(line.starts_with("✕ ghe.test (work)  not signed in · run gh auth login"));
        let err = require_signed_in(&[a]).unwrap_err();
        assert_eq!(err.exit().code(), 4);
        assert!(err
            .to_string()
            .contains("review-buddy auth login --host ghe.test"));
    }

    #[tokio::test]
    async fn a_rejected_token_is_a_failure_with_the_method_fix() {
        let server = MockServer::start().await;
        Mock::given(path("/user"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let store = Arc::new(MemorySecretStore::new());
        store.set("ghe.test", &Secret::new(SECRET)).unwrap();
        let a = run(&cfg(&server, "token"), None, &store, None).await;
        assert!(text(&[a]).contains("token rejected · run review-buddy auth login"));
    }

    fn gitlab_cfg(server: &MockServer, auth: &str) -> SourceConfig {
        let mut c = cfg(server, auth);
        c.kind = Kind::Gitlab;
        c.host = "gitlab.test".into();
        c
    }

    async fn mount_gitlab(server: &MockServer, bearer: bool) {
        let (name, value) = if bearer {
            ("authorization", "Bearer glpat-topsecretvalue123")
        } else {
            ("private-token", "glpat-topsecretvalue123")
        };
        Mock::given(path("/user"))
            .and(header(name, value))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ratelimit-limit", "2000")
                    .insert_header("ratelimit-remaining", "1990")
                    .insert_header("ratelimit-reset", "1700000100")
                    .set_body_json(json!({"username": "smorris", "name": null})),
            )
            .mount(server)
            .await;
        Mock::given(path("/personal_access_tokens/self"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"scopes": ["api", "read_user"], "expires_at": "2027-01-12"}),
                ),
            )
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn gitlab_via_glab_reports_user_scopes_and_expiry() {
        let server = MockServer::start().await;
        mount_gitlab(&server, true).await;
        let a = run(
            &gitlab_cfg(&server, "cli"),
            Some("glpat-topsecretvalue123\n"),
            &Arc::new(MemorySecretStore::new()),
            None,
        )
        .await;
        assert_eq!(
            text(std::slice::from_ref(&a)),
            "✓ gitlab.test (work)  signed in as smorris via glab · scopes api, read_user · expires 2027-01-12\n"
        );
        assert_eq!(a.report.unwrap().core.remaining, 1990);
    }

    #[tokio::test]
    async fn gitlab_keyring_token_uses_private_token_header() {
        let server = MockServer::start().await;
        mount_gitlab(&server, false).await;
        let store = Arc::new(MemorySecretStore::new());
        store
            .set("gitlab.test", &Secret::new("glpat-topsecretvalue123"))
            .unwrap();
        let a = run(&gitlab_cfg(&server, "token"), None, &store, None).await;
        assert_eq!(a.method, Some(Method::Keyring));
        assert!(matches!(a.state, State::SignedIn { .. }));
    }

    #[tokio::test]
    async fn gitlab_without_a_token_points_at_glab() {
        let server = MockServer::start().await;
        let a = run(
            &gitlab_cfg(&server, "cli"),
            None,
            &Arc::new(MemorySecretStore::new()),
            None,
        )
        .await;
        assert!(text(std::slice::from_ref(&a))
            .contains("not signed in · run glab auth login --hostname gitlab.test"));
        assert_eq!(require_signed_in(&[a]).unwrap_err().exit().code(), 4);
    }

    #[tokio::test]
    async fn no_output_ever_contains_the_token() {
        let server = MockServer::start().await;
        mount_ok(&server).await;
        let store = Arc::new(MemorySecretStore::new());
        store.set("ghe.test", &Secret::new(SECRET)).unwrap();
        let mut auths = vec![
            run(
                &cfg(&server, "cli"),
                Some("ghp_topsecretvalue123"),
                &store,
                None,
            )
            .await,
            run(&cfg(&server, "token"), None, &store, None).await,
            run(
                &cfg(&server, "command"),
                Some("ghp_topsecretvalue123"),
                &store,
                None,
            )
            .await,
        ];
        let bad = MockServer::start().await;
        auths.push(run(&cfg(&bad, "token"), None, &store, None).await);
        let everything = format!(
            "{}{}{auths:?}{}",
            text(&auths),
            auths_json(&auths),
            require_signed_in(&auths).unwrap_err()
        );
        assert!(!everything.contains("topsecretvalue"), "{everything}");
    }

    #[test]
    fn json_has_every_documented_field() {
        let auth = SourceAuth {
            name: "work".into(),
            kind: ForgeKind::GitHub,
            host: "github.com".into(),
            api_url: None,
            method: Some(Method::Gh),
            state: State::SignedIn {
                user: "me".into(),
                scopes: vec!["repo".into()],
                expires: None,
            },
            report: None,
        };
        let value = auths_json(&[auth]);
        for field in FIELDS {
            assert!(value[0].get(field).is_some(), "{field}");
        }
        assert_eq!(value[0]["state"], "signedIn");
    }
}
