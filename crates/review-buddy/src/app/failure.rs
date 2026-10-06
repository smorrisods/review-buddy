//! Why a source couldn't be refreshed, in words that say what to do next.

use rb_core::Error;

pub const SIGN_IN_STEP: &str =
    "Sign in with `gh auth login`, or add a token with `review-buddy auth login`.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    SignIn,
    RateLimited,
    Offline,
    /// The source can't load in this release.
    Unavailable,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFailure {
    pub kind: FailureKind,
    pub summary: String,
    pub next_step: String,
}

impl SourceFailure {
    pub fn sign_in(host: &str) -> Self {
        Self {
            kind: FailureKind::SignIn,
            summary: format!("Not signed in to {host}."),
            next_step: SIGN_IN_STEP.to_string(),
        }
    }

    pub fn from_error(error: &Error, host: &str) -> Self {
        match error {
            Error::Unauthorized { .. } => Self::sign_in(host),
            Error::RateLimited {
                retry_after_secs, ..
            } => Self {
                kind: FailureKind::RateLimited,
                summary: format!("{host} is rate limiting requests."),
                next_step: match retry_after_secs {
                    Some(secs) => format!(
                        "The limit resets in about {}. Press r to try again then.",
                        wait_phrase(*secs)
                    ),
                    None => "Give it a few minutes, then press r to try again.".to_string(),
                },
            },
            Error::Network { .. } => Self {
                kind: FailureKind::Offline,
                summary: format!("Couldn't reach {host}."),
                next_step: "Check your connection, then press r to try again.".to_string(),
            },
            Error::Forbidden { reason, .. } => Self {
                kind: FailureKind::Other,
                summary: format!(
                    "{host} refused the request: {}.",
                    reason.trim_end_matches('.')
                ),
                next_step: "Check the token's access, then press r to try again.".to_string(),
            },
            Error::Api(text) if is_server_error(text) => Self {
                kind: FailureKind::Offline,
                summary: format!("{host} isn't answering properly right now."),
                next_step: "It usually clears by itself. Press r to try again.".to_string(),
            },
            other => Self {
                kind: FailureKind::Other,
                summary: format!("{}.", other.to_string().trim_end_matches('.')),
                next_step: "Press r to try again.".to_string(),
            },
        }
    }

    pub fn unavailable(summary: impl Into<String>, next_step: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::Unavailable,
            summary: summary.into(),
            next_step: next_step.into(),
        }
    }

    /// A few words for the Sources pane.
    pub fn short(&self) -> &'static str {
        match self.kind {
            FailureKind::SignIn => "sign-in needed",
            FailureKind::RateLimited => "rate limited",
            FailureKind::Offline => "offline",
            FailureKind::Unavailable => "not available yet",
            FailureKind::Other => "couldn't refresh",
        }
    }

    /// The headline for an empty queue.
    pub fn headline(&self) -> String {
        match self.kind {
            FailureKind::SignIn => "Sign in to see your reviews.".to_string(),
            _ => self.summary.clone(),
        }
    }

    pub fn toast(&self, label: &str) -> String {
        format!("{label}: {} {}", self.summary, self.next_step)
    }
}

/// A forge answer that was a 5xx, from the `<host> answered 5xx` text the providers write.
pub fn is_server_error(text: &str) -> bool {
    text.split("answered ")
        .nth(1)
        .and_then(|rest| rest.get(..3))
        .is_some_and(|code| code.starts_with('5') && code.bytes().all(|b| b.is_ascii_digit()))
}

fn wait_phrase(secs: u64) -> String {
    let plural = |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    match secs {
        0..=59 => plural(secs.max(1), "second"),
        60..=3599 => plural(secs.div_ceil(60), "minute"),
        _ => {
            let (hours, minutes) = (secs / 3600, secs % 3600 / 60);
            match minutes {
                0 => plural(hours, "hour"),
                m => format!("{} {}", plural(hours, "hour"), plural(m, "minute")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_error_reads_as_offline_with_a_next_step() {
        let f = SourceFailure::from_error(&Error::Api("h answered 503: busy".into()), "h");
        assert_eq!(f.kind, FailureKind::Offline);
        assert!(f.next_step.contains("Press r"));
        let f = SourceFailure::from_error(&Error::Api("h answered 418".into()), "h");
        assert_eq!(f.kind, FailureKind::Other);
    }

    #[test]
    fn unauthorized_says_how_to_sign_in() {
        let f = SourceFailure::from_error(
            &Error::Unauthorized {
                host: "github.com".into(),
            },
            "github.com",
        );
        assert_eq!(f.kind, FailureKind::SignIn);
        assert!(f.next_step.contains("gh auth login"));
        assert!(f.next_step.contains("review-buddy auth login"));
        assert_eq!(f.headline(), "Sign in to see your reviews.");
    }

    #[test]
    fn rate_limits_surface_the_reset_time() {
        let limited = |secs| {
            SourceFailure::from_error(
                &Error::RateLimited {
                    host: "h".into(),
                    retry_after_secs: secs,
                },
                "h",
            )
        };
        assert!(limited(Some(90)).next_step.contains("2 minutes"));
        assert!(limited(Some(30)).next_step.contains("30 seconds"));
        assert!(limited(Some(3720)).next_step.contains("1 hour 2 minutes"));
        assert!(limited(None).next_step.contains("few minutes"));
        assert_eq!(limited(Some(1)).kind, FailureKind::RateLimited);
    }

    #[test]
    fn network_and_other_errors_keep_the_next_step() {
        let net = SourceFailure::from_error(
            &Error::Network {
                host: "h".into(),
                reason: "timed out".into(),
            },
            "h",
        );
        assert_eq!(net.kind, FailureKind::Offline);
        assert!(net.next_step.contains("press r"));
        let other = SourceFailure::from_error(&Error::Api("odd".into()), "h");
        assert_eq!(other.kind, FailureKind::Other);
        assert!(other.toast("work").starts_with("work: "));
    }
}
