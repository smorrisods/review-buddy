use rb_core::Error;
use rb_platform::auth::{token_from_cli, token_from_command, token_from_env, AuthMode, CliTool};
use rb_platform::{CommandRunner, Os, Secret, SecretStore};

/// Where a resolved token came from. Never includes the token itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenOrigin {
    GhCli,
    Keyring,
    Env(String),
    Command,
}

#[derive(Debug, Clone)]
pub struct ResolvedToken {
    pub secret: Secret,
    pub origin: TokenOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("no token found for {host}. Run `gh auth login`, or store one with `review-buddy auth login --host {host}`")]
    NoToken { host: String },
    #[error("couldn't read the token for {host}: {reason}")]
    Unavailable { host: String, reason: String },
}

impl From<AuthError> for Error {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::NoToken { host } => Error::Unauthorized { host },
            AuthError::Unavailable { host, reason } => Error::Forbidden { host, reason },
        }
    }
}

/// How to find the token for one GitHub host.
#[derive(Debug, Clone)]
pub struct Auth {
    host: String,
    mode: AuthMode,
    token_command: Option<String>,
}

impl Auth {
    pub fn new(host: impl Into<String>, mode: AuthMode) -> Self {
        Self {
            host: host.into(),
            mode,
            token_command: None,
        }
    }

    pub fn with_token_command(mut self, command: impl Into<String>) -> Self {
        self.token_command = Some(command.into());
        self
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// `cli` tries `gh auth token` first and falls back to the keyring.
    pub fn resolve(
        &self,
        runner: &dyn CommandRunner,
        store: &dyn SecretStore,
        getenv: impl Fn(&str) -> Option<String>,
    ) -> Result<ResolvedToken, AuthError> {
        let host = &self.host;
        let from_store = || match store.get(host) {
            Ok(Some(secret)) => Ok(ResolvedToken {
                secret,
                origin: TokenOrigin::Keyring,
            }),
            Ok(None) => Err(AuthError::NoToken { host: host.clone() }),
            Err(e) => Err(self.unavailable(e)),
        };
        match &self.mode {
            AuthMode::Cli => match token_from_cli(CliTool::Gh, host, runner) {
                Ok(secret) => Ok(ResolvedToken {
                    secret,
                    origin: TokenOrigin::GhCli,
                }),
                Err(_) => from_store(),
            },
            AuthMode::Token => from_store(),
            AuthMode::Env(var) => token_from_env(var, getenv)
                .map(|secret| ResolvedToken {
                    secret,
                    origin: TokenOrigin::Env(var.clone()),
                })
                .map_err(|e| self.unavailable(e)),
            AuthMode::Command => {
                let command = self.token_command.as_deref().unwrap_or_default();
                token_from_command(command, Os::current(), runner)
                    .map(|secret| ResolvedToken {
                        secret,
                        origin: TokenOrigin::Command,
                    })
                    .map_err(|e| self.unavailable(e))
            }
        }
    }

    fn unavailable(&self, e: impl std::fmt::Display) -> AuthError {
        AuthError::Unavailable {
            host: self.host.clone(),
            reason: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_platform::{CommandOutput, MemorySecretStore, PlatformError};
    use std::cell::RefCell;

    struct Runner {
        out: Option<CommandOutput>,
        calls: RefCell<Vec<(String, Vec<String>)>>,
    }

    impl Runner {
        fn ok(stdout: &str) -> Self {
            Self::with(Some(CommandOutput {
                success: true,
                stdout: stdout.into(),
                stderr: String::new(),
            }))
        }
        fn with(out: Option<CommandOutput>) -> Self {
            Self {
                out,
                calls: RefCell::default(),
            }
        }
    }

    impl CommandRunner for Runner {
        fn run(
            &self,
            program: &str,
            args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            self.calls
                .borrow_mut()
                .push((program.into(), args.iter().map(|a| a.to_string()).collect()));
            self.out
                .clone()
                .ok_or_else(|| PlatformError::CommandFailed {
                    program: program.into(),
                })
        }
    }

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn cli_uses_gh_for_the_host() {
        let r = Runner::ok("gho_abc\n");
        let store = MemorySecretStore::new();
        let auth = Auth::new("ghe.example.com", AuthMode::Cli);
        let t = auth.resolve(&r, &store, none).unwrap();
        assert_eq!(t.secret.expose(), "gho_abc");
        assert_eq!(t.origin, TokenOrigin::GhCli);
        assert_eq!(
            r.calls.borrow()[0].1,
            ["auth", "token", "--hostname", "ghe.example.com"]
        );
    }

    #[test]
    fn cli_falls_back_to_keyring() {
        let store = MemorySecretStore::new();
        store.set("github.com", &Secret::new("ghp_kr")).unwrap();
        let auth = Auth::new("github.com", AuthMode::Cli);
        let t = auth.resolve(&Runner::with(None), &store, none).unwrap();
        assert_eq!(t.secret.expose(), "ghp_kr");
        assert_eq!(t.origin, TokenOrigin::Keyring);
    }

    #[test]
    fn cli_with_nothing_asks_to_sign_in() {
        let auth = Auth::new("github.com", AuthMode::Cli);
        let e = auth
            .resolve(&Runner::with(None), &MemorySecretStore::new(), none)
            .unwrap_err();
        assert_eq!(
            e,
            AuthError::NoToken {
                host: "github.com".into()
            }
        );
        assert!(e.to_string().contains("gh auth login"));
        assert!(matches!(Error::from(e), Error::Unauthorized { .. }));
    }

    #[test]
    fn token_mode_skips_gh() {
        let r = Runner::ok("gho_no");
        let store = MemorySecretStore::new();
        store.set("github.com", &Secret::new("ghp_kr")).unwrap();
        let t = Auth::new("github.com", AuthMode::Token)
            .resolve(&r, &store, none)
            .unwrap();
        assert_eq!(t.secret.expose(), "ghp_kr");
        assert!(r.calls.borrow().is_empty());
    }

    #[test]
    fn env_mode() {
        let auth = Auth::new("github.com", AuthMode::Env("GH_T".into()));
        let get = |k: &str| (k == "GH_T").then(|| "ghp_env".to_string());
        let t = auth
            .resolve(&Runner::with(None), &MemorySecretStore::new(), get)
            .unwrap();
        assert_eq!(t.origin, TokenOrigin::Env("GH_T".into()));
        assert!(auth
            .resolve(&Runner::with(None), &MemorySecretStore::new(), none)
            .is_err());
    }

    #[test]
    fn command_mode() {
        let r = Runner::ok("ghp_cmd\n");
        let auth = Auth::new("github.com", AuthMode::Command).with_token_command("pass gh");
        let t = auth.resolve(&r, &MemorySecretStore::new(), none).unwrap();
        assert_eq!(t.secret.expose(), "ghp_cmd");
        assert_eq!(t.origin, TokenOrigin::Command);
        assert!(r.calls.borrow()[0].1.contains(&"pass gh".to_string()));
    }

    #[test]
    fn resolved_token_debug_is_redacted() {
        let t = ResolvedToken {
            secret: Secret::new("ghp_topsecret"),
            origin: TokenOrigin::Keyring,
        };
        assert!(!format!("{t:?}").contains("topsecret"));
    }
}
