use async_trait::async_trait;
use rb_core::{
    Capabilities, ChangeDetail, ChangeId, ChangeSummary, Check, Comment, Error, Etag, FilePatch,
    ForgeKind, MergeOpts, MergeOutcome, Page, ProbeOutcome, Provider, Result, ReviewDraft, Scope,
    SourceId, Thread, ThreadId, User, Verdict,
};
use url::Url;

use crate::{changes, checks, files, review, threads, GitlabClient};

/// GitLab behind the `Provider` trait. Listing, detail, files, threads, pipeline jobs and review
/// writes (comment, approve, reply, resolve) are implemented; request changes, merge and re-run
/// report `Unsupported`.
#[derive(Debug, Clone)]
pub struct GitlabProvider {
    client: GitlabClient,
    source_id: SourceId,
}

impl GitlabProvider {
    pub fn new(client: GitlabClient) -> Self {
        let source_id = SourceId::new(client.host());
        Self { client, source_id }
    }

    /// The source these changes belong to; defaults to the host name.
    pub fn with_source_id(mut self, source_id: SourceId) -> Self {
        self.source_id = source_id;
        self
    }

    pub fn client(&self) -> &GitlabClient {
        &self.client
    }
}

fn not_yet<T>(what: &str) -> Result<T> {
    Err(Error::Unsupported(format!("{what} on GitLab yet")))
}

#[async_trait]
impl Provider for GitlabProvider {
    fn kind(&self) -> ForgeKind {
        ForgeKind::GitLab
    }

    async fn whoami(&self) -> Result<User> {
        self.client.whoami().await
    }

    async fn list_changes(
        &self,
        scope: &Scope,
        since: Option<Etag>,
    ) -> Result<Page<ChangeSummary>> {
        changes::list_changes(&self.client, &self.source_id, scope, since).await
    }

    async fn change_detail(&self, id: &ChangeId) -> Result<ChangeDetail> {
        changes::change_detail(&self.client, &self.source_id, id, self.web_url(id)).await
    }

    async fn files(&self, id: &ChangeId) -> Result<Vec<FilePatch>> {
        files::files(&self.client, id).await
    }

    async fn threads(&self, id: &ChangeId) -> Result<Vec<Thread>> {
        threads::threads(&self.client, id).await
    }

    async fn checks(&self, id: &ChangeId) -> Result<Vec<Check>> {
        checks::checks(&self.client, id).await
    }

    async fn submit_review(
        &self,
        id: &ChangeId,
        review: &ReviewDraft,
        verdict: Verdict,
    ) -> Result<()> {
        review::submit_review(&self.client, id, review, verdict).await
    }

    async fn reply(&self, thread: &ThreadId, body: &str) -> Result<Comment> {
        review::reply(&self.client, thread, body).await
    }

    async fn resolve(&self, thread: &ThreadId, resolved: bool) -> Result<()> {
        review::resolve(&self.client, thread, resolved).await
    }

    async fn merge(&self, _id: &ChangeId, _opts: &MergeOpts) -> Result<MergeOutcome> {
        not_yet("merging")
    }

    async fn rerun_failed(&self, _id: &ChangeId) -> Result<()> {
        not_yet("re-running pipelines")
    }

    fn checkout_refspec(&self, id: &ChangeId) -> String {
        format!("merge-requests/{}/head", id.number)
    }

    fn web_url(&self, id: &ChangeId) -> Url {
        let base = self.client.web_base();
        rb_core::http::join_web(
            &base,
            &format!("{}/-/merge_requests/{}", id.repo, id.number),
        )
        .unwrap_or(base)
    }

    async fn probe(&self) -> Result<ProbeOutcome> {
        crate::probe::probe(&self.client, self.capabilities()).await
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            range_comments: true,
            resolve_threads: true,
            ..Capabilities::none()
        }
    }
}
