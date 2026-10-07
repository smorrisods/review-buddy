//! The async event loop: terminal events and timers in, `Msg`s through `update`, `Cmd`s out.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::time::{interval, sleep_until, Instant, MissedTickBehavior};

use crate::app::{update, App, AppConfig, Cmd, Msg, Notice, NoticeKind};
use crate::ui;

mod effects;
mod terminal;

pub use effects::Platform;
pub use terminal::{restore, TerminalGuard};

/// Redraws are capped at about 30 frames per second.
pub const MIN_FRAME: Duration = Duration::from_millis(33);
const TICK: Duration = Duration::from_millis(250);

/// Whether a redraw should happen now: only when something changed, and not
/// sooner than [`MIN_FRAME`] after the last one.
pub fn frame_due(dirty: bool, since_last_draw: Duration) -> bool {
    dirty && since_last_draw >= MIN_FRAME
}

/// Translates a terminal event into a message, if the app cares about it.
pub fn msg_from_event(event: Event) -> Option<Msg> {
    match event {
        Event::Key(key) => Some(Msg::Key(key)),
        Event::Mouse(mouse) => Some(Msg::Mouse(mouse)),
        Event::Resize(w, h) => Some(Msg::Resize(w, h)),
        Event::FocusGained => Some(Msg::FocusGained),
        Event::FocusLost => Some(Msg::FocusLost),
        Event::Paste(text) => Some(Msg::Paste(text)),
    }
}

/// What answers [`Cmd::LoadChanges`].
#[derive(Debug, Clone, Default)]
pub enum Backend {
    #[default]
    None,
    #[cfg(feature = "demo")]
    Demo(crate::demo::DemoWorld),
    #[cfg(feature = "live")]
    Live(std::sync::Arc<crate::providers::Live>),
}

/// The parts of `config.toml` the interface applies at launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub queue: crate::app::queue::QueueSettings,
    pub theme: String,
    pub depth: Option<rb_theme::ColourDepth>,
    pub tab_width: u8,
    pub confirm_post_now: bool,
    pub reduced_motion: bool,
    /// `ui.images`, with `REVIEW_BUDDY_IMAGES` already applied when it came from the config.
    pub images: crate::config::Images,
    pub background: rb_theme::BackgroundMode,
    pub per_theme_background: std::collections::BTreeMap<String, rb_theme::BackgroundMode>,
    pub layout: crate::ui::layout::Options,
    /// The remembered layout file and what it held at launch. `None` when remembering is off.
    pub session: Option<(std::path::PathBuf, Option<crate::session::Session>)>,
    /// Where `w` in the Show control writes `hide_repos`; `None` when there's nowhere to write.
    pub write_target: Option<std::path::PathBuf>,
    /// The folder drafts are saved in and what it held at launch. `None` keeps them in memory.
    pub drafts: Option<(std::path::PathBuf, Vec<crate::drafts::StoredDraft>)>,
}

fn background_mode(choice: crate::config::Background) -> rb_theme::BackgroundMode {
    match choice {
        crate::config::Background::Theme => rb_theme::BackgroundMode::Theme,
        crate::config::Background::Yes => rb_theme::BackgroundMode::Yes,
        crate::config::Background::No => rb_theme::BackgroundMode::No,
    }
}

/// `REVIEW_BUDDY_BACKGROUND` for this run. Values that aren't `theme`, `yes` or `no` are ignored.
pub fn background_from_env() -> Option<rb_theme::BackgroundMode> {
    std::env::var("REVIEW_BUDDY_BACKGROUND").ok()?.parse().ok()
}

impl Settings {
    /// Lets the Show control save the project choice to the config's write target.
    pub fn with_write_target(mut self, path: std::path::PathBuf) -> Self {
        self.write_target = Some(path);
        self
    }

    /// Saves review drafts in `dir`, starting from what it already holds.
    pub fn with_drafts(mut self, dir: std::path::PathBuf) -> Self {
        let loaded = crate::drafts::load_all(&dir);
        self.drafts = Some((dir, loaded));
        self
    }

