//! Elm-style app state: `update(&mut App, Msg) -> Vec<Cmd>` is pure, so it stays unit-testable.
//! Effects leave as [`Cmd`]s and results return as [`Msg`]s.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use rb_core::{
    ChangeId, ChangeSummary, Check, Comment, CommentId, FeatureAction, ProbeOutcome, ReviewDraft,
    Source, SourceId, Thread, ThreadId, Timestamp, Verdict,
};
use rb_theme::{
    BackgroundMode, BackgroundSettings, ColourDepth, Palette, Theme, BUILTIN_IDS, DEFAULT_THEME_ID,
};

use crate::ui::HitMap;

pub mod comments;
pub mod composer;
mod dashboard;
mod diff;
pub mod diffview;
pub mod drafts;
pub mod editor;
pub mod failure;
pub mod images;
pub mod links;
mod live;
mod mouse;
pub mod pending;
pub mod projects;
pub mod queue;
pub mod range;
pub mod refresh;
pub mod resize;
pub mod review;
pub mod settings;
pub mod setup;
pub mod show;
pub mod terminal;
mod update;

pub use dashboard::{Chip, Dashboard, Pane, Selected, Tab};
pub use diff::{
    open_change, DiffData, DiffFile, DiffFocus, DiffState, FileView, Intent, Phase, Syntax,
};
pub use failure::{FailureKind, SourceFailure};
pub use range::RowRange;
pub use refresh::SourceStatus;
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
    /// First run: connecting accounts. See `crate::setup`.
    FirstRun,
    /// Settings → Sources. See `crate::settings`.
    Settings,
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
    CycleBackground,
    /// Close or reopen the Detail pane.
    ToggleDetail,
    /// Cycle where the Sources sit: auto, left, top.
    CycleSources,
    /// Cycle where Detail sits: auto, right, left, top, bottom.
    CyclePosition,
    /// Open the current change (or its diff page) in the browser.
    Open,
    /// Copy the current change's URL.
    Copy,
    ToggleHelp,
    /// Open the Show filters control.
    OpenShow,
    /// Tick or clear one project in the Show control.
    ToggleProject(SourceId, String),
    /// Put the cursor in the Show control's project search.
    FocusProjectSearch,
    CloseShow,
    /// Tick or clear one Show filter.
    ToggleShow(crate::config::ShowFilter),
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
    /// Pick the nth button of a three-way confirmation.
    Choose(usize),
    /// Open the Pending reviews list.
    OpenPending,
    /// Select the nth row of the Pending reviews list.
    PendingRow(usize),
    /// Jump to the nth pending comment listed in the Files pane.
    JumpComment(usize),
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
    /// Pick a verdict in the review modal.
    ReviewVerdict(Verdict),
    /// Reply to the thread at this diff row: moves the cursor there, then opens the reply box.
    ReplyAt(usize),
    /// Press the review modal's Cancel (`false`) or Submit (`true`) button.
    ReviewButton(bool),
    /// Put the review summary's caret where the pointer is; fields as for `ComposerCursor`.
    SummaryCursor {
        x: u16,
        y: u16,
        first: usize,
        across: usize,
    },
    /// Press the preview's Cancel (`false`) or confirm (`true`) button.
    Answer(bool),
    /// Open the terminal pane, or answer its start prompt.
    Terminal(terminal::TermAction),
    /// A press on part of the first-run screen.
    Setup(crate::setup::Click),
    /// Open Settings from the dashboard.
    OpenSettings,
    /// A press on part of the Settings screen.
    Settings(crate::settings::Click),
}

