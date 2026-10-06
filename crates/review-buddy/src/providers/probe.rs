//! The capability probe with its cache: a fresh saved answer for the host is reused, otherwise the
//! provider is asked and the answer saved. Time comes in as an argument so TTLs are testable.

use std::sync::Mutex;

use rb_core::{ForgeKind, ProbeOutcome, Provider, Result, Timestamp};
use rb_store::{probe_key, Store};

/// How long a probe stays good: a day, so launches don't ask again.
pub const PROBE_TTL_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub outcome: ProbeOutcome,
    /// When the instance was asked.
    pub at: Timestamp,
    pub cached: bool,
}

/// Probes `provider`, reusing a saved answer unless `force` is set. Only complete answers are
/// saved, so a blip never pins the fallback for a day.
pub async fn probe_cached(
    provider: &dyn Provider,
    cache: Option<&Mutex<Store>>,
    host: &str,
    now: Timestamp,
    force: bool,
) -> Result<Probed> {
    let key = probe_key(provider.kind(), host);
    if let (false, Some(cache)) = (force, cache) {
        let store = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(Some((outcome, at))) = store.get_probe(&key, now, PROBE_TTL_SECS) {
            return Ok(Probed {
                outcome,
                at,
                cached: true,
            });
        }
    }
    let outcome = provider.probe().await?;
    if let (true, Some(cache)) = (outcome.complete, cache) {
        let mut store = cache.lock().unwrap_or_else(|e| e.into_inner());
        let _ = store.put_probe(&key, &outcome, now);
    }
    Ok(Probed {
        outcome,
        at: now,
        cached: false,
    })
}

/// Forgets a host's saved probe, for when its sign-in changes.
pub fn forget(cache: &mut Store, kind: ForgeKind, host: &str) {
    let _ = cache.clear_probe(&probe_key(kind, host));
}

#[cfg(test)]
mod tests {
    use rb_gitlab::{GitlabClient, GitlabProvider};
    use rb_platform::Secret;
    use serde_json::json;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn server(version: serde_json::Value) -> (MockServer, GitlabProvider) {
        let server = MockServer::start().await;
        Mock::given(path("/version"))
            .respond_with(ResponseTemplate::new(200).set_body_json(version))
            .mount(&server)
            .await;
        Mock::given(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"username": "me"})))
            .mount(&server)
            .await;
        Mock::given(path("/personal_access_tokens/self"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"scopes": ["api"]})))
            .mount(&server)
            .await;
        let client = GitlabClient::new("gl.test", Some(&server.uri()), Secret::new("t")).unwrap();
        (server, GitlabProvider::new(client))
    }

    async fn version_calls(server: &MockServer) -> usize {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/version")
            .count()
    }

    fn cache() -> Mutex<Store> {
        Mutex::new(Store::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn a_fresh_answer_is_reused_until_the_ttl_then_asked_again() {
        let (server, p) = server(json!({"version": "17.4.0", "enterprise": true})).await;
        let cache = cache();
        let at = |t| Timestamp(1_000 + t);
        let first = probe_cached(&p, Some(&cache), "Gl.Test", at(0), false)
            .await
            .unwrap();
        assert!(!first.cached && first.outcome.capabilities.request_changes);
        let second = probe_cached(&p, Some(&cache), "gl.test", at(PROBE_TTL_SECS - 1), false)
            .await
            .unwrap();
        assert!(second.cached);
        assert_eq!(second.at, at(0));
        assert_eq!(second.outcome, first.outcome);
        assert_eq!(version_calls(&server).await, 1);
        let third = probe_cached(&p, Some(&cache), "gl.test", at(PROBE_TTL_SECS), false)
            .await
            .unwrap();
        assert!(!third.cached);
        assert_eq!(version_calls(&server).await, 2);
    }

    #[tokio::test]
    async fn force_skips_the_cache_and_forget_clears_it() {
        let (server, p) = server(json!({"version": "17.4.0", "enterprise": true})).await;
        let cache = cache();
        probe_cached(&p, Some(&cache), "h", Timestamp(0), false)
            .await
            .unwrap();
        probe_cached(&p, Some(&cache), "h", Timestamp(1), true)
            .await
            .unwrap();
        assert_eq!(version_calls(&server).await, 2);
        forget(&mut cache.lock().unwrap(), ForgeKind::GitLab, "h");
        let again = probe_cached(&p, Some(&cache), "h", Timestamp(2), false)
            .await
            .unwrap();
        assert!(!again.cached);
    }

    #[tokio::test]
    async fn incomplete_answers_are_not_saved() {
        let (server, p) = server(json!({"version": "soon"})).await;
        let cache = cache();
        let first = probe_cached(&p, Some(&cache), "h", Timestamp(0), false)
            .await
            .unwrap();
        assert!(!first.outcome.complete);
        let again = probe_cached(&p, Some(&cache), "h", Timestamp(1), false)
            .await
            .unwrap();
        assert!(!again.cached);
        assert_eq!(version_calls(&server).await, 2);
        probe_cached(&p, None, "h", Timestamp(2), false)
            .await
            .unwrap();
    }
}
