//! The capability probe: what this GitLab instance and token can do, from `GET /version` and the
//! token's scopes. Everything here is pure except [`probe`], so the rules are table-tested.

use rb_core::{Capabilities, Error, FeatureAction, ProbeOutcome, Result};
use serde::Deserialize;

use crate::GitlabClient;

/// A GitLab version, `major.minor.patch`. Suffixes such as `-pre` or `-ee` are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let core = text.trim().split(['-', '+', ' ']).next()?;
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

/// Requesting changes (the `requested_changes` reviewer state) is a Premium and Ultimate feature.
/// It arrived in 16.11 behind a flag, was on by default from 17.2 and the flag was removed in 17.3,
/// so 17.3 is the first version it can be relied on.
/// <https://docs.gitlab.com/user/project/merge_requests/reviews/#request-changes>
pub const REQUEST_CHANGES_SINCE: Version = Version::new(17, 3, 0);

/// Why an instance can or can't request changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestChanges {
    Available,
    TooOld,
    /// Community Edition builds, which `/version` marks with `enterprise: false`. An enterprise
    /// build without a licence still looks the same here; GitLab then refuses and we say so.
    NeedsPaidTier,
}

pub fn request_changes(version: Version, enterprise: bool) -> RequestChanges {
    if version < REQUEST_CHANGES_SINCE {
        RequestChanges::TooOld
    } else if !enterprise {
        RequestChanges::NeedsPaidTier
    } else {
        RequestChanges::Available
    }
}

/// Retrying pipelines needs the `api` scope. A token that can't list its scopes is given the
/// benefit of the doubt.
pub fn can_retry_pipelines(scopes: &[String]) -> bool {
    scopes.is_empty() || scopes.iter().any(|s| s == "api")
}

/// Capabilities and reasons from what the probe read.
pub fn decide(version: Version, enterprise: bool, scopes: &[String], host: &str) -> ProbeOutcome {
    let decision = request_changes(version, enterprise);
    let mut reasons = Vec::new();
    let shown = format!("{}.{}", version.major, version.minor);
    match decision {
        RequestChanges::Available => {}
        RequestChanges::TooOld => reasons.push((
            FeatureAction::RequestChanges,
            format!("GitLab {shown} on {host} doesn't support requesting changes (it needs 17.3 or newer)."),
        )),
        RequestChanges::NeedsPaidTier => reasons.push((
            FeatureAction::RequestChanges,
            format!("GitLab {shown} on {host} doesn't include requesting changes (it needs Premium or Ultimate)."),
        )),
    }
    let rerun = can_retry_pipelines(scopes);
    if !rerun {
        reasons.push((
            FeatureAction::RerunFailed,
            format!(
                "Your token for {host} can't retry pipelines. Create one with the `api` scope."
            ),
        ));
    }
    ProbeOutcome {
        capabilities: Capabilities {
            request_changes: decision == RequestChanges::Available,
            viewed_files: false,
            range_comments: true,
            suggestions: true,
            resolve_threads: true,
            rerun_failed: rerun,
        },
        version: Some(version.to_string()),
        reasons,
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Deserialize)]
struct VersionBody {
    version: String,
    #[serde(default)]
    enterprise: bool,
}

/// Reads `GET /version` and the token's scopes. A rejected sign-in is an error; anything else
/// that goes wrong (an older or locked-down instance, a proxy, a timeout) falls back to the
/// static answer with no version, which callers don't cache.
pub async fn probe(client: &GitlabClient, fallback: Capabilities) -> Result<ProbeOutcome> {
    let host = client.host().to_string();
    let body = match client.get_json::<VersionBody>("/version").await {
        Ok(body) => body,
        Err(e @ Error::Unauthorized { .. }) => return Err(e),
        Err(_) => return Ok(ProbeOutcome::new(fallback)),
    };
    let Some(version) = Version::parse(&body.version) else {
        return Ok(ProbeOutcome::new(fallback));
    };
    let scopes = match client.test_token().await {
        Ok(report) => report.scopes,
        Err(e @ Error::Unauthorized { .. }) => return Err(e),
        Err(_) => Vec::new(),
    };
    Ok(decide(version, body.enterprise, &scopes, &host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_with_or_without_suffixes() {
        let v = |s| Version::parse(s);
        assert_eq!(v("17.4.1"), Some(Version::new(17, 4, 1)));
        assert_eq!(v("17.5.0-pre"), Some(Version::new(17, 5, 0)));
        assert_eq!(v("16.11.2-ee"), Some(Version::new(16, 11, 2)));
        assert_eq!(v("18.0"), Some(Version::new(18, 0, 0)));
        assert_eq!(v(" 15.6.0 "), Some(Version::new(15, 6, 0)));
        assert_eq!(v(""), None);
        assert_eq!(v("latest"), None);
        assert_eq!(v("17"), None);
        assert_eq!(v("17.x.1"), None);
    }

    #[test]
    fn versions_order_numerically() {
        assert!(Version::new(17, 10, 0) > Version::new(17, 9, 9));
        assert!(Version::new(16, 99, 0) < Version::new(17, 0, 0));
    }

    #[test]
    fn request_changes_follows_version_and_edition() {
        use RequestChanges::*;
        let table = [
            ((16, 9, 0), true, TooOld),
            ((17, 2, 9), true, TooOld),
            ((17, 3, 0), true, Available),
            ((17, 3, 0), false, NeedsPaidTier),
            ((18, 1, 4), true, Available),
            ((18, 1, 4), false, NeedsPaidTier),
            ((15, 0, 0), false, TooOld),
        ];
        for ((a, b, c), enterprise, want) in table {
            assert_eq!(
                request_changes(Version::new(a, b, c), enterprise),
                want,
                "{a}.{b}.{c} enterprise={enterprise}"
            );
        }
    }

    #[test]
    fn pipeline_retry_needs_the_api_scope_unless_scopes_are_unknown() {
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(can_retry_pipelines(&[]));
        assert!(can_retry_pipelines(&s(&["api"])));
        assert!(can_retry_pipelines(&s(&["read_user", "api"])));
        assert!(!can_retry_pipelines(&s(&["read_api"])));
    }

    #[test]
    fn decisions_carry_calm_reasons() {
        let old = decide(Version::new(16, 9, 2), true, &[], "gitlab.example.com");
        assert!(!old.capabilities.request_changes);
        assert_eq!(
            old.reason(FeatureAction::RequestChanges),
            Some("GitLab 16.9 on gitlab.example.com doesn't support requesting changes (it needs 17.3 or newer).")
        );
        assert_eq!(old.version.as_deref(), Some("16.9.2"));
        let ce = decide(Version::new(17, 4, 0), false, &[], "gl.test");
        assert!(ce
            .reason(FeatureAction::RequestChanges)
            .unwrap()
            .contains("Premium or Ultimate"));
        let ok = decide(
            Version::new(17, 4, 0),
            true,
            &["read_api".into()],
            "gl.test",
        );
        assert!(ok.capabilities.request_changes && !ok.capabilities.rerun_failed);
        assert!(ok.reason(FeatureAction::RequestChanges).is_none());
        assert!(ok.reason(FeatureAction::RerunFailed).is_some());
        assert!(ok.capabilities.suggestions && !ok.capabilities.viewed_files);
    }
}