/// Input, timers, and results of effects.
#[derive(Debug, Clone)]
pub enum Msg {
    Key(crossterm::event::KeyEvent),
    Mouse(crossterm::event::MouseEvent),
    Resize(u16, u16),
    Tick,
    /// The debounce after a layout change passed: write the remembered layout.
    SaveSessionDue,
    /// The debounce after a draft change passed: write the changed drafts.
    SaveDraftsDue,
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
    /// A source's state changed (paused, offline, back to normal).
    SourceStatus {
        source: SourceId,
        status: SourceStatus,
    },
    /// A retry inside a refresh that has already answered once: updates rows without
    /// counting as another answer.
    SourceUpdated {
        source: SourceId,
        result: Result<Vec<ChangeSummary>, SourceFailure>,
        now: Timestamp,
    },
    /// When the saved rows were last fetched, for the offline banner.
    CacheTime(Timestamp),
    /// The refresh interval elapsed.
    RefreshDue,
    /// A refresh was asked for too soon after the last one, so nothing was sent.
    RefreshSkipped,
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
    /// The answer to a [`Cmd::EditComment`].
    CommentEdited {
        id: ChangeId,
        thread: ThreadId,
        comment: CommentId,
        body: String,
        result: Result<(), String>,
        demo: bool,
    },
    /// The answer to a [`Cmd::DeleteComment`].
    CommentDeleted {
        id: ChangeId,
        thread: ThreadId,
        comment: CommentId,
        result: Result<(), String>,
        demo: bool,
    },
    /// The result of a first-run [`Cmd::Setup`] effect.
    Setup(crate::setup::Input),
    /// What a source can do, from its capability probe. `at` is when the instance was asked.
    Probed {
        source: SourceId,
        outcome: Box<ProbeOutcome>,
        at: Timestamp,
    },
    /// The result of a Settings [`Cmd::Settings`] effect.
    Settings(crate::settings::Input),
    /// An image a [`Cmd::FetchImage`] asked for, decoded, or why it can't be shown.
    ImageLoaded {
        url: String,
        result: Result<std::sync::Arc<image::DynamicImage>, crate::images::Failure>,
    },
    /// The answer to a [`Cmd::Reply`].
    ReplyPosted {
        id: ChangeId,
        thread: ThreadId,
        result: Result<Comment, String>,
        demo: bool,
    },
    /// Output, exit or a planning result from the terminal pane.
    Term(terminal::TermMsg),
}

/// Effects, run by the runtime. They never run inside `update`.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Deliver `msg` after `delay`.
    After { delay: Duration, msg: Msg },
    /// Fetch every source's changes from whatever backs this session. Automatic refreshes
    /// (launch, interval) respect a source's backoff.
    LoadChanges,
    /// The same, because you asked (`r`): cuts a backoff wait short, but never a rate-limit pause.
    LoadChangesNow,
    /// The same, because the terminal regained focus: skipped if one just ran.
    LoadChangesOnFocus,
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
    /// Rewrite one of your pending comments on the forge.
    EditComment {
        id: ChangeId,
        thread: ThreadId,
        comment: CommentId,
        body: String,
    },
    /// Remove one of your pending comments from the forge.
    DeleteComment {
        id: ChangeId,
        thread: ThreadId,
        comment: CommentId,
    },
    /// Write changed drafts into `dir`; `None` removes that change's file.
    SaveDrafts {
        dir: std::path::PathBuf,
        writes: Vec<(ChangeId, Option<crate::drafts::StoredDraft>)>,
    },
    /// Fetch, decode and cache one image from a description, for the source that shows it.
    FetchImage { source: SourceId, url: String },
    /// Write each source's `hide_repos` into the config file at `target`.
    SaveHiddenProjects {
        target: std::path::PathBuf,
        sources: Vec<(String, Vec<String>)>,
    },
    /// Write `ui.queue_width` and `ui.queue_height` into the config file at `target`; a size
    /// that is automatic (`None`) removes its key.
    SaveLayoutSizes {
        target: std::path::PathBuf,
        queue_width: Option<crate::ui::layout::Size>,
        queue_height: Option<crate::ui::layout::Size>,
    },
    /// Write the remembered layout to `path`, quietly.
    SaveSession {
        path: std::path::PathBuf,
        session: crate::session::Session,
    },
    /// Run one first-run effect (detection, a token check, the config write).
    Setup(crate::setup::Effect),
    /// Run one Settings effect (read the config, test a token, write a change).
    Settings(crate::settings::Effect),
    /// The config was written: rebuild the sources from it and load the queue.
    FinishSetup,
    /// Hand the terminal back and stop the process until the shell continues it (`⌃Z`).
    Suspend,
    /// Open a web address in the browser.
    OpenUrl(String),
    /// Put text on the clipboard.
    Copy(String),
    /// An effect for the terminal pane: its PTY and its worktree.
    Term(terminal::TermCmd),
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
    /// What each source can do, once probed. Sources not in here show every action.
    pub probes: HashMap<SourceId, SourceProbe>,
    /// The sources in `pending_sources`, so each row can show its own spinner.
    pub refreshing: HashSet<SourceId>,
    /// Each source's last reported state.
    pub statuses: HashMap<SourceId, SourceStatus>,
    /// When a source last answered with fresh or confirmed-unchanged rows.
    pub last_refreshed: Option<Timestamp>,
    pub sources: Vec<Source>,
    pub changes: Vec<ChangeSummary>,
    /// The clock the queue's relative ages are measured against.
    pub now: Option<Timestamp>,
    /// `[triage]` as the queue applies it.
    pub queue_settings: queue::QueueSettings,
}

