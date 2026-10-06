//! `review-buddy auth token`: print the token review-buddy would use, for scripts.
//!
//! The token goes to stdout and nowhere else; where it came from goes to stderr. A terminal
//! gets a refusal unless `--show` says it's meant.

// Without `live` there is no network, so some of this is only reachable from tests.
#![cfg_attr(not(feature = "live"), allow(dead_code, unused_imports))]

use std::sync::Arc;

use rb_platform::auth::AuthMode;
use rb_platform::{CommandRunner, Secret, SecretStore};

use super::context::Context;
use super::error::CmdError;
use super::host::{self, Target};
use super::output;
use super::DEMO_LABEL;
use crate::config::AuthSetting;
use rb_core::ForgeKind;

type EnvLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// What resolving a token touches outside the process.
pub struct Sources {
    pub runner: Arc<dyn CommandRunner + Send + Sync>,
    pub store: Arc<dyn SecretStore>,
    pub getenv: EnvLookup,
}

fn mode_of(auth: &AuthSetting) -> AuthMode {
    match auth {
        AuthSetting::Cli => AuthMode::Cli,
        AuthSetting::Token => AuthMode::Token,
        AuthSetting::Env(var) => AuthMode::Env(var.clone()),
        AuthSetting::Command => AuthMode::Command,
    }
}

fn describe(mode: &AuthMode, kind: ForgeKind) -> String {
    match mode {
        AuthMode::Cli if kind == ForgeKind::GitLab => "glab".into(),
        AuthMode::Cli => "gh".into(),
        AuthMode::Token => "the OS keyring".into(),
        AuthMode::Env(var) => format!("env:{var}"),
        AuthMode::Command => "token_command".into(),
    }
}

fn resolve_with(target: &Target, mode: &AuthMode, from: &Sources) -> Result<Secret, String> {
    let getenv = |key: &str| (from.getenv)(key);
    let (runner, store) = (&*from.runner, &*from.store);
    match target.kind {
        ForgeKind::GitHub => {
            let mut auth = rb_github::Auth::new(&target.host, mode.clone());
            if let Some(command) = &target.token_command {
                auth = auth.with_token_command(command);
            }
            auth.resolve(runner, store, getenv)
                .map(|t| t.secret)
                .map_err(|e| e.to_string())
        }
        ForgeKind::GitLab => {
            let mut auth = rb_gitlab::Auth::new(&target.host, mode.clone());
            if let Some(command) = &target.token_command {
                auth = auth.with_token_command(command);
            }
            auth.resolve(runner, store, getenv)
                .map(|t| t.secret)
                .map_err(|e| e.to_string())
        }
    }
}

/// The token for `target` and a line saying where it came from. A source's own `auth` decides;
/// a host with no source tries the keyring, then the `gh` or `glab` sign-in.
pub fn resolve(target: &Target, from: &Sources) -> Result<(Secret, String), CmdError> {
    let modes = match (&target.source, &target.auth) {
        (Some(_), auth) => vec![auth.as_ref().map_or(AuthMode::Cli, mode_of)],
        (None, _) => vec![AuthMode::Token, AuthMode::Cli],
    };
    let mut last = String::new();
    for mode in &modes {
        match resolve_with(target, mode, from) {
            Ok(secret) => {
                let origin = format!(
                    "Token for {} from {}.",
                    target.host,
                    describe(mode, target.kind)
                );
                return Ok((secret, origin));
            }
            Err(reason) => last = reason,
        }
    }
    Err(CmdError::AuthNeeded(format!(
        "{last}\nRun review-buddy auth login --host {}, or check the source's auth setting.",
        target.host
    )))
}

fn refuse_terminal() -> CmdError {
    CmdError::usage(
        "Not printing a token to a terminal.\nPass --show if you mean it, or pipe it: review-buddy auth token | pbcopy",
    )
}

pub fn run(ctx: &Context, host: Option<&str>, show: bool) -> Result<(), CmdError> {
    let target = host::resolve(ctx, host)?;
    if ctx.is_demo() {
        return Err(CmdError::usage(format!(
            "Demo mode has no real tokens to print {DEMO_LABEL}\nDrop --demo to print the token for {}.",
            target.host
        )));
    }
    if ctx.out.tty && !show {
        return Err(refuse_terminal());
    }
    #[cfg(not(feature = "live"))]
    {
        let _ = resolve;
        Err(host::no_network())
    }
    #[cfg(feature = "live")]
    {
        let deps = crate::providers::Deps::system();
        let (secret, origin) = resolve(
            &target,
            &Sources {
                runner: deps.runner,
                store: deps.store,
                getenv: deps.getenv,
            },
        )?;
        eprintln!("{origin}");
        output::print(&format!("{}\n", secret.expose()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_platform::{CommandOutput, MemorySecretStore, PlatformError};

    struct NoRunner;

    impl CommandRunner for NoRunner {
        fn run(
            &self,
            program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            Err(PlatformError::CommandFailed {
                program: program.into(),
            })
        }
    }

    fn from(store: MemorySecretStore, env: &'static [(&'static str, &'static str)]) -> Sources {
        Sources {
            runner: Arc::new(NoRunner),
            store: Arc::new(store),
            getenv: Arc::new(move |k| {
                env.iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }),
        }
    }

    fn target(auth: Option<AuthSetting>, source: bool) -> Target {
        Target {
            host: "ghe.test".into(),
            kind: ForgeKind::GitHub,
            api_url: None,
            source: source.then(|| "work".to_string()),
            auth,
            token_command: None,
        }
    }

    #[test]
    fn a_hostless_source_tries_the_keyring_first_and_names_it() {
        let store = MemorySecretStore::new();
        store.set("ghe.test", &Secret::new("ghp_k")).unwrap();
        let (secret, origin) = resolve(&target(None, false), &from(store, &[])).unwrap();
        assert_eq!(secret.expose(), "ghp_k");
        assert_eq!(origin, "Token for ghe.test from the OS keyring.");
    }

    #[test]
    fn a_source_uses_its_own_auth_setting() {
        let t = target(Some(AuthSetting::Env("GH_T".into())), true);
        let (secret, origin) =
            resolve(&t, &from(MemorySecretStore::new(), &[("GH_T", "ghp_e")])).unwrap();
        assert_eq!(secret.expose(), "ghp_e");
        assert!(origin.ends_with("from env:GH_T."));
    }

    #[test]
    fn no_token_exits_4_with_the_fix() {
        let err = resolve(&target(None, false), &from(MemorySecretStore::new(), &[])).unwrap_err();
        assert_eq!(err.exit().code(), 4);
        assert!(err.to_string().contains("auth login --host ghe.test"));
    }

    #[test]
    fn terminal_refusal_is_a_calm_usage_error() {
        let err = refuse_terminal();
        assert_eq!(err.exit().code(), 2);
        assert!(err.to_string().contains("--show"));
    }
}
