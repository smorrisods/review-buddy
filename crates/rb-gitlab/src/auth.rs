use rb_core::Error;
use rb_platform::auth::{token_from_cli, token_from_command, token_from_env, AuthMode, CliTool};
use rb_platform::{CommandRunner, Os, Secret, SecretStore};

/// Where a resolved token came from. Never includes the token itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenOrigin {
    GlabCli,
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
    #[error("no token found for {host}. Run `glab auth login --hostname {host}`, or store one with `review-buddy auth login --host {host}`")]
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

/// How to find the token for one GitLab host.
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

    /// `cli` tries `glab config get token --host` (then `glab auth status -t`) and falls back
    /// to the keyring.
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
            AuthMode::Cli => match token_from_cli(CliTool::Glab, host, runner) {
                Ok(secret) => Ok(ResolvedToken {
                    secret,
                    origin: TokenOrigin::GlabCli,
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
        direct: Option<&'static str>,
        status: Option<&'static str>,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl Runner {
        fn new(direct: Option<&'static str>, status: Option<&'static str>) -> Self {
            Self {
                direct,
                status,
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
                .push(args.iter().map(|a| a.to_string()).collect());
            let out = if args.first() == Some(&"config") {
                self.direct
            } else {
                self.status
            };
            out.map(|stdout| CommandOutput {
                success: true,
                stdout: stdout.into(),
                stderr: String::new(),
            })
            .ok_or_else(|| PlatformError::CommandFailed {
                program: program.into(),
            })
        }
    }

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn cli_uses_glab_config_for_the_host() {
        let r = Runner::new(Some("glpat-abc\n"), None);
        let auth = Auth::new("gitlab.example.com", AuthMode::Cli);
        let t = auth.resolve(&r, &MemorySecretStore::new(), none).unwrap();
        assert_eq!(t.secret.expose(), "glpat-abc");
        assert_eq!(t.origin, TokenOrigin::GlabCli);
        assert_eq!(
            r.calls.borrow()[0],
            ["config", "get", "token", "--host", "gitlab.example.com"]
        );
    }

    #[test]
    fn cli_falls_back_to_glab_auth_status() {
        let r = Runner::new(None, Some("gitlab.com\n  ✓ Token found: glpat-status\n"));
        let t = Auth::new("gitlab.com", AuthMode::Cli)
            .resolve(&r, &MemorySecretStore::new(), none)
            .unwrap();
        assert_eq!(t.secret.expose(), "glpat-status");
        assert_eq!(t.origin, TokenOrigin::GlabCli);
        assert_eq!(r.calls.borrow().len(), 2);
    }

    #[test]
    fn cli_falls_back_to_keyring() {
        let store = MemorySecretStore::new();
        store.set("gitlab.com", &Secret::new("glpat-kr")).unwrap();
        let t = Auth::new("gitlab.com", AuthMode::Cli)
            .resolve(&Runner::new(None, None), &store, none)
            .unwrap();
        assert_eq!(t.origin, TokenOrigin::Keyring);
    }

    #[test]
    fn cli_with_nothing_asks_to_sign_in() {
        let e = Auth::new("gitlab.com", AuthMode::Cli)
            .resolve(&Runner::new(None, None), &MemorySecretStore::new(), none)
            .unwrap_err();
        assert!(e.to_string().contains("glab auth login"));
        assert!(matches!(Error::from(e), Error::Unauthorized { .. }));
    }

    #[test]
    fn token_mode_skips_glab() {
        let r = Runner::new(Some("glpat-no"), None);
        let store = MemorySecretStore::new();
        store.set("gitlab.com", &Secret::new("glpat-kr")).unwrap();
        let t = Auth::new("gitlab.com", AuthMode::Token)
            .resolve(&r, &store, none)
            .unwrap();
        assert_eq!(t.secret.expose(), "glpat-kr");
        assert!(r.calls.borrow().is_empty());
    }

    #[test]
    fn env_mode() {
        let auth = Auth::new("gitlab.com", AuthMode::Env("GL_T".into()));
        let get = |k: &str| (k == "GL_T").then(|| "glpat-env".to_string());
        let r = Runner::new(None, None);
        let t = auth.resolve(&r, &MemorySecretStore::new(), get).unwrap();
        assert_eq!(t.origin, TokenOrigin::Env("GL_T".into()));
        assert!(matches!(
            auth.resolve(&r, &MemorySecretStore::new(), none),
            Err(AuthError::Unavailable { .. })
        ));
    }

    #[test]
    fn command_mode() {
        let r = Runner::new(Some("glpat-cmd\n"), Some("glpat-cmd\n"));
        let auth = Auth::new("gitlab.com", AuthMode::Command).with_token_command("pass gl");
        let t = auth.resolve(&r, &MemorySecretStore::new(), none).unwrap();
        assert_eq!(t.secret.expose(), "glpat-cmd");
        assert_eq!(t.origin, TokenOrigin::Command);
        assert!(r.calls.borrow()[0].contains(&"pass gl".to_string()));
    }

    #[test]
    fn resolved_token_debug_is_redacted() {
        let t = ResolvedToken {
            secret: Secret::new("glpat-topsecret"),
            origin: TokenOrigin::Keyring,
        };
        assert!(!format!("{t:?}").contains("topsecret"));
    }
}
