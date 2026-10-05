use crate::{CommandRunner, Os, PlatformError, Secret};

/// The forge CLI whose existing sign-in can be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliTool {
    Gh,
    Glab,
}

/// How a host's token is obtained, mirroring the `auth` config key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthMode {
    Cli,
    Token,
    Env(String),
    Command,
}

impl AuthMode {
    /// Parses `cli`, `token`, `command` or `env:VAR_NAME`.
    pub fn parse(s: &str) -> Result<Self, PlatformError> {
        match s.trim() {
            "cli" => Ok(Self::Cli),
            "token" => Ok(Self::Token),
            "command" => Ok(Self::Command),
            other => match other.strip_prefix("env:") {
                Some(var) if !var.is_empty() => Ok(Self::Env(var.to_string())),
                _ => Err(PlatformError::InvalidAuth(other.to_string())),
            },
        }
    }
}

fn non_empty(text: &str, source: &str) -> Result<Secret, PlatformError> {
    let t = text.trim();
    if t.is_empty() {
        Err(PlatformError::EmptyToken(source.to_string()))
    } else {
        Ok(Secret::new(t))
    }
}

/// Reuses the token from `gh` or `glab`. Read on each call; never copied to disk.
pub fn token_from_cli(
    tool: CliTool,
    host: &str,
    runner: &dyn CommandRunner,
) -> Result<Secret, PlatformError> {
    match tool {
        CliTool::Gh => {
            let out = runner.run("gh", &["auth", "token", "--hostname", host], None)?;
            if !out.success {
                return Err(PlatformError::CommandFailed {
                    program: "gh".into(),
                });
            }
            non_empty(&out.stdout, "gh")
        }
        CliTool::Glab => {
            let direct = runner
                .run("glab", &["config", "get", "token", "--host", host], None)
                .ok()
                .filter(|o| o.success)
                .and_then(|o| non_empty(&o.stdout, "glab").ok());
            if let Some(t) = direct {
                return Ok(t);
            }
            let out = runner.run("glab", &["auth", "status", "-t", "--hostname", host], None)?;
            if !out.success {
                return Err(PlatformError::CommandFailed {
                    program: "glab".into(),
                });
            }
            parse_glab_status(&format!("{}\n{}", out.stdout, out.stderr))
                .ok_or_else(|| PlatformError::EmptyToken("glab".into()))
        }
    }
}

/// Finds the value on a `Token: …` / `Token found: …` line of `glab auth status -t`.
fn parse_glab_status(text: &str) -> Option<Secret> {
    text.lines().find_map(|line| {
        let (label, value) = line.rsplit_once(':')?;
        let label = label.trim().to_ascii_lowercase();
        let value = value.trim();
        (label.ends_with("token") || label.ends_with("token found"))
            .then_some(value)
            .filter(|v| !v.is_empty() && !v.contains(' ') && !v.chars().all(|c| c == '*'))
            .map(Secret::new)
    })
}

/// Reads a token from an environment variable via `getenv`.
pub fn token_from_env(
    var: &str,
    getenv: impl Fn(&str) -> Option<String>,
) -> Result<Secret, PlatformError> {
    match getenv(var) {
        Some(v) if !v.trim().is_empty() => Ok(Secret::new(v.trim())),
        _ => Err(PlatformError::EnvMissing(var.to_string())),
    }
}

/// Runs `token_command` through the platform shell; trimmed stdout is the token.
pub fn token_from_command(
    command: &str,
    os: Os,
    runner: &dyn CommandRunner,
) -> Result<Secret, PlatformError> {
    let (shell, flag) = if os == Os::Windows {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let out = runner.run(shell, &[flag, command], None)?;
    if !out.success {
        return Err(PlatformError::CommandFailed {
            program: "token_command".into(),
        });
    }
    non_empty(&out.stdout, "token_command")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::fake::FakeRunner;

    #[test]
    fn parses_modes() {
        assert_eq!(AuthMode::parse("cli").unwrap(), AuthMode::Cli);
        assert_eq!(AuthMode::parse("token").unwrap(), AuthMode::Token);
        assert_eq!(AuthMode::parse("command").unwrap(), AuthMode::Command);
        assert_eq!(
            AuthMode::parse("env:GL_TOKEN").unwrap(),
            AuthMode::Env("GL_TOKEN".into())
        );
        assert!(AuthMode::parse("env:").is_err());
        assert!(AuthMode::parse("magic").is_err());
    }

    #[test]
    fn gh_token_uses_hostname() {
        let r = FakeRunner::default().with("gh", true, "gho_x\n", "");
        let t = token_from_cli(CliTool::Gh, "ghe.example.com", &r).unwrap();
        assert_eq!(t.expose(), "gho_x");
        assert_eq!(
            r.calls.borrow()[0].1,
            ["auth", "token", "--hostname", "ghe.example.com"]
        );
    }

    #[test]
    fn gh_failure_and_empty() {
        let r = FakeRunner::default().with("gh", false, "", "not logged in");
        assert!(token_from_cli(CliTool::Gh, "github.com", &r).is_err());
        let r = FakeRunner::default().with("gh", true, "  \n", "");
        assert!(matches!(
            token_from_cli(CliTool::Gh, "github.com", &r),
            Err(PlatformError::EmptyToken(_))
        ));
        assert!(token_from_cli(CliTool::Gh, "github.com", &FakeRunner::default()).is_err());
    }

    #[test]
    fn glab_config_get() {
        let r = FakeRunner::default().with("glab", true, "glpat-1\n", "");
        let t = token_from_cli(CliTool::Glab, "gitlab.com", &r).unwrap();
        assert_eq!(t.expose(), "glpat-1");
        assert_eq!(r.calls.borrow().len(), 1);
    }

    #[test]
    fn glab_status_parsing() {
        let text = "gitlab.com\n  ✓ Logged in to gitlab.com as me\n  ✓ Token found: glpat-abc\n";
        assert_eq!(parse_glab_status(text).unwrap().expose(), "glpat-abc");
        assert_eq!(
            parse_glab_status("  ✓ Token: glpat-zzz").unwrap().expose(),
            "glpat-zzz"
        );
        assert!(parse_glab_status("  ✓ Token: **************").is_none());
    }

    #[test]
    fn env_token() {
        let get = |k: &str| (k == "T").then(|| " abc \n".to_string());
        assert_eq!(token_from_env("T", get).unwrap().expose(), "abc");
        assert!(matches!(
            token_from_env("U", get),
            Err(PlatformError::EnvMissing(_))
        ));
        assert!(token_from_env("E", |_| Some("  ".into())).is_err());
    }

    #[test]
    fn command_token_via_shell() {
        let r = FakeRunner::default().with("sh", true, "secret\n", "");
        let t = token_from_command("pass show x", Os::Linux, &r).unwrap();
        assert_eq!(t.expose(), "secret");
        assert_eq!(r.calls.borrow()[0].1, ["-c", "pass show x"]);
        let r = FakeRunner::default().with("cmd", false, "", "");
        assert!(token_from_command("x", Os::Windows, &r).is_err());
        assert_eq!(r.calls.borrow()[0].1[0], "/C");
    }

    #[test]
    fn errors_do_not_leak_tokens() {
        let r = FakeRunner::default().with("sh", false, "topsecret", "topsecret");
        let e = token_from_command("x", Os::Linux, &r).unwrap_err();
        assert!(!format!("{e} {e:?}").contains("topsecret"));
    }
}