/// A source's probe result and when it was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProbe {
    pub outcome: ProbeOutcome,
    pub at: Timestamp,
}

impl AppState {
    pub fn source(&self, id: &SourceId) -> Option<&Source> {
        self.sources.iter().find(|s| &s.id == id)
    }

    /// Whether `action` is on for the source. Unprobed sources are given the benefit of the doubt.
    pub fn supports(&self, source: &SourceId, action: FeatureAction) -> bool {
        self.probes
            .get(source)
            .is_none_or(|p| p.outcome.capabilities.supports(action))
    }

    /// A calm sentence for why `action` is off on the source; `None` when it's on or unprobed.
    pub fn explain_unsupported(&self, source: &SourceId, action: FeatureAction) -> Option<String> {
        let probe = self.probes.get(source)?;
        if probe.outcome.capabilities.supports(action) {
            return None;
        }
        let host = self
            .source(source)
            .map_or("this source", |s| s.host.as_str());
        let alternative = match action {
            FeatureAction::RequestChanges => " Approve or comment instead.",
            _ => "",
        };
        Some(match probe.outcome.reason(action) {
            Some(reason) => format!("{reason}{alternative}"),
            None => probe
                .outcome
                .capabilities
                .explain_unsupported(action, host)
                .unwrap_or_default(),
        })
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
    /// The background setting by layer: config, `REVIEW_BUDDY_BACKGROUND`, and this session.
    pub background: BackgroundSettings,
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
    /// Your unsent review comments per change: kept when you leave a diff, saved to disk
    /// unless they are memory only.
    pub drafts: drafts::Drafts,
    /// The Pending reviews list, while it is on screen.
    pub pending: Option<pending::PendingList>,
    /// The first-run flow, while it is on screen.
    pub setup: Option<crate::setup::Flow>,
    /// Settings, while it is on screen.
    pub settings: Option<crate::settings::State>,
    /// Running on demo data: Settings shows it read-only and touches nothing real.
    pub demo: bool,
    /// The terminal reports `⌃⏎` as such (the kitty keyboard protocol). Set at startup.
    pub kitty_keys: bool,
    /// Pictures in descriptions: the renderer, each address's progress and the selected image.
    pub images: crate::images::State,
    /// The help overlay is showing.
    pub help: bool,
    /// Rows the help overlay is scrolled by.
    pub help_scroll: u16,
    /// The Show filters control.
    pub show: show::ShowControl,
    /// The config file `w` in the Show control writes the project choice to. Never set in demo
    /// mode.
    pub project_save: Option<std::path::PathBuf>,
    /// The last left click, for spotting a double-click: what it hit and the tick it came on.
    pub(crate) last_click: Option<(Action, u64)>,
    /// Preview before posting a single comment now (`review.confirm_post_now`).
    pub confirm_post_now: bool,
    /// Columns a tab expands to in diffs (`diff.tab_width`).
    pub tab_width: u8,
    /// Still glyphs instead of a spinner (`ui.reduced_motion`).
    pub reduced_motion: bool,
    /// Where Sources and Detail sit (`ui.sources`, `ui.detail`), as changed this session.
    pub layout: crate::ui::layout::Options,
    /// Remembers layout changes between runs. `None` in demo mode and when it is turned off.
    pub session: Option<crate::session::Tracker>,
    /// The terminal pane.
    pub term: terminal::TerminalState,
    /// The host terminal reports keys in enough detail for the kitty keyboard protocol.
    pub kitty_keys: bool,
    /// The seam being dragged with the mouse.
    pub drag: Option<resize::Drag>,
    /// The last press on a seam, for spotting a double-click: which seam and the tick.
    pub(crate) last_seam: Option<(crate::ui::layout::SeamKind, u64)>,
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
            background: BackgroundSettings::default(),
            status: None,
            toasts: Vec::new(),
            hits: HitMap::default(),
            source_label: "no sources".to_string(),
            change_count: 0,
            state: AppState::default(),
            dashboard: Dashboard::default(),
            diff: None,
            drafts: drafts::Drafts::default(),
            pending: None,
            setup: None,
            settings: None,
            demo: false,
            kitty_keys: false,
            images: crate::images::State::default(),
            help: false,
            help_scroll: 0,
            show: show::ShowControl::default(),
            project_save: None,
            last_click: None,
            confirm_post_now: true,
            tab_width: diffview::TAB_WIDTH,
            reduced_motion: false,
            layout: crate::ui::layout::Options::default(),
            session: None,
            term: terminal::TerminalState::default(),
            kitty_keys: false,
            drag: None,
            last_seam: None,
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
        !self.drafts.is_empty()
            || self.diff.as_ref().is_some_and(|s| {
                s.data.as_ref().is_some_and(|d| !d.draft.is_empty())
                    || s.composer
                        .as_ref()
                        .is_some_and(composer::Composer::is_dirty)
                    || s.review.as_ref().is_some_and(|r| !r.summary.is_blank())
            })
    }

