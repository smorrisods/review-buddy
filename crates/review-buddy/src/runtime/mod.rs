//! The async event loop: terminal events and timers in, `Msg`s through `update`, `Cmd`s out.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::time::{interval, sleep_until, Instant, MissedTickBehavior};

use crate::app::{update, App, AppConfig, Cmd, Msg};
use crate::ui;

mod terminal;

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
}

/// How the interface should start.
#[derive(Debug, Default)]
pub struct RunOptions {
    #[cfg(feature = "demo")]
    pub demo: Option<crate::demo::Demo>,
}

impl RunOptions {
    fn backend(&self) -> Backend {
        #[cfg(feature = "demo")]
        if let Some(demo) = &self.demo {
            return Backend::Demo(demo.world.clone());
        }
        Backend::None
    }
}

/// Runs one effect. Results come back as messages on `tx`.
pub fn execute(cmd: Cmd, tx: &UnboundedSender<Msg>, backend: &Backend) {
    match cmd {
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
        .enable_time()
        .build()?;
    runtime.block_on(event_loop(options))
}

async fn event_loop(options: RunOptions) -> Result<()> {
    let backend = options.backend();
    let mut guard = TerminalGuard::enter()?;
    let size = guard.terminal.size()?;
    let mut app = App::new(AppConfig::from_env((size.width, size.height)));

    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let mut events = EventStream::new();
    let mut ticker = interval(TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_draw = Instant::now() - MIN_FRAME;

    if !matches!(backend, Backend::None) {
        app.state.loading = true;
        // The first frame already shows the loaded data; the load is local and immediate.
        execute(Cmd::LoadChanges, &tx, &backend);
        if let Some(msg) = rx.recv().await {
            for cmd in update(&mut app, msg) {
                execute(cmd, &tx, &backend);
            }
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
                execute(cmd, &tx, &backend);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent};

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
        );
        let got = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("message arrives")
            .expect("channel open");
        assert!(matches!(got, Msg::ToastExpired(7)));
    }
}
