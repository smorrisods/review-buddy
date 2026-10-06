use async_trait::async_trait;
use rb_core::{
    Capabilities, ChangeDetail, ChangeId, ChangeSummary, Check, Comment, Error, Etag, FilePatch,
    ForgeKind, MergeOpts, MergeOutcome, Page, ProbeOutcome, Provider, Result, ReviewDraft, Scope,
    SourceId, Thread, ThreadId, User, Verdict,
};
use url::Url;

use crate::{changes, checks, files, review, threads, GithubClient};

/// GitHub behind the `Provider` trait. Listing, detail, files, threads, checks and review writes are implemented; merge and re-run
/// report `Unsupported` until v0.3.
#[derive(Debug, Clone)]
pub struct GithubProvider {
    client: GithubClient,
    source_id: SourceId,
}

impl GithubProvider {
    pub fn new(client: GithubClient) -> Self {
        let source_id = SourceId::new(client.host());
        Self { client, source_id }
    }

    /// The source these changes belong to; defaults to the host name.
    pub fn with_source_id(mut self, source_id: SourceId) -> Self {
        self.source_id = source_id;
        self
    }

    pub fn client(&self) -> &GithubClient {
        &self.client
    }
}

fn not_yet<T>(what: &str) -> Result<T> {
    Err(Error::Unsupported(format!("{what} on GitHub yet")))
}

#[async_trait]
impl Provider for GithubProvider {
    fn kind(&self) -> ForgeKind {
        ForgeKind::GitHub
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
        not_yet("re-running checks")
    }

    fn checkout_refspec(&self, id: &ChangeId) -> String {
        format!("pull/{}/head", id.number)
    }

    fn web_url(&self, id: &ChangeId) -> Url {
        let host = self.client.host();
        Url::parse(&format!("https://{host}/{}/pull/{}", id.repo, id.number))
            .unwrap_or_else(|_| Url::parse("https://github.com").expect("static URL"))
    }

    async fn probe(&self) -> Result<ProbeOutcome> {
        crate::probe::probe(&self.client).await
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::all()
    }
}
