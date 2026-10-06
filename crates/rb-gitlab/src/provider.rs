use async_trait::async_trait;
use rb_core::{
    Capabilities, ChangeDetail, ChangeId, ChangeSummary, Check, Comment, Error, Etag, FilePatch,
    ForgeKind, MergeOpts, MergeOutcome, Page, Provider, Result, ReviewDraft, Scope, SourceId,
    Thread, ThreadId, User, Verdict,
};
use url::Url;

use crate::{changes, GitlabClient};

/// GitLab behind the `Provider` trait. Sign-in checks, listing and detail work; diffs, threads,
/// pipelines and review writes report `Unsupported` until they land, and capabilities stay empty.
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

    async fn files(&self, _id: &ChangeId) -> Result<Vec<FilePatch>> {
        not_yet("reading diffs")
    }

    async fn threads(&self, _id: &ChangeId) -> Result<Vec<Thread>> {
        not_yet("reading threads")
    }

    async fn checks(&self, _id: &ChangeId) -> Result<Vec<Check>> {
        not_yet("reading pipelines")
    }

    async fn submit_review(
        &self,
        _id: &ChangeId,
        _review: &ReviewDraft,
        _verdict: Verdict,
    ) -> Result<()> {
        not_yet("submitting reviews")
    }

    async fn reply(&self, _thread: &ThreadId, _body: &str) -> Result<Comment> {
        not_yet("replying")
    }

    async fn resolve(&self, _thread: &ThreadId, _resolved: bool) -> Result<()> {
        not_yet("resolving threads")
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
        let host = self.client.host();
        Url::parse(&format!(
            "https://{host}/{}/-/merge_requests/{}",
            id.repo, id.number
        ))
        .unwrap_or_else(|_| Url::parse("https://gitlab.com").expect("static URL"))
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::none()
    }
}