    /// Switches to a built-in theme by id; unknown ids are ignored.
    pub fn set_theme(&mut self, id: &str) {
        if let Some(i) = BUILTIN_IDS.iter().position(|b| *b == id) {
            self.theme_index = i;
            self.rebuild_palette();
            self.mark_dirty();
        }
    }

    fn cycle_theme(&mut self) {
        self.theme_index = (self.theme_index + 1) % BUILTIN_IDS.len();
        self.rebuild_palette();
    }

    /// The background mode in force for the current theme, after every layer.
    pub fn background_mode(&self) -> BackgroundMode {
        self.background.mode_for(&self.palette.theme().id)
    }

    /// The layout as the remembered session sees it.
    pub fn session_snapshot(&self) -> crate::session::Snapshot {
        crate::session::Snapshot {
            options: self.layout,
            background: self.background.session,
            terminal: (self.term.position, self.term.size),
        }
    }

    /// Steps the session's background through theme, yes and no.
    pub fn cycle_background(&mut self) {
        self.background.session = Some(self.background_mode().next());
        self.rebuild_palette();
        self.mark_dirty();
    }

    /// Rebuilds the palette for the current theme and background mode.
    pub fn rebuild_palette(&mut self) {
        let theme = builtin(self.theme_index);
        let mode = self.background.mode_for(&theme.id);
        self.palette = Palette::new(theme, self.palette.depth(), self.palette.no_color())
            .with_background_mode(mode);
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
