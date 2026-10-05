use async_trait::async_trait;
use rb_core::{
    Capabilities, ChangeDetail, ChangeId, ChangeSummary, Check, Comment, Error, Etag, FilePatch,
    ForgeKind, MergeOpts, MergeOutcome, Page, Provider, Result, ReviewDraft, Scope, Thread,
    ThreadId, User, Verdict,
};
use url::Url;

use crate::GithubClient;

/// GitHub behind the `Provider` trait. Listing, detail and write methods land with the
/// issues that need them and report `Unsupported` until then.
#[derive(Debug, Clone)]
pub struct GithubProvider {
    client: GithubClient,
}

impl GithubProvider {
    pub fn new(client: GithubClient) -> Self {
        Self { client }
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
        _scope: &Scope,
        _since: Option<Etag>,
    ) -> Result<Page<ChangeSummary>> {
        not_yet("listing changes")
    }

    async fn change_detail(&self, _id: &ChangeId) -> Result<ChangeDetail> {
        not_yet("change details")
    }

    async fn files(&self, _id: &ChangeId) -> Result<Vec<FilePatch>> {
        not_yet("file patches")
    }

    async fn threads(&self, _id: &ChangeId) -> Result<Vec<Thread>> {
        not_yet("review threads")
    }

    async fn checks(&self, _id: &ChangeId) -> Result<Vec<Check>> {
        not_yet("checks")
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

    fn capabilities(&self) -> Capabilities {
        Capabilities::none()
    }
}
