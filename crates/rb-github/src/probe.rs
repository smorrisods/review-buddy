//! The capability probe for GitHub. GitHub offers every action, so the only thing to learn is
//! whether the token may re-run workflows. Classic tokens list their scopes in `X-OAuth-Scopes`;
//! fine-grained tokens and GitHub Apps don't, so unknown scopes get the benefit of the doubt.

use rb_core::{Capabilities, Error, FeatureAction, ProbeOutcome, Result};

use crate::GithubClient;

/// Classic tokens re-run workflows with `repo` (or `workflow`).
/// <https://docs.github.com/en/rest/actions/workflow-runs#re-run-failed-jobs-from-a-workflow-run>
pub fn can_rerun_workflows(scopes: &[String]) -> bool {
    scopes.is_empty() || scopes.iter().any(|s| s == "repo" || s == "workflow")
}

pub fn decide(scopes: &[String], host: &str) -> ProbeOutcome {
    let rerun = can_rerun_workflows(scopes);
    let mut out = ProbeOutcome::new(Capabilities {
        rerun_failed: rerun,
        ..Capabilities::all()
    });
    out.complete = true;
    if !rerun {
        out.reasons.push((
            FeatureAction::RerunFailed,
            format!("Your token for {host} can't re-run workflows. Create one with the `repo` or `workflow` scope."),
        ));
    }
    out
}

/// Reads the token's scopes. A rejected sign-in is an error; other failures fall back to the
/// static answer. GitHub has no version to report, so the outcome's `version` is `None` and
/// callers keep it cached by its scopes alone.
pub async fn probe(client: &GithubClient) -> Result<ProbeOutcome> {
    match client.test_token().await {
        Ok(report) => Ok(decide(&report.scopes, client.host())),
        Err(e @ Error::Unauthorized { .. }) => Err(e),
        Err(_) => Ok(ProbeOutcome::new(Capabilities::all())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rerun_follows_classic_scopes_and_trusts_unknown_ones() {
        assert!(can_rerun_workflows(&[]));
        assert!(can_rerun_workflows(&s(&["repo", "read:org"])));
        assert!(can_rerun_workflows(&s(&["workflow"])));
        assert!(!can_rerun_workflows(&s(&["read:org", "gist"])));
    }

    #[test]
    fn decisions_keep_everything_else_on_and_explain_a_missing_scope() {
        let ok = decide(&s(&["repo"]), "github.com");
        assert_eq!(ok.capabilities, Capabilities::all());
        let limited = decide(&s(&["read:org"]), "github.com");
        assert!(!limited.capabilities.rerun_failed && limited.capabilities.request_changes);
        assert!(limited.reason(FeatureAction::RerunFailed).is_some());
    }
}
