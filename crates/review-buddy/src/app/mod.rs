//! Elm-style app state: `update(&mut App, Msg) -> Vec<Cmd>` is pure, so it stays unit-testable.
//! Effects leave as [`Cmd`]s and results return as [`Msg`]s.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use rb_core::{
    ChangeId, ChangeSummary, Check, Comment, ReviewDraft, Source, SourceId, Thread, ThreadId,
    Timestamp, Verdict,
};
use rb_theme::{ColourDepth, Palette, Theme, BUILTIN_IDS, DEFAULT_THEME_ID};

use crate::ui::HitMap;

pub mod composer;
mod dashboard;
mod diff;
pub mod diffview;
pub mod editor;
pub mod failure;
pub mod links;
mod live;
mod mouse;
pub mod queue;
pub mod range;
mod update;

pub use dashboard::{Chip, Dashboard, Pane, Selected, Tab};
pub use diff::{
    open_change, DiffData, DiffFile, DiffFocus, DiffState, FileView, Intent, Phase, Syntax,
};
pub use failure::{FailureKind, SourceFailure};
pub use range::RowRange;
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
    /// Open the current change (or its diff page) in the browser.
    Open,
    /// Copy the current change's URL.
    Copy,
    ToggleHelp,
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
    /// Focus the Files or Diff pane by clicking its border or empty space.
    DiffFocus(DiffFocus),
    /// Place the composer's cursor where the pointer is. `x` and `y` are the screen position of
    /// the text's top-left cell; `first` and `across` are the rows and columns scrolled off.
    ComposerCursor {
        x: u16,
        y: u16,
        first: usize,
        across: usize,
    },
    /// Press the preview's Cancel (`false`) or confirm (`true`) button.
    Answer(bool),
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
    /// An async result shown in the footer.
    Status(Notice),
    StatusExpired(u64),
    ToastExpired(u64),
    /// The sources and changes a [`Cmd::LoadChanges`] asked for.
    Loaded(Box<Snapshot>),
    /// Saved rows to paint first. A refresh of every source follows straight away.
    Cached(Box<Snapshot>),
    /// One source's refresh finished.
    SourceLoaded {
        source: SourceId,
        result: Result<Vec<ChangeSummary>, SourceFailure>,
        now: Timestamp,
    },
    /// The description, checks and threads a [`Cmd::LoadInfo`] asked for.
    InfoLoaded {
        id: ChangeId,
        result: Result<Box<ChangeInfo>, String>,
    },
    /// The patches, threads and pending comments a [`Cmd::LoadDiff`] asked for.
    DiffLoaded {
        id: ChangeId,
        result: Result<Box<DiffData>, String>,
    },
    /// The answer to a [`Cmd::SubmitReview`]. `demo` marks results that only changed memory.
    ReviewSubmitted {
        id: ChangeId,
        verdict: Verdict,
        result: Result<(), String>,
        demo: bool,
    },
    /// The answer to a [`Cmd::Reply`].
    ReplyPosted {
        id: ChangeId,
        thread: ThreadId,
        result: Result<Comment, String>,
        demo: bool,
    },
}

/// Effects, run by the runtime. They never run inside `update`.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Deliver `msg` after `delay`.
    After { delay: Duration, msg: Msg },
    /// Fetch every source's changes from whatever backs this session.
    LoadChanges,
    /// Fetch the description, checks and threads for one change.
    LoadInfo(ChangeId),
    /// Fetch the files, threads and your pending comments for one change.
    LoadDiff(ChangeId),
    /// Submit `draft` to the forge with `verdict`.
    SubmitReview {
        id: ChangeId,
        draft: ReviewDraft,
        verdict: Verdict,
    },
    /// Reply on an existing thread.
    Reply {
        id: ChangeId,
        thread: ThreadId,
        body: String,
    },
    /// Open a web address in the browser.
    OpenUrl(String),
    /// Put text on the clipboard.
    Copy(String),
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
    /// Changes whose details have been asked for, so each is fetched once per refresh.
    pub info_requested: HashSet<ChangeId>,
    /// Sources that couldn't be refreshed, and why.
    pub failures: HashMap<SourceId, SourceFailure>,
    /// Sources a refresh is still waiting on.
    pub pending_sources: usize,
    pub sources: Vec<Source>,
    pub changes: Vec<ChangeSummary>,
    /// The clock the queue's relative ages are measured against.
    pub now: Option<Timestamp>,
    /// `[triage]` as the queue applies it.
    pub queue_settings: queue::QueueSettings,
}

impl AppState {
    pub fn source(&self, id: &SourceId) -> Option<&Source> {
        self.sources.iter().find(|s| &s.id == id)
    }
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
    /// Refresh when the terminal regains focus.
    pub refresh_on_focus: bool,
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
    /// The help overlay is showing.
    pub help: bool,
    /// Rows the help overlay is scrolled by.
    pub help_scroll: u16,
    /// The last left click, for spotting a double-click: what it hit and the tick it came on.
    pub(crate) last_click: Option<(Action, u64)>,
    /// Preview before posting a single comment now (`review.confirm_post_now`).
    pub confirm_post_now: bool,
    /// Columns a tab expands to in diffs (`diff.tab_width`).
    pub tab_width: u8,
    quit_armed: bool,
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
            refresh_on_focus: true,
            palette: Palette::new(builtin(theme_index), config.depth, config.no_color),
            status: None,
            toasts: Vec::new(),
            hits: HitMap::default(),
            source_label: "no sources".to_string(),
            change_count: 0,
            state: AppState::default(),
            dashboard: Dashboard::default(),
            diff: None,
            help: false,
            help_scroll: 0,
            last_click: None,
            confirm_post_now: true,
            tab_width: diffview::TAB_WIDTH,
            quit_armed: false,
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

    /// Whether quitting would lose review text that hasn't been sent.
    pub fn has_unsent_drafts(&self) -> bool {
        self.diff.as_ref().is_some_and(|s| {
            s.data.as_ref().is_some_and(|d| !d.draft.is_empty())
                || s.composer
                    .as_ref()
                    .is_some_and(composer::Composer::is_dirty)
        })
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
