//! What a command can fail with, and the exit code each failure maps to.

use std::process::ExitCode;

use thiserror::Error;

use super::selector::SelectorError;

/// The exit codes in `docs/cli.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Success,
    Failure,
    Usage,
    Cancelled,
    AuthNeeded,
    Unsupported,
    ChecksPending,
}

impl Exit {
    pub const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Failure => 1,
            Self::Usage => 2,
            Self::Cancelled => 3,
            Self::AuthNeeded => 4,
            Self::Unsupported => 5,
            Self::ChecksPending => 8,
        }
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        ExitCode::from(exit.code())
    }
}

/// A command failure. The message is one or two calm lines: what happened, then what to try.
#[derive(Debug, Error)]
pub enum CmdError {
    #[error("{0}")]
    Usage(String),
    #[error("Not built yet. It's planned for {milestone}.\nTry review-buddy --help to see what's available.")]
    NotBuilt { milestone: &'static str },
    #[error("{0}")]
    Cancelled(String),
    #[error("{0}")]
    AuthNeeded(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    ChecksPending(String),
    #[error("{0}")]
    Failed(String),
    #[error(transparent)]
    Selector(#[from] SelectorError),
}

impl CmdError {
    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }

    pub fn exit(&self) -> Exit {
        match self {
            Self::Usage(_) | Self::NotBuilt { .. } | Self::Selector(_) => Exit::Usage,
            Self::Cancelled(_) => Exit::Cancelled,
            Self::AuthNeeded(_) => Exit::AuthNeeded,
            Self::Unsupported(_) => Exit::Unsupported,
            Self::ChecksPending(_) => Exit::ChecksPending,
            Self::Failed(_) => Exit::Failure,
        }
    }
}

impl From<rb_core::Error> for CmdError {
    fn from(err: rb_core::Error) -> Self {
        use rb_core::Error;
        match err {
            Error::Unauthorized { host } => Self::AuthNeeded(format!(
                "The token for {host} was rejected.\nRun review-buddy auth login --host {host}, or check the source's auth setting."
            )),
            Error::Unsupported(what) => Self::Unsupported(format!(
                "{what} isn't supported by this forge.\nLeave a comment instead, or use the web page."
            )),
            Error::RateLimited {
                host,
                retry_after_secs,
            } => {
                let wait = retry_after_secs
                    .map_or_else(|| "a little while".to_string(), |s| format!("{s} seconds"));
                Self::Failed(format!(
                    "{host} is asking us to slow down.\nTry again in {wait}."
                ))
            }
            Error::NotFound(what) => Self::Failed(format!(
                "Couldn't find {what}.\nCheck the reference with review-buddy pr list."
            )),
            Error::Network { host, reason } => Self::Failed(format!(
                "Couldn't reach {host}: {reason}.\nCheck your connection and try again, or use --demo."
            )),
            Error::Forbidden { host, reason } => {
                let text = format!(
                    "{host} refused the request: {reason}.\nCheck the token's scopes with review-buddy auth status."
                );
                if reason.contains("scope") {
                    Self::AuthNeeded(text)
                } else {
                    Self::Failed(text)
                }
            }
            Error::Conflict(what) => Self::Failed(format!(
                "That conflicts with the current state: {what}.\nRefresh and try again."
            )),
            Error::Api(what) => Self::Failed(format!(
                "The forge sent something unexpected: {what}.\nTry again in a moment."
            )),
        }
    }
}

impl From<std::io::Error> for CmdError {
    fn from(err: std::io::Error) -> Self {
        Self::Failed(format!("Couldn't write the output: {err}."))
    }
}

impl From<serde_json::Error> for CmdError {
    fn from(err: serde_json::Error) -> Self {
        Self::Failed(format!("Couldn't encode the output as JSON: {err}."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_the_table() {
        let cases = [
            (CmdError::usage("x"), 2),
            (
                CmdError::NotBuilt {
                    milestone: "v0.3.0",
                },
                2,
            ),
            (CmdError::Cancelled("x".into()), 3),
            (CmdError::AuthNeeded("x".into()), 4),
            (CmdError::Unsupported("x".into()), 5),
            (CmdError::ChecksPending("x".into()), 8),
            (CmdError::failed("x"), 1),
            (SelectorError::NoRepository.into(), 2),
        ];
        for (err, code) in cases {
            assert_eq!(err.exit().code(), code, "{err}");
        }
        assert_eq!(Exit::Success.code(), 0);
    }

    #[test]
    fn forge_errors_map_to_codes_and_stay_short() {
        let cases = [
            (
                rb_core::Error::Unauthorized {
                    host: "gitlab.work.ca".into(),
                },
                4,
            ),
            (rb_core::Error::Unsupported("request changes".into()), 5),
            (
                rb_core::Error::Network {
                    host: "github.com".into(),
                    reason: "timed out".into(),
                },
                1,
            ),
            (rb_core::Error::NotFound("a/b#1".into()), 1),
            (
                rb_core::Error::RateLimited {
                    host: "github.com".into(),
                    retry_after_secs: Some(30),
                },
                1,
            ),
            (rb_core::Error::Conflict("stale".into()), 1),
            (rb_core::Error::Api("odd".into()), 1),
            (
                rb_core::Error::Forbidden {
                    host: "h".into(),
                    reason: "no".into(),
                },
                1,
            ),
        ];
        for (forge, code) in cases {
            let err = CmdError::from(forge);
            assert_eq!(err.exit().code(), code);
            let text = err.to_string();
            assert!(text.lines().count() <= 2, "{text}");
            assert!(!text.contains("Error"), "{text}");
        }
    }

    #[test]
    fn auth_message_names_the_next_step() {
        let text = CmdError::from(rb_core::Error::Unauthorized {
            host: "gitlab.work.ca".into(),
        })
        .to_string();
        assert!(text.contains("review-buddy auth login --host gitlab.work.ca"));
    }

    #[test]
    fn not_built_names_the_milestone() {
        let text = CmdError::NotBuilt {
            milestone: "v0.3.0",
        }
        .to_string();
        assert!(text.starts_with("Not built yet. It's planned for v0.3.0."));
    }
}
