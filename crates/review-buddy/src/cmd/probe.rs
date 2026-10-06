//! What `doctor` and `source test` say about a source's capability probe. Both ask the instance
//! afresh (they exist to check), and save the answer for the interface to reuse.

// Without `live` there is no network, so some of this is only reachable from tests.
#![cfg_attr(not(feature = "live"), allow(dead_code, unused_imports))]

use rb_core::{ProbeOutcome, Timestamp};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

use super::auth::{SourceAuth, State};
use super::changes::iso;
use super::context::Context;
use super::DEMO_LABEL;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub outcome: ProbeOutcome,
    /// When the instance was asked; `None` for demo sources, which have nothing to ask.
    pub at: Option<Timestamp>,
}

impl Detected {
    /// `GitLab 17.4.1, probed 2026-10-06T12:00:00Z`, or just the time when there's no version.
    pub fn summary(&self, forge: &str) -> String {
        let Some(at) = self.at else {
            return format!("demo capabilities {DEMO_LABEL}");
        };
        match &self.outcome.version {
            Some(version) => format!("{forge} {version}, probed {}", iso(at)),
            None if self.outcome.complete => format!("probed {}", iso(at)),
            None => "version unknown, using what the forge always offers".to_string(),
        }
    }

    pub fn json(&self) -> Value {
        json!({
            "version": self.outcome.version,
            "probedAt": self.at.map(iso),
            "complete": self.outcome.complete,
            "reasons": self.outcome.reasons.iter().map(|(action, text)| json!({
                "action": format!("{action:?}"),
                "reason": text,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Probes one signed-in source. `None` when sign-in failed or the probe couldn't run.
pub fn detect(ctx: &Context, runtime: &Runtime, auth: &SourceAuth) -> Option<Detected> {
    if !matches!(auth.state, State::SignedIn { .. }) {
        return None;
    }
    if ctx.is_demo() {
        let source = ctx
            .sources()
            .ok()?
            .into_iter()
            .find(|s| s.label == auth.name)?;
        let provider = ctx.provider_for(&source).ok()?;
        return Some(Detected {
            outcome: ProbeOutcome {
                complete: true,
                ..ProbeOutcome::new(provider.capabilities())
            },
            at: None,
        });
    }
    #[cfg(feature = "live")]
    {
        use std::sync::Mutex;

        let provider = ctx
            .factory()
            .provider(&rb_core::SourceId::new(&auth.name))
            .ok()?;
        let cache = ctx.cache().ok().map(Mutex::new);
        let probed = runtime
            .block_on(crate::providers::probe::probe_cached(
                provider.as_ref(),
                cache.as_ref(),
                &auth.host,
                crate::load::now(),
                true,
            ))
            .ok()?;
        Some(Detected {
            outcome: probed.outcome,
            at: Some(probed.at),
        })
    }
    #[cfg(not(feature = "live"))]
    {
        let _ = runtime;
        None
    }
}

/// Drops a host's saved probe because its sign-in changed, so the next look is a fresh one.
#[cfg(feature = "live")]
pub fn forget(ctx: &Context, kind: rb_core::ForgeKind, host: &str) {
    if let Ok(mut cache) = ctx.cache() {
        crate::providers::probe::forget(&mut cache, kind, host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detected(version: Option<&str>, complete: bool) -> Detected {
        Detected {
            outcome: ProbeOutcome {
                version: version.map(str::to_string),
                complete,
                ..ProbeOutcome::new(rb_core::Capabilities::all())
            },
            at: Some(Timestamp(0)),
        }
    }

    #[test]
    fn summaries_say_what_was_learned() {
        assert_eq!(
            detected(Some("17.4.1"), true).summary("GitLab"),
            "GitLab 17.4.1, probed 1970-01-01T00:00:00Z"
        );
        assert_eq!(
            detected(None, true).summary("GitHub"),
            "probed 1970-01-01T00:00:00Z"
        );
        assert!(detected(None, false).summary("GitLab").contains("unknown"));
    }

    #[test]
    fn json_has_the_version_and_time() {
        let value = detected(Some("17.4.1"), true).json();
        assert_eq!(value["version"], "17.4.1");
        assert_eq!(value["probedAt"], "1970-01-01T00:00:00Z");
    }
}
