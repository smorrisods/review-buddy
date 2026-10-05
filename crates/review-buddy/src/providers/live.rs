//! The TUI's live backend: cached rows first, then every source refreshed concurrently.
//! Results go back to the app as messages; nothing here touches app state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rb_core::{ChangeId, ReviewDraft, Source, SourceId};
use rb_store::Store;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Semaphore;

use super::Factory;
use crate::app::{ChangeInfo, Msg, Snapshot, SourceFailure};
use crate::cmd::context::Context;
use crate::load;

pub struct Live {
    factory: Arc<Factory>,
    sources: Vec<Source>,
    cache: Option<Mutex<Store>>,
    limit: Arc<Semaphore>,
    pub refresh_on_focus: bool,
}

impl std::fmt::Debug for Live {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Live")
            .field("sources", &self.sources.len())
            .finish_non_exhaustive()
    }
}

impl Live {
    pub fn new(
        factory: Arc<Factory>,
        sources: Vec<Source>,
        cache: Option<Store>,
        concurrency: usize,
    ) -> Self {
        Self {
            factory,
            sources,
            cache: cache.map(Mutex::new),
            limit: Arc::new(Semaphore::new(concurrency.max(1))),
            refresh_on_focus: true,
        }
    }

    /// Built from the same sources, factory and cache path the commands use.
    pub fn from_context(ctx: &Context) -> Result<Self, crate::cmd::error::CmdError> {
        let sources = ctx.sources()?;
        let mut live = Self::new(
            ctx.factory(),
            sources,
            ctx.cache().ok(),
            usize::from(ctx.config.refresh.max_concurrency_per_host),
        );
        live.refresh_on_focus = ctx.config.refresh.on_focus;
        Ok(live)
    }

    /// Sources and whatever the cache holds for them, ready to paint before the network answers.
    pub fn cached_snapshot(&self) -> Snapshot {
        let mut changes = Vec::new();
        if let Some(cache) = &self.cache {
            let store = cache.lock().unwrap_or_else(|e| e.into_inner());
            for source in &self.sources {
                changes.extend(store.list_summaries(&source.id).unwrap_or_default());
            }
        }
        Snapshot {
            label: "live".to_string(),
            sources: self.sources.clone(),
            changes,
            now: load::now(),
            details: HashMap::new(),
        }
    }

    /// Refreshes every source at once, at most `concurrency` requests in flight.
    pub fn refresh(self: &Arc<Self>, tx: &UnboundedSender<Msg>) {
        for source in &self.sources {
            let live = Arc::clone(self);
            let tx = tx.clone();
            let source = source.clone();
            tokio::spawn(async move {
                let result = live.refresh_one(&source).await;
                let _ = tx.send(Msg::SourceLoaded {
                    source: source.id,
                    result,
                    now: load::now(),
                });
            });
        }
    }

    async fn refresh_one(
        &self,
        source: &Source,
    ) -> Result<Vec<rb_core::ChangeSummary>, SourceFailure> {
        let _permit = self.limit.acquire().await;
        let provider = self.provider(&source.id, &source.host).await?;
        let page = provider
            .list_changes(&source.scope, None)
            .await
            .map_err(|e| SourceFailure::from_error(&e, &source.host))?;
        if let Some(cache) = &self.cache {
            let mut store = cache.lock().unwrap_or_else(|e| e.into_inner());
            let _ = store.replace_summaries(&source.id, &page.items, load::now());
        }
        Ok(page.items)
    }

    async fn provider(
        &self,
        id: &SourceId,
        host: &str,
    ) -> Result<Arc<dyn rb_core::Provider>, SourceFailure> {
        let factory = Arc::clone(&self.factory);
        let wanted = id.clone();
        tokio::task::spawn_blocking(move || factory.provider(&wanted))
            .await
            .map_err(|_| {
                SourceFailure::unavailable(
                    "Couldn't start the sign-in check.",
                    "Press r to try again.",
                )
            })?
            .map_err(|e| e.failure(host))
    }

    fn host_of(&self, id: &SourceId) -> String {
        self.sources
            .iter()
            .find(|s| &s.id == id)
            .map_or_else(String::new, |s| s.host.clone())
    }

    pub fn load_info(self: &Arc<Self>, id: ChangeId, tx: &UnboundedSender<Msg>) {
        let live = Arc::clone(self);
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = live.info(&id).await.map(Box::new).map_err(describe);
            let _ = tx.send(Msg::InfoLoaded { id, result });
        });
    }

    async fn info(&self, id: &ChangeId) -> Result<ChangeInfo, SourceFailure> {
        let host = self.host_of(&id.source_id);
        let _permit = self.limit.acquire().await;
        let provider = self.provider(&id.source_id, &host).await?;
        load::fetch_info(provider.as_ref(), id)
            .await
            .map_err(|e| SourceFailure::from_error(&e, &host))
    }

    pub fn load_diff(self: &Arc<Self>, id: ChangeId, tx: &UnboundedSender<Msg>) {
        let live = Arc::clone(self);
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = live.diff(&id).await.map(Box::new).map_err(describe);
            let _ = tx.send(Msg::DiffLoaded { id, result });
        });
    }

    async fn diff(&self, id: &ChangeId) -> Result<crate::app::DiffData, SourceFailure> {
        let host = self.host_of(&id.source_id);
        let _permit = self.limit.acquire().await;
        let provider = self.provider(&id.source_id, &host).await?;
        load::fetch_diff(provider.as_ref(), id, ReviewDraft::default())
            .await
            .map_err(|e| SourceFailure::from_error(&e, &host))
    }
}

fn describe(failure: SourceFailure) -> String {
    format!("{} {}", failure.summary, failure.next_step)
}
