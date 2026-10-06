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

/// Why review writes aren't sent to live sources yet.
#[cfg(feature = "live")]
const NOT_WIRED: &str = "Sending reviews isn't wired up for live sources yet";

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

/// How the interface should start.
#[derive(Debug, Default)]
pub struct RunOptions {
    #[cfg(feature = "demo")]
    pub demo: Option<crate::demo::Demo>,
    /// Start on this change's diff instead of the dashboard.
    pub open: Option<rb_core::ChangeId>,
    #[cfg(feature = "live")]
    pub live: Option<std::sync::Arc<crate::providers::Live>>,
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
        Cmd::LoadChanges => match backend {
            Backend::None => {}
            #[cfg(feature = "demo")]
            Backend::Demo(world) => {
                let world = world.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let msg = match world.snapshot().await {
                        Ok(snapshot) => Msg::Loaded(Box::new(snapshot)),
                        Err(err) => Msg::Notify(crate::app::Notice::new(
                            crate::app::NoticeKind::Warning,
                            format!("The demo data didn't load: {err}."),
                        )),
                    };
                    let _ = tx.send(msg);
                });
            }
            #[cfg(feature = "live")]
            Backend::Live(live) => live.refresh(tx),
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
            Backend::Live(_) => {
                let _ = &draft;
                let _ = tx.send(Msg::ReviewSubmitted {
                    id,
                    verdict,
                    result: Err(NOT_WIRED.to_string()),
                    demo: false,
                });
            }
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
            Backend::Live(_) => {
                let _ = &body;
                let _ = tx.send(Msg::ReplyPosted {
                    id,
                    thread,
                    result: Err(NOT_WIRED.to_string()),
                    demo: false,
                });
            }
        },
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
    let backend = options.backend();
    let platform = Platform::system();
    let open = options.open.clone();
    let mut guard = TerminalGuard::enter()?;
    let size = guard.terminal.size()?;
    let mut app = App::new(AppConfig::from_env((size.width, size.height)));

    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let mut events = EventStream::new();
    let mut ticker = interval(TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_draw = Instant::now() - MIN_FRAME;

    #[cfg(feature = "live")]
    if let Backend::Live(live) = &backend {
        app.refresh_on_focus = live.refresh_on_focus;
        app.state.loading = true;
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
                execute(cmd, &tx, &backend, &platform);
            }
        }
    }
    Ok(())
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