    /// Remembers the layout in `path`, starting from `loaded` (what the file held, if it was
    /// there). Precedence at launch: a key pressed this session, `REVIEW_BUDDY_*` in `env`,
    /// the remembered file, then the config this was built from.
    pub fn with_session(
        mut self,
        path: std::path::PathBuf,
        loaded: Option<crate::session::Session>,
        env: &dyn rb_paths::Env,
    ) -> Self {
        if let Some(session) = loaded {
            let l = session.layout;
            let env_position = env
                .var("REVIEW_BUDDY_DETAIL_POSITION")
                .is_some_and(|v| crate::config::parse_position(&v).is_some());
            let env_background = env
                .var("REVIEW_BUDDY_BACKGROUND")
                .is_some_and(|v| v.parse::<rb_theme::BackgroundMode>().is_ok());
            let o = &mut self.layout;
            if !env_position {
                o.position = l.detail_position.unwrap_or(o.position);
            }
            o.sources = l.sources.unwrap_or(o.sources);
            o.detail = l.detail.unwrap_or(o.detail);
            o.split.queue_width = l.queue_width.or(o.split.queue_width);
            o.split.queue_height = l.queue_height.or(o.split.queue_height);
            o.split.sources_width = l.sources_width.or(o.split.sources_width);
            if let (Some(mode), false) = (l.background, env_background) {
                self.background = mode;
                self.per_theme_background.clear();
            }
        }
        self.session = Some((path, loaded));
        self
    }

    pub fn from_config(config: &crate::config::Config) -> Self {
        use std::str::FromStr;
        Self {
            queue: crate::app::queue::queue_settings(config),
            theme: config.ui.theme.clone(),
            depth: match config.ui.colour_depth {
                crate::config::ColourDepth::Auto => None,
                other => rb_theme::ColourDepth::from_str(other.as_str()).ok(),
            },
            tab_width: config.diff.tab_width.clamp(1, 16),
            confirm_post_now: config.review.confirm_post_now,
            reduced_motion: config.ui.reduced_motion,
            images: config.ui.images,
            background: background_mode(config.ui.background),
            per_theme_background: config
                .ui
                .theme_background
                .iter()
                .map(|(id, mode)| (id.clone(), background_mode(*mode)))
                .collect(),
            layout: crate::ui::layout::Options {
                sources: config.ui.sources,
                detail: config.ui.detail,
                position: config.ui.detail_position,
                split: crate::ui::layout::Split {
                    queue_width: config.ui.queue_width,
                    queue_height: config.ui.queue_height,
                    sources_width: None,
                },
            },
            session: None,
            write_target: None,
            drafts: None,
        }
    }

    /// Applies the settings to a freshly built app config and app.
    pub fn app_config(&self, mut base: AppConfig) -> AppConfig {
        base.theme_id.clone_from(&self.theme);
        if let Some(depth) = self.depth {
            base.depth = depth;
        }
        base
    }

    pub fn apply(&self, app: &mut App) {
        app.confirm_post_now = self.confirm_post_now;
        app.tab_width = self.tab_width;
        app.reduced_motion = self.reduced_motion;
        app.layout = self.layout;
        app.background.global = self.background;
        app.background
            .per_theme
            .clone_from(&self.per_theme_background);
        app.rebuild_palette();
        app.session = self.session.as_ref().map(|(path, loaded)| {
            crate::session::Tracker::new(
                path.clone(),
                loaded.unwrap_or_default(),
                app.session_snapshot(),
            )
        });
        app.drafts = match &self.drafts {
            Some((dir, loaded)) => {
                crate::app::drafts::Drafts::new(Some(dir.clone()), loaded.clone())
            }
            None => crate::app::drafts::Drafts::default(),
        };
        app.state.queue_settings = self.queue.clone();
        app.project_save.clone_from(&self.write_target);
    }
}

/// How the interface should start.
#[derive(Debug, Default)]
pub struct RunOptions {
    /// Config for non-demo runs; demo mode never reads it.
    pub settings: Option<Settings>,
    #[cfg(feature = "demo")]
    pub demo: Option<crate::demo::Demo>,
    /// Leave mouse capture off (`ui.mouse = false`). Demo mode never reads the config, so it captures.
    pub no_mouse: bool,
    /// Start on this change's diff instead of the dashboard.
    pub open: Option<rb_core::ChangeId>,
    #[cfg(feature = "live")]
    pub live: Option<std::sync::Arc<crate::providers::Live>>,
    /// What Settings → Sources needs. Without it, Settings opens but can't reach the config.
    pub settings_services: Option<std::sync::Arc<crate::settings::Services>>,
    /// Open first run on this flow, with what its effects need.
    pub setup: Option<(crate::setup::Flow, std::sync::Arc<crate::setup::Services>)>,
    /// Rebuilds the settings and live backend from the config first run just wrote.
    #[cfg(feature = "live")]
    pub reload: Option<Reload>,
}

/// Rebuilds the settings and live backend from the config on disk.
#[cfg(feature = "live")]
#[derive(Clone)]
pub struct Reload(
    pub  std::sync::Arc<
        dyn Fn() -> Result<(Settings, std::sync::Arc<crate::providers::Live>)> + Send + Sync,
    >,
);

