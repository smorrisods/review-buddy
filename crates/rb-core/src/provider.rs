use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    ChangeDetail, ChangeId, ChangeSummary, Check, Comment, Etag, FilePatch, ForgeKind, MergeOpts,
    MergeOutcome, Page, Result, ReviewDraft, Scope, Thread, ThreadId, User, Verdict,
};

/// A forge (GitHub or GitLab). UI code never branches on forge; it asks `capabilities()`.
#[async_trait]
pub trait Provider: Send + Sync {
    fn kind(&self) -> ForgeKind;
    async fn whoami(&self) -> Result<User>;
    async fn list_changes(&self, scope: &Scope, since: Option<Etag>)
        -> Result<Page<ChangeSummary>>;
    async fn change_detail(&self, id: &ChangeId) -> Result<ChangeDetail>;
    async fn files(&self, id: &ChangeId) -> Result<Vec<FilePatch>>;
    async fn threads(&self, id: &ChangeId) -> Result<Vec<Thread>>;
    async fn checks(&self, id: &ChangeId) -> Result<Vec<Check>>;
    async fn submit_review(
        &self,
        id: &ChangeId,
        review: &ReviewDraft,
        verdict: Verdict,
    ) -> Result<()>;
    async fn reply(&self, thread: &ThreadId, body: &str) -> Result<Comment>;
    async fn resolve(&self, thread: &ThreadId, resolved: bool) -> Result<()>;
    async fn merge(&self, id: &ChangeId, opts: &MergeOpts) -> Result<MergeOutcome>;
    async fn rerun_failed(&self, id: &ChangeId) -> Result<()>;
    fn checkout_refspec(&self, id: &ChangeId) -> String;
    fn web_url(&self, id: &ChangeId) -> Url;
    fn capabilities(&self) -> Capabilities;
    /// Asks the instance what it can do. Defaults to the static [`capabilities`](Self::capabilities).
    /// Implementations tolerate a failed version check and fall back to the static answer with no
    /// `version`; an error is only for a rejected sign-in.
    async fn probe(&self) -> Result<ProbeOutcome> {
        Ok(ProbeOutcome::new(self.capabilities()))
    }
}

/// What a capability probe learned about one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeOutcome {
    pub capabilities: Capabilities,
    /// The instance version as reported, such as `17.4.1`. `None` when the forge has none to
    /// report or it couldn't be read.
    pub version: Option<String>,
    /// The instance answered the probe. When `false`, `capabilities` is the static fallback and
    /// shouldn't be cached.
    #[serde(default)]
    pub complete: bool,
    /// A calm sentence for each unsupported action that has a specific reason.
    #[serde(default)]
    pub reasons: Vec<(FeatureAction, String)>,
}

impl ProbeOutcome {
    pub fn new(capabilities: Capabilities) -> Self {
        Self {
            capabilities,
            version: None,
            complete: false,
            reasons: Vec::new(),
        }
    }

    /// Why `action` is off here, when the probe knows.
    pub fn reason(&self, action: FeatureAction) -> Option<&str> {
        self.reasons
            .iter()
            .find(|(a, _)| *a == action)
            .map(|(_, r)| r.as_str())
    }
}

/// Optional actions a forge or instance may not offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FeatureAction {
    RequestChanges,
    ViewedFiles,
    RangeComments,
    Suggestions,
    ResolveThreads,
    RerunFailed,
}

impl FeatureAction {
    fn phrase(self) -> &'static str {
        match self {
            Self::RequestChanges => "request changes",
            Self::ViewedFiles => "viewed files",
            Self::RangeComments => "range comments",
            Self::Suggestions => "suggestions",
            Self::ResolveThreads => "resolving threads",
            Self::RerunFailed => "re-running failed jobs",
        }
    }
}

/// What a connected forge instance can do. Unsupported actions are hidden and explained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub request_changes: bool,
    pub viewed_files: bool,
    pub range_comments: bool,
    pub suggestions: bool,
    pub resolve_threads: bool,
    pub rerun_failed: bool,
}

impl Capabilities {
    pub const fn all() -> Self {
        Self {
            request_changes: true,
            viewed_files: true,
            range_comments: true,
            suggestions: true,
            resolve_threads: true,
            rerun_failed: true,
        }
    }

    pub const fn none() -> Self {
        Self {
            request_changes: false,
            viewed_files: false,
            range_comments: false,
            suggestions: false,
            resolve_threads: false,
            rerun_failed: false,
        }
    }

    pub fn supports(&self, action: FeatureAction) -> bool {
        match action {
            FeatureAction::RequestChanges => self.request_changes,
            FeatureAction::ViewedFiles => self.viewed_files,
            FeatureAction::RangeComments => self.range_comments,
            FeatureAction::Suggestions => self.suggestions,
            FeatureAction::ResolveThreads => self.resolve_threads,
            FeatureAction::RerunFailed => self.rerun_failed,
        }
    }

    /// A kind sentence for the footer when `action` is unavailable on `host`; `None` if supported.
    pub fn explain_unsupported(&self, action: FeatureAction, host: &str) -> Option<String> {
        if self.supports(action) {
            return None;
        }
        let hint = match action {
            FeatureAction::RequestChanges => " Leave a comment instead (c).",
            _ => "",
        };
        Some(format!("{host} doesn't support {}.{hint}", action.phrase()))
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_matches_flags() {
        let caps = Capabilities {
            request_changes: false,
            ..Capabilities::all()
        };
        assert!(!caps.supports(FeatureAction::RequestChanges));
        assert!(caps.supports(FeatureAction::Suggestions));
        assert!(!Capabilities::default().supports(FeatureAction::RerunFailed));
    }

    #[test]
    fn explanation_is_kind_and_only_when_unsupported() {
        let caps = Capabilities {
            request_changes: false,
            ..Capabilities::all()
        };
        assert_eq!(
            caps.explain_unsupported(FeatureAction::RequestChanges, "gitlab.work.ca")
                .as_deref(),
            Some("gitlab.work.ca doesn't support request changes. Leave a comment instead (c).")
        );
        assert_eq!(
            caps.explain_unsupported(FeatureAction::Suggestions, "gitlab.work.ca"),
            None
        );
    }

    #[test]
    fn provider_is_object_safe() {
        fn _assert(_: &dyn Provider) {}
    }
}
