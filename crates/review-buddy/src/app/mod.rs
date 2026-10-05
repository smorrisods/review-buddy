//! Elm-style app state: `update(&mut App, Msg) -> Vec<Cmd>` is pure, so it stays unit-testable.
//! Effects leave as [`Cmd`]s and results return as [`Msg`]s.

use std::collections::HashMap;
use std::time::Duration;

use rb_core::{ChangeId, ChangeSummary, Check, Source, Thread, Timestamp};
use rb_theme::{ColourDepth, Palette, Theme, BUILTIN_IDS, DEFAULT_THEME_ID};

use crate::ui::HitMap;

mod dashboard;
mod diff;
pub mod diffview;
pub mod queue;
mod update;

pub use dashboard::{Chip, Dashboard, Pane, Selected, Tab};
pub use diff::{DiffData, DiffFile, DiffFocus, DiffState, FileView, Phase, Syntax};
pub use update::update;

/// How long a status message or toast stays on screen.
pub const NOTICE_TTL: Duration = Duration::from_secs(4);
/// Toasts beyond this many drop the oldest.
pub const MAX_TOASTS: usize = 3;

/// The screen shown in the body. Later screens plug in here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Dashboard,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Success,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

impl Notice {
    pub fn new(kind: NoticeKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

/// A notice with an id, so its expiry timer can find it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub notice: Notice,
}

/// Something a mouse click or a key hint can trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    CycleTheme,
    DismissToast(u64),
    FocusPane(Pane),
    /// Show one source: 0 is All, then each source in order.
    SelectSource(usize),
    /// Select the nth item of the queue.
    SelectItem(usize),
    SelectTab(Tab),
    Chip(Chip),
    /// Leave the diff for the dashboard.
    CloseDiff,
    /// Choose the nth file of the diff.
    DiffFile(usize),
    /// Put the diff cursor at the nth row, or the nearest line to it.
    DiffRow(usize),
}

/// Input, timers, and results of effects.
#[derive(Debug, Clone)]
pub enum Msg {
    Key(crossterm::event::KeyEvent),
    Mouse(crossterm::event::MouseEvent),
    Resize(u16, u16),
    Tick,
    FocusGained,
    FocusLost,
    Paste(String),
    /// An async result that wants the user's attention.
    Notify(Notice),
    StatusExpired(u64),
    ToastExpired(u64),
    /// The sources and changes a [`Cmd::LoadChanges`] asked for.
    Loaded(Box<Snapshot>),
    /// The patches, threads and pending comments a [`Cmd::LoadDiff`] asked for.
    DiffLoaded {
        id: ChangeId,
        result: Result<Box<DiffData>, String>,
    },
}

/// Effects, run by the runtime. They never run inside `update`.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Deliver `msg` after `delay`.
    After { delay: Duration, msg: Msg },
    /// Fetch every source's changes from whatever backs this session.
    LoadChanges,
    /// Fetch the files, threads and your pending comments for one change.
    LoadDiff(ChangeId),
}

/// The sources and changes handed to the app in one go.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Short name of the backing store, shown in the top bar (`demo`).
    pub label: String,
    pub sources: Vec<Source>,
    pub changes: Vec<ChangeSummary>,
    pub now: Timestamp,
    pub details: HashMap<ChangeId, ChangeInfo>,
}

/// The slower, per-change data the detail pane shows beyond the summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeInfo {
    /// Markdown description.
    pub body: String,
    pub checks: Vec<Check>,
    pub threads: Vec<Thread>,
}

/// Remote data held by the app. Screens read it; only `update` writes it.
#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub loaded: bool,
    /// A load has been asked for and hasn't answered yet.
    pub loading: bool,
    pub details: HashMap<ChangeId, ChangeInfo>,
    pub sources: Vec<Source>,
    pub changes: Vec<ChangeSummary>,
    /// The clock the queue's relative ages are measured against.
    pub now: Option<Timestamp>,
}

/// What the app needs to know about the terminal it starts in.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub theme_id: String,
    pub depth: ColourDepth,
    pub no_color: bool,
    pub size: (u16, u16),
}

impl AppConfig {
    pub fn from_env(size: (u16, u16)) -> Self {
        Self {
            theme_id: DEFAULT_THEME_ID.to_string(),
            depth: ColourDepth::from_env(),
            no_color: rb_theme::no_color_requested(),
            size,
        }
    }
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub size: (u16, u16),
    pub focused: bool,
    pub palette: Palette,
    pub status: Option<Entry>,
    pub toasts: Vec<Entry>,
    /// Rectangles from the last draw, used to resolve clicks.
    pub hits: HitMap,
    /// Short description of the connected sources, shown in the top bar.
    pub source_label: String,
    pub change_count: usize,
    pub state: AppState,
    pub dashboard: Dashboard,
    pub diff: Option<DiffState>,
    pub(crate) syntax: Syntax,
    pub(crate) ticks: u64,
    pub(crate) next_id: u64,
    theme_index: usize,
    quit: bool,
    dirty: bool,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let theme_index = BUILTIN_IDS
            .iter()
            .position(|id| *id == config.theme_id)
            .unwrap_or(0);
        Self {
            screen: Screen::default(),
            size: config.size,
            focused: true,
            palette: Palette::new(builtin(theme_index), config.depth, config.no_color),
            status: None,
            toasts: Vec::new(),
            hits: HitMap::default(),
            source_label: "no sources".to_string(),
            change_count: 0,
            state: AppState::default(),
            dashboard: Dashboard::default(),
            diff: None,
            syntax: Syntax::default(),
            ticks: 0,
            next_id: 1,
            theme_index,
            quit: false,
            dirty: true,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn theme_name(&self) -> &str {
        &self.palette.theme().name
    }

    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    fn cycle_theme(&mut self) {
        self.theme_index = (self.theme_index + 1) % BUILTIN_IDS.len();
        self.palette = Palette::new(
            builtin(self.theme_index),
            self.palette.depth(),
            self.palette.no_color(),
        );
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

fn builtin(index: usize) -> Theme {
    Theme::builtin(BUILTIN_IDS[index]).expect("built-in themes are embedded and valid")
}