#[cfg(feature = "live")]
impl std::fmt::Debug for Reload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reload")
    }
}

impl Backend {
    /// Demo mode never opens anything outside the terminal.
    fn is_demo(&self) -> bool {
        match self {
            Backend::None => false,
            #[cfg(feature = "demo")]
            Backend::Demo(_) => true,
            #[cfg(feature = "live")]
            Backend::Live(_) => false,
        }
    }
}

impl RunOptions {
    fn backend(&self) -> Backend {
        #[cfg(feature = "demo")]
        if let Some(demo) = &self.demo {
            return Backend::Demo(demo.world.clone());
        }
        #[cfg(feature = "live")]
        if let Some(live) = &self.live {
            return Backend::Live(std::sync::Arc::clone(live));
        }
        Backend::None
    }
}

/// Writes the project choice and says what happened, in a sentence that points at the next step.
fn save_hidden_projects(target: &std::path::Path, sources: &[(String, Vec<String>)]) -> Notice {
    match crate::settings::edit::save_hide_repos(target, sources) {
        Ok((saved, elsewhere)) if elsewhere.is_empty() => Notice::new(
            NoticeKind::Success,
            format!(
                "Saved your project choice to {}{}.",
                target.display(),
                if saved.is_empty() { " (nothing to change)" } else { "" }
            ),
        ),
        Ok((_, elsewhere)) => Notice::new(
            NoticeKind::Warning,
            format!(
                "{} isn't defined in {}, so its projects weren't saved. Add hide_repos to it where it is defined.",
                elsewhere.join(", "),
                target.display()
            ),
        ),
        Err(err) => Notice::new(
            NoticeKind::Warning,
            format!("Couldn't save the project choice: {err}"),
        ),
    }
}

/// Writes the pane sizes and says what happened.
fn save_layout_sizes(
    target: &std::path::Path,
    queue_width: Option<crate::ui::layout::Size>,
    queue_height: Option<crate::ui::layout::Size>,
) -> Notice {
    match crate::settings::edit::save_layout_sizes(target, queue_width, queue_height) {
        Ok(()) => Notice::new(
            NoticeKind::Success,
            format!("Saved your pane sizes to {}.", target.display()),
        ),
        Err(err) => Notice::new(
            NoticeKind::Warning,
            format!("Couldn't save the pane sizes: {err}"),
        ),
    }
}

