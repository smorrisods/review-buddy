//! The TUI's live backend: cached rows first, then every source refreshed concurrently.
//! Results go back to the app as messages; nothing here touches app state.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rb_core::{
    Capabilities, ChangeId, ChangeSummary, Error, Etag, ProbeOutcome, Provider, ReviewDraft, Scope,
    Source, SourceId, ThreadId, Timestamp, Verdict,
};
use rb_store::Store;
use tokio::sync::mpsc::UnboundedSender;

use super::probe::{self, probe_cached, Probed};
use super::refresh::{
    classify, jittered_interval, Backoff, Class, Clock, Engine, Jitter, Next, SourceMachine,
};
use super::{Factory, ProviderError};
use crate::app::{ChangeInfo, Msg, Snapshot, SourceFailure, SourceStatus};
use crate::cmd::context::Context;
use crate::load;

pub struct Live {
    factory: Arc<Factory>,
    sources: Vec<Source>,
    cache: Option<Mutex<Store>>,
    engine: Engine,
    /// Sources already probed this session, so each is asked once.
    probed: Mutex<HashSet<SourceId>>,
    /// What each probed source can do, so diffs opened later carry it.
    known: Mutex<HashMap<SourceId, Capabilities>>,
    /// `refresh.interval`; `None` means manual refreshes only.
    pub refresh_interval: Option<Duration>,
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
            engine: Engine::new(concurrency),
            probed: Mutex::default(),
            known: Mutex::default(),
            refresh_interval: None,
            refresh_on_focus: true,
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.engine.clock = clock;
        self
    }

    pub fn with_jitter(mut self, jitter: Arc<dyn Jitter>) -> Self {
        self.engine.jitter = jitter;
        self
    }

    pub fn with_backoff(mut self, backoff: Backoff) -> Self {
        self.engine.backoff = backoff;
        self
    }

    /// What this source is doing right now, as the engine sees it.
    pub fn status(&self, id: &SourceId) -> SourceStatus {
        self.engine.status(id)
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
        live.refresh_interval = ctx.config.refresh.interval.filter(|d| !d.is_zero());
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

    /// An automatic refresh (launch, interval): every source at once, each respecting its own
    /// backoff and rate-limit pause.
    pub fn refresh(self: &Arc<Self>, tx: &UnboundedSender<Msg>) {
        self.start(Ask::Auto, tx);
    }

    /// A refresh you asked for: also cuts a source's backoff wait short.
    pub fn refresh_now(self: &Arc<Self>, tx: &UnboundedSender<Msg>) {
        self.start(Ask::Manual, tx);
    }

    /// A refresh on focus: skipped when one started less than `focus_gap` ago.
    pub fn refresh_on_focus_gap(self: &Arc<Self>, tx: &UnboundedSender<Msg>) {
        if self.engine.mark_started(Some(self.engine.focus_gap)) {
            self.start_unmarked(Ask::Focus, tx);
        } else {
            let _ = tx.send(Msg::RefreshSkipped);
        }
    }

    /// Sends `Msg::RefreshDue` every `interval` (spread a little by jitter) until the app closes.
    pub fn spawn_interval(self: &Arc<Self>, interval: Duration, tx: &UnboundedSender<Msg>) {
        let live = Arc::clone(self);
        let tx = tx.clone();
        tokio::spawn(async move {
            loop {
                let wait = jittered_interval(interval, live.engine.jitter.unit());
                live.engine.clock.sleep(wait).await;
                if tx.send(Msg::RefreshDue).is_err() {
                    break;
                }
            }
        });
    }

    /// The newest time any source's rows were saved, for the offline banner.
    pub fn cached_at(&self) -> Option<Timestamp> {
        let cache = self.cache.as_ref()?;
        let store = cache.lock().unwrap_or_else(|e| e.into_inner());
        self.sources
            .iter()
            .filter_map(|s| store.last_fetched(&s.id).ok().flatten())
            .max()
    }

    fn start(self: &Arc<Self>, how: Ask, tx: &UnboundedSender<Msg>) {
        self.engine.mark_started(None);
        self.start_unmarked(how, tx);
    }

    fn start_unmarked(self: &Arc<Self>, how: Ask, tx: &UnboundedSender<Msg>) {
        for source in &self.sources {
            if self.engine.begin(&source.id) {
                let live = Arc::clone(self);
                let tx = tx.clone();
                let source = source.clone();
                tokio::spawn(async move { live.run_cycle(source, tx).await });
                continue;
            }
            if how == Ask::Manual
                && matches!(self.engine.status(&source.id), SourceStatus::Offline { .. })
            {
                self.engine.kick(&source.id).notify_one();
            }
            let result = match self
                .engine
                .with_machine(&source.id, |m| m.last_failure().cloned())
            {
                Some(failure) => Err(failure),
                None => Ok(self.cached_rows(&source.id)),
            };
            let _ = tx.send(Msg::SourceLoaded {
                source: source.id.clone(),
                result,
                now: self.engine.clock.now(),
            });
        }
    }

    /// One source's refresh: tries, and on a transient failure waits and tries again, until it
    /// works, is refused for good, or the app closes. The first answer is `SourceLoaded`; later
    /// ones are `SourceUpdated`.
    async fn run_cycle(self: Arc<Self>, source: Source, tx: UnboundedSender<Msg>) {
        let engine = &self.engine;
        let mut first = true;
        let reply = |first: &mut bool, result| {
            let source = source.id.clone();
            let now = engine.clock.now();
            let msg = if *first {
                Msg::SourceLoaded {
                    source,
                    result,
                    now,
                }
            } else {
                Msg::SourceUpdated {
                    source,
                    result,
                    now,
                }
            };
            *first = false;
            tx.send(msg).is_ok()
        };
        let status = |status| {
            let _ = tx.send(Msg::SourceStatus {
                source: source.id.clone(),
                status,
            });
        };
        loop {
            if let Some(until) = engine.host_paused_until(&source.host) {
                engine.with_machine(&source.id, |m| m.pause_until(until));
                status(SourceStatus::RateLimited { until });
                let wait = until.0 - engine.clock.now().0;
                let failure = SourceFailure::from_error(
                    &Error::RateLimited {
                        host: source.host.clone(),
                        retry_after_secs: u64::try_from(wait).ok(),
                    },
                    &source.host,
                );
                if !reply(&mut first, Err(failure)) {
                    return;
                }
                engine
                    .clock
                    .sleep(Duration::from_secs(u64::try_from(wait).unwrap_or(1).max(1)))
                    .await;
                continue;
            }
            match self.attempt(&source).await {
                Ok(items) => {
                    engine.with_machine(&source.id, SourceMachine::succeed);
                    status(SourceStatus::Ok);
                    self.probe_once(&source, &tx);
                    reply(&mut first, Ok(items));
                    return;
                }
                Err(failed) => {
                    let next = engine.with_machine(&source.id, |m| {
                        m.fail(
                            failed.class,
                            failed.failure.clone(),
                            engine.clock.now(),
                            &engine.backoff,
                            engine.jitter.unit(),
                        )
                    });
                    if let Next::Pause { until } = next {
                        engine.pause_host(&source.host, until);
                    }
                    status(engine.status(&source.id));
                    if !reply(&mut first, Err(failed.failure)) {
                        return;
                    }
                    match next {
                        Next::Stop => return,
                        Next::Retry(delay) => {
                            let kicked = engine.kick(&source.id);
                            tokio::select! {
                                () = engine.clock.sleep(delay) => {}
                                () = kicked.notified() => {}
                            }
                        }
                        Next::Pause { until } => {
                            let wait = until.0 - engine.clock.now().0;
                            engine
                                .clock
                                .sleep(Duration::from_secs(u64::try_from(wait).unwrap_or(1).max(1)))
                                .await;
                        }
                    }
                }
            }
        }
    }

    /// Probes the source the first time a refresh reaches it this session. The answer is the
    /// saved one when it's under a day old, so most launches send nothing.
    fn probe_once(self: &Arc<Self>, source: &Source, tx: &UnboundedSender<Msg>) {
        let first = self
            .probed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(source.id.clone());
        if !first {
            return;
        }
        let live = Arc::clone(self);
        let tx = tx.clone();
        let source = source.clone();
        tokio::spawn(async move {
            if let Some(probed) = live.probe_source(&source, false).await {
                live.remember(&source.id, &probed.outcome);
                let _ = tx.send(Msg::Probed {
                    source: source.id,
                    outcome: Box::new(probed.outcome),
                    at: probed.at,
                });
            }
        });
    }

    /// Asks (or recalls) what one source can do. A failure is quiet: the refresh reports sign-in
    /// trouble itself, and the source keeps showing every action meanwhile.
    pub async fn probe_source(&self, source: &Source, force: bool) -> Option<Probed> {
        let _permit = self.engine.semaphore(&source.host).acquire_owned().await;
        let provider = self.provider(&source.id, &source.host).await.ok()?;
        probe_cached(
            provider.as_ref(),
            self.cache.as_ref(),
            &source.host,
            self.engine.clock.now(),
            force,
        )
        .await
        .ok()
    }

    /// Forgets saved probes for every source, for when settings or sign-in changed.
    pub fn forget_probes(&self) {
        if let Some(cache) = &self.cache {
            let mut store = cache.lock().unwrap_or_else(|e| e.into_inner());
            for source in &self.sources {
                probe::forget(&mut store, source.kind, &source.host);
            }
        }
    }

    fn remember(&self, id: &SourceId, outcome: &ProbeOutcome) {
        self.known
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), outcome.capabilities);
    }

    fn cached_rows(&self, id: &SourceId) -> Vec<ChangeSummary> {
        self.cache.as_ref().map_or_else(Vec::new, |cache| {
            let store = cache.lock().unwrap_or_else(|e| e.into_inner());
            store.list_summaries(id).unwrap_or_default()
        })
    }

    /// One request for a source's changes, sent conditionally when rows are cached under a
    /// saved ETag. A not-modified answer keeps the cached rows.
    async fn attempt(&self, source: &Source) -> Result<Vec<ChangeSummary>, Failed> {
        let _permit = self.engine.semaphore(&source.host).acquire_owned().await;
        let (provider, host) = (
            self.provider_raw(&source.id, &source.host).await?,
            &source.host,
        );
        let key = etag_key(&source.scope);
        let (since, cached) = self.conditional(&source.id, &key);
        let page = provider
            .list_changes(&source.scope, since)
            .await
            .map_err(|e| Failed {
                class: classify(&e),
                failure: SourceFailure::from_error(&e, host),
            })?;
        let now = self.engine.clock.now();
        let Some(cache) = &self.cache else {
            return Ok(page.items);
        };
        let mut store = cache.lock().unwrap_or_else(|e| e.into_inner());
        if page.not_modified {
            let _ = store.put_summaries(&source.id, &cached, now);
            return Ok(cached);
        }
        let _ = store.replace_summaries(&source.id, &page.items, now);
        match &page.etag {
            Some(etag) => {
                let _ = store.set_etag(&source.id, &key, etag, now);
            }
            None => {
                let _ = store.clear_etag(&source.id, &key);
            }
        }
        Ok(page.items)
    }

    /// The saved ETag and rows, when there are rows to fall back on.
    fn conditional(&self, id: &SourceId, key: &str) -> (Option<Etag>, Vec<ChangeSummary>) {
        let Some(cache) = &self.cache else {
            return (None, Vec::new());
        };
        let store = cache.lock().unwrap_or_else(|e| e.into_inner());
        let cached = store.list_summaries(id).unwrap_or_default();
        if cached.is_empty() {
            return (None, cached);
        }
        (store.etag(id, key).ok().flatten(), cached)
    }

    async fn provider_raw(
        &self,
        id: &SourceId,
        host: &str,
    ) -> Result<Arc<dyn rb_core::Provider>, Failed> {
        let factory = Arc::clone(&self.factory);
        let wanted = id.clone();
        let built = tokio::task::spawn_blocking(move || factory.provider(&wanted))
            .await
            .map_err(|_| Failed {
                class: Class::Other,
                failure: SourceFailure::unavailable(
                    "Couldn't start the sign-in check.",
                    "Press r to try again.",
                ),
            })?;
        built.map_err(|e| Failed {
            class: match &e {
                ProviderError::Auth(_) | ProviderError::GitlabAuth(_) => Class::Auth,
                ProviderError::Client(error) => classify(error),
                ProviderError::UnknownSource(_) => Class::Other,
            },
            failure: e.failure(host),
        })
    }

    async fn provider(
        &self,
        id: &SourceId,
        host: &str,
    ) -> Result<Arc<dyn rb_core::Provider>, SourceFailure> {
        self.provider_raw(id, host).await.map_err(|f| f.failure)
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
        let _permit = self.engine.semaphore(&host).acquire_owned().await;
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
        let _permit = self.engine.semaphore(&host).acquire_owned().await;
        let provider = self.provider(&id.source_id, &host).await?;
        let mut data = load::fetch_diff(provider.as_ref(), id, ReviewDraft::default())
            .await
            .map_err(|e| SourceFailure::from_error(&e, &host))?;
        if let Some(caps) = self
            .known
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id.source_id)
        {
            data.caps = *caps;
        }
        Ok(data)
    }

    /// Sends a review (or one standalone comment) to the change's forge. Only called after the
    /// confirmation step; the app's `submitting` flag keeps a second one from starting.
    pub fn submit_review(
        self: &Arc<Self>,
        id: ChangeId,
        draft: ReviewDraft,
        verdict: Verdict,
        tx: &UnboundedSender<Msg>,
    ) {
        let live = Arc::clone(self);
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = match live.write_provider(&id).await {
                Ok(provider) => provider
                    .submit_review(&id, &draft, verdict)
                    .await
                    .map_err(|e| e.to_string()),
                Err(message) => Err(message),
            };
            let _ = tx.send(Msg::ReviewSubmitted {
                id,
                verdict,
                result,
                demo: false,
            });
        });
    }

    /// Replies on a thread of `id`.
    pub fn reply(
        self: &Arc<Self>,
        id: ChangeId,
        thread: ThreadId,
        body: String,
        tx: &UnboundedSender<Msg>,
    ) {
        let live = Arc::clone(self);
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = match live.write_provider(&id).await {
                Ok(provider) => provider
                    .reply(&thread, &body)
                    .await
                    .map_err(|e| e.to_string()),
                Err(message) => Err(message),
            };
            let _ = tx.send(Msg::ReplyPosted {
                id,
                thread,
                result,
                demo: false,
            });
        });
    }

    /// The provider for a write, or the calm sign-in message when there isn't one.
    async fn write_provider(&self, id: &ChangeId) -> Result<Arc<dyn Provider>, String> {
        let host = self.host_of(&id.source_id);
        self.provider(&id.source_id, &host).await.map_err(describe)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Auto,
    Manual,
    Focus,
}

struct Failed {
    class: Class,
    failure: SourceFailure,
}

/// Where a source's ETag is kept: one per scope, so changing the scope never reuses a stale one.
fn etag_key(scope: &Scope) -> String {
    format!("changes:{scope:?}")
}

fn describe(failure: SourceFailure) -> String {
    format!("{} {}", failure.summary, failure.next_step)
}