/// Runs one effect. Results come back as messages on `tx`.
pub fn execute(cmd: Cmd, tx: &UnboundedSender<Msg>, backend: &Backend, platform: &Platform) {
    match cmd {
        Cmd::OpenUrl(url) if backend.is_demo() => {
            let text = format!("Would open {url} (demo)");
            let _ = tx.send(Msg::Status(Notice::new(NoticeKind::Info, text)));
        }
        Cmd::OpenUrl(url) => {
            let platform = platform.clone();
            let tx = tx.clone();
            tokio::task::spawn_blocking(move || {
                let _ = tx.send(Msg::Status(platform.open(&url)));
            });
        }
        Cmd::Copy(text) => {
            let _ = tx.send(Msg::Status(platform.copy(&text)));
        }
        Cmd::FetchImage { source, url } => match backend {
            Backend::None => {
                drop(source);
                let result = Err(crate::images::Failure::NoSource);
                let _ = tx.send(Msg::ImageLoaded { url, result });
            }
            #[cfg(feature = "demo")]
            Backend::Demo(_) => {
                let tx = tx.clone();
                drop(source);
                tokio::task::spawn_blocking(move || {
                    let result = crate::demo::images::load(&url);
                    let _ = tx.send(Msg::ImageLoaded { url, result });
                });
            }
            #[cfg(feature = "live")]
            Backend::Live(live) => live.fetch_image(source, url, tx),
        },
        Cmd::LoadChanges | Cmd::LoadChangesNow | Cmd::LoadChangesOnFocus => match backend {
            Backend::None => {}
            #[cfg(feature = "demo")]
            Backend::Demo(world) => {
                let world = world.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let msg = match world.snapshot().await {
                        Ok(snapshot) => {
                            let probes: Vec<Msg> = snapshot
                                .sources
                                .iter()
                                .map(|source| {
                                    use rb_core::Provider as _;
                                    let caps = world.provider(source.kind).capabilities();
                                    Msg::Probed {
                                        source: source.id.clone(),
                                        outcome: Box::new(rb_core::ProbeOutcome {
                                            complete: true,
                                            ..rb_core::ProbeOutcome::new(caps)
                                        }),
                                        at: snapshot.now,
                                    }
                                })
                                .collect();
                            let _ = tx.send(Msg::Loaded(Box::new(snapshot)));
                            for probe in probes {
                                let _ = tx.send(probe);
                            }
                            return;
                        }
                        Err(err) => Msg::Notify(crate::app::Notice::new(
                            crate::app::NoticeKind::Warning,
                            format!("The demo data didn't load: {err}."),
                        )),
                    };
                    let _ = tx.send(msg);
                });
            }
            #[cfg(feature = "live")]
            Backend::Live(live) => match cmd {
                Cmd::LoadChangesNow => live.refresh_now(tx),
                Cmd::LoadChangesOnFocus => live.refresh_on_focus_gap(tx),
                _ => live.refresh(tx),
            },
        },
        Cmd::LoadInfo(id) => match backend {
            Backend::None => drop(id),
            #[cfg(feature = "demo")]
            Backend::Demo(world) => {
                let provider = world.provider(id.kind);
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = crate::load::fetch_info(&provider, &id)
                        .await
                        .map(Box::new)
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::InfoLoaded { id, result });
                });
            }
            #[cfg(feature = "live")]
            Backend::Live(live) => live.load_info(id, tx),
        },
        Cmd::LoadDiff(id) => match backend {
            Backend::None => {
                let _ = tx.send(Msg::DiffLoaded {
                    id,
                    result: Err("No source is connected. Add one, then try again.".to_string()),
                });
            }
            #[cfg(feature = "demo")]
            Backend::Demo(world) => {
                let world = world.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = world
                        .diff_data(&id)
                        .await
                        .map(Box::new)
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::DiffLoaded { id, result });
                });
            }
            #[cfg(feature = "live")]
            Backend::Live(live) => live.load_diff(id, tx),
        },
        Cmd::SubmitReview { id, draft, verdict } => match backend {
            Backend::None => {
                let _ = &draft;
                let _ = tx.send(Msg::ReviewSubmitted {
                    id,
                    verdict,
                    result: Err("No source is connected".to_string()),
                    demo: false,
                });
            }
            #[cfg(feature = "demo")]
            Backend::Demo(world) => {
                use rb_core::Provider;
                let provider = world.provider(id.kind);
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = provider
                        .submit_review(&id, &draft, verdict)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::ReviewSubmitted {
                        id,
                        verdict,
                        result,
                        demo: true,
                    });
                });
            }
            #[cfg(feature = "live")]
            Backend::Live(live) => live.submit_review(id, draft, verdict, tx),
        },
        Cmd::Reply { id, thread, body } => match backend {
            Backend::None => {
                let _ = &body;
                let _ = tx.send(Msg::ReplyPosted {
                    id,
                    thread,
                    result: Err("No source is connected".to_string()),
                    demo: false,
                });
            }
            #[cfg(feature = "demo")]
            Backend::Demo(world) => {
                use rb_core::Provider;
                let provider = world.provider(id.kind);
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = provider
                        .reply(&thread, &body)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::ReplyPosted {
                        id,
                        thread,
                        result,
                        demo: true,
                    });
                });
            }
            #[cfg(feature = "live")]
            Backend::Live(live) => live.reply(id, thread, body, tx),
        },
        Cmd::EditComment {
            id,
            thread,
            comment,
            body,
        } => match backend {
            Backend::None => {
                let _ = tx.send(Msg::CommentEdited {
                    id,
                    thread,
                    comment,
                    body,
                    result: Err("No source is connected".to_string()),
                    demo: false,
                });
            }
            #[cfg(feature = "demo")]
            Backend::Demo(world) => {
                use rb_core::Provider;
                let provider = world.provider(id.kind);
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = provider
                        .update_comment(&thread, &comment, &body)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::CommentEdited {
                        id,
                        thread,
                        comment,
                        body,
                        result,
                        demo: true,
                    });
                });
            }
            #[cfg(feature = "live")]
            Backend::Live(live) => live.edit_comment(id, thread, comment, body, tx),
        },
        Cmd::DeleteComment {
            id,
            thread,
            comment,
        } => match backend {
            Backend::None => {
                let _ = tx.send(Msg::CommentDeleted {
                    id,
                    thread,
                    comment,
                    result: Err("No source is connected".to_string()),
                    demo: false,
                });
            }
            #[cfg(feature = "demo")]
            Backend::Demo(world) => {
                use rb_core::Provider;
                let provider = world.provider(id.kind);
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = provider
                        .delete_comment(&thread, &comment)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::CommentDeleted {
                        id,
                        thread,
                        comment,
                        result,
                        demo: true,
                    });
                });
            }
            #[cfg(feature = "live")]
            Backend::Live(live) => live.delete_comment(id, thread, comment, tx),
        },
        Cmd::SaveDrafts { dir, writes } => {
            let now = crate::load::now().0;
            for (id, draft) in writes {
                let _ = match draft {
                    Some(mut draft) => {
                        draft.written_at = now;
                        crate::drafts::save(&dir, &draft)
                    }
                    None => crate::drafts::remove(&dir, &id),
                };
            }
        }
        Cmd::SaveHiddenProjects { target, sources } => {
            let tx = tx.clone();
            tokio::task::spawn_blocking(move || {
                let _ = tx.send(Msg::Notify(save_hidden_projects(&target, &sources)));
            });
        }
        Cmd::SaveLayoutSizes {
            target,
            queue_width,
            queue_height,
        } => {
            let tx = tx.clone();
            tokio::task::spawn_blocking(move || {
                let _ = tx.send(Msg::Notify(save_layout_sizes(
                    &target,
                    queue_width,
                    queue_height,
                )));
            });
        }
        Cmd::SaveSession { path, session } => {
            let _ = crate::session::save(&path, &session);
        }
        Cmd::Setup(effect) => {
            let Some(services) = platform.setup().cloned() else {
                return;
            };
            let tx = tx.clone();
            tokio::spawn(async move {
                let input = crate::setup::run_effect(&services, effect).await;
                let _ = tx.send(Msg::Setup(input));
            });
        }
        Cmd::Settings(effect) => {
            let Some(services) = platform.settings().cloned() else {
                let _ = tx.send(Msg::Settings(crate::settings::unavailable(effect)));
                return;
            };
            let tx = tx.clone();
            tokio::spawn(async move {
                let input = crate::settings::run_effect(&services, effect).await;
                let _ = tx.send(Msg::Settings(input));
            });
        }
        Cmd::FinishSetup | Cmd::Suspend => {}
        Cmd::After { delay, msg } => {
            let tx = tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = tx.send(msg);
            });
        }
    }
}

/// Starts the interface and blocks until the user quits.
pub fn run(options: RunOptions) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(event_loop(options))
}

async fn event_loop(options: RunOptions) -> Result<()> {
    #[cfg_attr(not(feature = "live"), allow(unused_mut))]
    let mut backend = options.backend();
    let mut platform = Platform::system();
    if let Some((_, services)) = &options.setup {
        platform = platform.with_setup(std::sync::Arc::clone(services));
    }
    if let Some(services) = &options.settings_services {
        platform = platform.with_settings(std::sync::Arc::clone(services));
    }
    let open = options.open.clone();
    let mut guard = TerminalGuard::enter(!options.no_mouse)?;
    let size = guard.terminal.size()?;
    let base = AppConfig::from_env((size.width, size.height));
    let mut app = App::new(match &options.settings {
        Some(settings) => settings.app_config(base),
        None => base,
    });
    app.background.env = background_from_env();
    if let Some(settings) = &options.settings {
        settings.apply(&mut app);
    }
    app.rebuild_palette();

    app.demo = backend.is_demo();
    app.kitty_keys = terminal::keys_enhanced();
    let mode =
        crate::images::detect::mode_from_env(std::env::var("REVIEW_BUDDY_IMAGES").ok().as_deref())
            .or_else(|| options.settings.as_ref().map(|s| s.images))
            .unwrap_or(crate::config::Images::Auto);
    app.images = crate::images::State::new(
        mode,
        crate::images::detect::detect(mode, app.palette.depth(), app.palette.no_color()),
    );

    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let mut events = EventStream::new();
    let mut ticker = interval(TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_draw = Instant::now() - MIN_FRAME;

    if let Some((flow, _)) = options.setup.clone() {
        for cmd in crate::app::setup::start(&mut app, flow) {
            execute(cmd, &tx, &backend, &platform);
        }
    }

    #[cfg(feature = "live")]
    if let Backend::Live(live) = &backend {
        app.refresh_on_focus = live.refresh_on_focus;
        app.state.loading = true;
        if let Some(at) = live.cached_at() {
            update(&mut app, Msg::CacheTime(at));
        }
        if let Some(interval) = live.refresh_interval {
            live.spawn_interval(interval, &tx);
        }
        // Saved rows paint on the first frame; the refresh they trigger fills in behind them.
        for cmd in update(&mut app, Msg::Cached(Box::new(live.cached_snapshot()))) {
            execute(cmd, &tx, &backend, &platform);
        }
    }
    #[cfg(not(feature = "live"))]
    if matches!(backend, Backend::None) {
        let notice = Notice::new(
            NoticeKind::Info,
            "This build has no network support. Try review-buddy --demo to explore.",
        );
        for cmd in update(&mut app, Msg::Notify(notice)) {
            execute(cmd, &tx, &backend, &platform);
        }
    }
    #[cfg(feature = "demo")]
    if matches!(backend, Backend::Demo(_)) {
        app.state.loading = true;
        // The first frame already shows the loaded data; the load is local and immediate.
        execute(Cmd::LoadChanges, &tx, &backend, &platform);
        if let Some(msg) = rx.recv().await {
            for cmd in update(&mut app, msg) {
                execute(cmd, &tx, &backend, &platform);
            }
        }
    }

    if let Some(id) = open {
        for cmd in crate::app::open_change(&mut app, &id) {
            execute(cmd, &tx, &backend, &platform);
        }
    }

    while !app.should_quit() {
        if frame_due(app.is_dirty(), last_draw.elapsed()) {
            guard.terminal.draw(|frame| {
                let hits = ui::draw(frame, &app);
                app.hits = hits;
            })?;
            app.clear_dirty();
            last_draw = Instant::now();
        }

        let next_frame = last_draw + MIN_FRAME;
        let msg = tokio::select! {
            event = events.next() => match event {
                Some(Ok(event)) => msg_from_event(event),
                Some(Err(err)) => return Err(err.into()),
                None => break,
            },
            Some(msg) = rx.recv() => Some(msg),
            _ = ticker.tick() => Some(Msg::Tick),
            _ = sleep_until(next_frame), if app.is_dirty() => None,
        };
        if let Some(msg) = msg {
            for cmd in update(&mut app, msg) {
                if matches!(cmd, Cmd::Suspend) {
                    suspend(&mut guard, &mut app, &tx, &backend, &platform)?;
                } else if matches!(cmd, Cmd::FinishSetup) {
                    #[cfg(feature = "live")]
                    finish_setup(&options, &mut app, &mut backend, &tx, &platform);
                    #[cfg(not(feature = "live"))]
                    drop(cmd);
                } else {
                    execute(cmd, &tx, &backend, &platform);
                }
            }
        }
    }
    Ok(())
}

/// `⌃Z`: gives the terminal back, stops until the shell continues the process, then takes the
/// terminal again, repaints everything and refreshes as if focus had just returned.
fn suspend(
    guard: &mut TerminalGuard,
    app: &mut App,
    tx: &UnboundedSender<Msg>,
    backend: &Backend,
    platform: &Platform,
) -> Result<()> {
    guard.suspend();
    terminal::stop_process();
    guard.resume()?;
    let size = guard.terminal.size()?;
    let msgs = [Msg::Resize(size.width, size.height), Msg::FocusGained];
    for msg in msgs {
        for cmd in update(app, msg) {
            execute(cmd, tx, backend, platform);
        }
    }
    Ok(())
}

/// First run wrote a config: build the sources from it and load the queue behind the dashboard.
#[cfg(feature = "live")]
fn finish_setup(
    options: &RunOptions,
    app: &mut App,
    backend: &mut Backend,
    tx: &UnboundedSender<Msg>,
    platform: &Platform,
) {
    let built = match &options.reload {
        Some(reload) => (reload.0)(),
        None => Err(anyhow::anyhow!("nothing to reload")),
    };
    let notice = match built {
        Ok((settings, live)) => {
            settings.apply(app);
            app.refresh_on_focus = live.refresh_on_focus;
            app.state.loading = true;
            live.forget_probes();
            app.state.probes.clear();
            let snapshot = live.cached_snapshot();
            *backend = Backend::Live(live);
            for cmd in update(app, Msg::Cached(Box::new(snapshot))) {
                execute(cmd, tx, backend, platform);
            }
            return;
        }
        Err(err) => Notice::new(
            NoticeKind::Warning,
            format!("Saved your config, but couldn't load the queue: {err}. Restart review-buddy to try again."),
        ),
    };
    for cmd in update(app, Msg::Notify(notice)) {
        execute(cmd, tx, backend, platform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use crossterm::event::{KeyCode, KeyEvent};
    use rb_platform::{
        clipboard::ClipboardContext, CommandOutput, CommandRunner, Os, PlatformError,
    };

    fn remembered(
        edit: impl FnOnce(&mut crate::session::SessionLayout),
    ) -> Option<crate::session::Session> {
        let mut session = crate::session::Session::default();
        edit(&mut session.layout);
        Some(session)
    }

    fn launched(
        config_toml: &str,
        session: Option<crate::session::Session>,
        env: &[(&str, &str)],
    ) -> Settings {
        let mut config: crate::config::Config = toml::from_str(config_toml).unwrap();
        let mut map = rb_paths::MapEnv::new("/home/a");
        for (k, v) in env {
            map = map.with_var(k, v);
        }
        if let Some(p) = env
            .iter()
            .find(|(k, _)| *k == "REVIEW_BUDDY_DETAIL_POSITION")
            .and_then(|(_, v)| crate::config::parse_position(v))
        {
            config.ui.detail_position = p;
        }
        Settings::from_config(&config).with_session("/s/session.toml".into(), session, &map)
    }

    #[test]
    fn precedence_is_env_over_remembered_over_config_over_default() {
        use crate::config::DetailPosition::*;
        let left = remembered(|l| l.detail_position = Some(Left));
        let position =
            |toml, session, env: &[(&str, &str)]| launched(toml, session, env).layout.position;
        assert_eq!(position("", None, &[]), Auto, "built-in default");
        assert_eq!(
            position("[ui]\ndetail_position = \"bottom\"", None, &[]),
            Bottom,
            "config"
        );
        assert_eq!(
            position("[ui]\ndetail_position = \"bottom\"", left, &[]),
            Left,
            "remembered beats config"
        );
        assert_eq!(
            position(
                "[ui]\ndetail_position = \"bottom\"",
                left,
                &[("REVIEW_BUDDY_DETAIL_POSITION", "top")]
            ),
            Top,
            "env beats remembered"
        );
        assert_eq!(
            position("", left, &[("REVIEW_BUDDY_DETAIL_POSITION", "sideways")]),
            Left,
            "an env value that isn't one is ignored"
        );
    }

    #[test]
    fn the_session_key_beats_everything_it_was_seeded_from() {
        let settings = launched(
            "",
            remembered(|l| l.background = Some(rb_theme::BackgroundMode::No)),
            &[],
        );
        let mut app = App::new(crate::app::AppConfig {
            theme_id: "liminal-hq".into(),
            depth: rb_theme::ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        app.background.env = Some(rb_theme::BackgroundMode::Theme);
        settings.apply(&mut app);
        assert_eq!(app.background_mode(), rb_theme::BackgroundMode::Theme);
        app.background.session = Some(rb_theme::BackgroundMode::Yes);
        assert_eq!(app.background_mode(), rb_theme::BackgroundMode::Yes);
    }

    #[test]
    fn remembered_background_beats_config_but_not_env() {
        use rb_theme::BackgroundMode::*;
        let config = "[ui]\nbackground = \"no\"\n[ui.theme_background]\ndusk = \"no\"\n";
        let remembered = remembered(|l| l.background = Some(Yes));
        let s = launched(config, remembered, &[]);
        assert_eq!(s.background, Yes);
        assert!(s.per_theme_background.is_empty());
        let s = launched(config, remembered, &[("REVIEW_BUDDY_BACKGROUND", "theme")]);
        assert_eq!(s.background, No);
        assert_eq!(s.per_theme_background.len(), 1);
    }

    #[test]
    fn remembered_keys_beat_config_and_unset_ones_follow_it() {
        use crate::ui::layout::Size;
        let config = "[ui]\nsources = \"left\"\ndetail = \"closed\"\nqueue_width = 60\nqueue_height = \"40%\"\n";
        let session = remembered(|l| {
            l.sources = Some(crate::config::SourcesLayout::Top);
            l.queue_width = Some(Size::Cells(52));
            l.sources_width = Some(Size::Cells(30));
        });
        let layout = launched(config, session, &[]).layout;
        assert_eq!(layout.sources, crate::config::SourcesLayout::Top);
        assert_eq!(layout.detail, crate::config::DetailMode::Closed);
        assert_eq!(layout.split.queue_width, Some(Size::Cells(52)));
        assert_eq!(layout.split.queue_height, Some(Size::Percent(40)));
        assert_eq!(layout.split.sources_width, Some(Size::Cells(30)));
    }

    #[test]
    fn a_missing_file_changes_nothing_and_without_a_path_nothing_is_tracked() {
        let plain = Settings::from_config(&crate::config::Config::default());
        assert_eq!(
            launched("", None, &[]).layout,
            plain.layout,
            "no file, no change"
        );
        assert!(plain.session.is_none());
        let mut app = App::new(crate::app::AppConfig {
            theme_id: "liminal-hq".into(),
            depth: rb_theme::ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        plain.apply(&mut app);
        assert!(app.session.is_none());
        launched("", None, &[]).apply(&mut app);
        assert!(app.session.is_some());
    }

    #[test]
    fn frames_are_capped_and_only_when_dirty() {
        assert!(!frame_due(false, Duration::from_secs(10)));
        assert!(!frame_due(true, Duration::from_millis(10)));
        assert!(frame_due(true, MIN_FRAME));
        assert!(MIN_FRAME >= Duration::from_millis(33));
    }

    #[test]
    fn events_become_messages() {
        assert!(matches!(
            msg_from_event(Event::Key(KeyEvent::from(KeyCode::Char('q')))),
            Some(Msg::Key(_))
        ));
        assert!(matches!(
            msg_from_event(Event::Resize(1, 2)),
            Some(Msg::Resize(1, 2))
        ));
        assert!(matches!(
            msg_from_event(Event::FocusLost),
            Some(Msg::FocusLost)
        ));
        assert!(matches!(
            msg_from_event(Event::Paste("x".into())),
            Some(Msg::Paste(_))
        ));
    }

    #[tokio::test]
    async fn after_delivers_the_message_back() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        execute(
            Cmd::After {
                delay: Duration::from_millis(5),
                msg: Msg::ToastExpired(7),
            },
            &tx,
            &Backend::None,
            &test_platform(),
        );
        let got = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("message arrives")
            .expect("channel open");
        assert!(matches!(got, Msg::ToastExpired(7)));
    }

    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<String>>,
    }

    impl CommandRunner for Recorder {
        fn run(
            &self,
            program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            self.calls.lock().unwrap().push(program.to_string());
            Ok(CommandOutput {
                success: true,
                ..CommandOutput::default()
            })
        }
    }

    fn test_platform() -> Platform {
        platform_with(
            Arc::new(Recorder::default()),
            Arc::new(Mutex::new(Vec::new())),
        )
    }

    fn platform_with(runner: Arc<Recorder>, tty: Arc<Mutex<dyn Write + Send>>) -> Platform {
        let ctx = ClipboardContext {
            os: Os::Linux,
            in_tmux: false,
            wayland: false,
        };
        Platform::new(runner, tty, ctx)
    }

    async fn status(rx: &mut mpsc::UnboundedReceiver<Msg>) -> String {
        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("a status arrives")
            .expect("channel open");
        match msg {
            Msg::Status(notice) => notice.text,
            other => panic!("expected a status, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn open_url_runs_the_opener_and_reports() {
        let runner = Arc::new(Recorder::default());
        let platform = platform_with(runner.clone(), Arc::new(Mutex::new(Vec::new())));
        let (tx, mut rx) = mpsc::unbounded_channel();
        execute(
            Cmd::OpenUrl("https://github.com/a/b/pull/1".into()),
            &tx,
            &Backend::None,
            &platform,
        );
        assert_eq!(
            status(&mut rx).await,
            "Opened https://github.com/a/b/pull/1"
        );
        assert_eq!(
            runner.calls.lock().unwrap().as_slice(),
            [if cfg!(target_os = "macos") {
                "open"
            } else if cfg!(windows) {
                "rundll32"
            } else {
                "xdg-open"
            }]
        );
    }

    #[tokio::test]
    async fn copy_writes_osc52_through_the_terminal_writer() {
        let tty = Arc::new(Mutex::new(Vec::<u8>::new()));
        let platform = platform_with(Arc::new(Recorder::default()), tty.clone());
        let (tx, mut rx) = mpsc::unbounded_channel();
        execute(
            Cmd::Copy("https://x.test/1".into()),
            &tx,
            &Backend::None,
            &platform,
        );
        assert_eq!(status(&mut rx).await, "Copied https://x.test/1");
        assert!(tty.lock().unwrap().starts_with(b"\x1b]52;c;"));
    }

    #[cfg(feature = "demo")]
    #[tokio::test]
    async fn demo_never_opens_anything() {
        let runner = Arc::new(Recorder::default());
        let platform = platform_with(runner.clone(), Arc::new(Mutex::new(Vec::new())));
        let world = crate::demo::DemoWorld::new(
            crate::demo::parse_iso(crate::demo::DEFAULT_FROZEN).unwrap(),
        )
        .unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        execute(
            Cmd::OpenUrl("https://github.com/a/b/pull/1".into()),
            &tx,
            &Backend::Demo(world),
            &platform,
        );
        assert_eq!(
            status(&mut rx).await,
            "Would open https://github.com/a/b/pull/1 (demo)"
        );
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
