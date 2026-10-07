//! Diff screen state and its pure transitions: loading, file and line selection, hunk and file
//! navigation, scrolling. Highlighting runs here (it is CPU only) so drawing stays read-only.

use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_core::{Capabilities, ChangeId, FilePatch, ReviewDraft, Thread, Timestamp, Verdict};
use rb_diff::{
    DiffBody, FileDiff, HighlightedFile, Highlighter, Hunk, HunkHeader, LineId, ParsedPatch,
};

use super::composer::{self, Composer, Confirm};
use super::diffview::{self, Inputs, Rows};
use super::range::{self, RowRange};
use super::review::{self, ReviewModal};
use super::update::set_status;
use super::{comments, drafts};
use super::{Action, App, Cmd, Notice, NoticeKind, Screen};
use crate::ui::layout;

const WHEEL_ROWS: usize = 3;
/// Lines highlighted together when a file is too big to highlight whole.
pub const CHUNK_LINES: usize = 150;

/// One file of a change: parsed once, off the event loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    pub diff: FileDiff,
    pub adds: u32,
    pub dels: u32,
}

/// Everything the diff screen needs from a forge for one change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffData {
    pub files: Vec<DiffFile>,
    pub threads: Vec<Thread>,
    /// Your pending, unsubmitted comments.
    pub draft: ReviewDraft,
    /// What the forge behind this change can do.
    pub caps: Capabilities,
    /// The forge can edit and delete your pending comments (`Provider::supports_pending_edit`).
    pub edit_pending: bool,
}

impl DiffData {
    pub fn new(files: Vec<FilePatch>, threads: Vec<Thread>, draft: ReviewDraft) -> Self {
        let files = files
            .iter()
            .map(|f| DiffFile {
                diff: FileDiff::from_file_patch(f),
                adds: f.adds,
                dels: f.dels,
            })
            .collect();
        Self {
            files,
            threads,
            draft,
            caps: Capabilities::all(),
            edit_pending: false,
        }
    }

    pub fn with_pending_edit(mut self, yes: bool) -> Self {
        self.edit_pending = yes;
        self
    }

    pub fn with_capabilities(mut self, caps: Capabilities) -> Self {
        self.caps = caps;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Failed(String),
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFocus {
    Files,
    Diff,
    /// The list of pending comments under the files.
    Review,
}

/// The syntax highlighter, created on first use because loading the grammars isn't free.
#[derive(Default)]
pub struct Syntax(Option<Highlighter>);

impl std::fmt::Debug for Syntax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Syntax(..)")
    }
}

/// Syntax highlighting for the current file. Big files (above rb-diff's lazy threshold) are
/// highlighted a chunk of lines at a time as they scroll into view, restarting the parser at
/// each chunk, so opening one never waits on the whole file.
#[derive(Debug, Default)]
pub enum Highlight {
    #[default]
    None,
    Whole(Arc<HighlightedFile>),
    Windowed(HashMap<(u32, u32), Arc<HighlightedFile>>),
}

#[derive(Debug, Default)]
pub struct FileView {
    pub rows: Rows,
    /// A row index that rests on a diff line.
    pub cursor: usize,
    pub scroll: usize,
    pub highlight: Highlight,
}

impl FileView {
    pub fn spans(&self, id: LineId) -> Option<&[rb_diff::Span]> {
        match &self.highlight {
            Highlight::None => None,
            Highlight::Whole(file) => file.line(id),
            Highlight::Windowed(chunks) => {
                let at = id.line as usize;
                chunks
                    .get(&(id.hunk, (at / CHUNK_LINES) as u32))?
                    .line(LineId {
                        hunk: 0,
                        line: (at % CHUNK_LINES) as u32,
                    })
            }
        }
    }

    /// How many chunks have been highlighted so far (0 unless the file is windowed).
    pub fn chunks(&self) -> usize {
        match &self.highlight {
            Highlight::Windowed(chunks) => chunks.len(),
            _ => 0,
        }
    }
}

#[derive(Debug)]
pub struct DiffState {
    pub id: ChangeId,
    pub phase: Phase,
    pub data: Option<DiffData>,
    pub focus: DiffFocus,
    pub file: usize,
    pub files_scroll: usize,
    pub view: FileView,
    /// The lines a drag or shift-click has selected. Groundwork for range comments.
    pub range: Option<RowRange>,
    /// The row a drag began on, while the button is held.
    pub drag: Option<usize>,
    /// The row a line-wise selection (`V`) started on, while that mode is on.
    pub select: Option<usize>,
    /// The docked composer, while one is open.
    pub composer: Option<Composer>,
    /// A preview or discard confirmation, shown over everything else.
    pub confirm: Option<Confirm>,
    /// The submit-review modal, shown over everything else.
    pub review: Option<ReviewModal>,
    /// A write is on its way to the forge and hasn't answered yet.
    pub submitting: bool,
    /// What to start once the patches arrive, when a dashboard key opened the diff.
    pub intent: Option<Intent>,
    /// The verdict you last chose in the review modal, kept with the draft.
    pub verdict: Option<Verdict>,
    /// The change's head moved since the restored draft was written.
    pub stale: bool,
    /// Which pending comment on the cursor line `e` and `d` act on: `(cursor row, index)`.
    pub pick: Option<(usize, usize)>,
    /// The selected entry of the pending-comment list.
    pub review_sel: usize,
    /// The draft as it was when the diff opened (or you last chose to keep it), to tell whether
    /// leaving needs to ask.
    pub baseline: Option<(ReviewDraft, Option<Verdict>)>,
}

/// An action the dashboard asked the diff to start as soon as it is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Approve,
    RequestChanges,
    Comment,
}

impl DiffState {
    pub fn loading(id: ChangeId) -> Self {
        Self {
            id,
            phase: Phase::Loading,
            data: None,
            focus: DiffFocus::Diff,
            file: 0,
            files_scroll: 0,
            view: FileView::default(),
            range: None,
            drag: None,
            select: None,
            composer: None,
            confirm: None,
            review: None,
            submitting: false,
            intent: None,
            verdict: None,
            stale: false,
            pick: None,
            review_sel: 0,
            baseline: None,
        }
    }

    /// Whether a composer or confirmation holds the keyboard.
    pub fn has_overlay(&self) -> bool {
        self.composer.is_some() || self.confirm.is_some() || self.review.is_some()
    }

    pub fn files(&self) -> &[DiffFile] {
        self.data.as_ref().map_or(&[], |d| &d.files)
    }

    pub fn current(&self) -> Option<&DiffFile> {
        self.files().get(self.file)
    }
}

/// Switches to the diff screen for the selected change and asks for its patches.
pub fn open(app: &mut App) -> Vec<Cmd> {
    open_with_intent(app, None)
}

/// Like [`open`], and starts `intent` (a review modal or a comment) once the diff loads.
pub fn open_with_intent(app: &mut App, intent: Option<Intent>) -> Vec<Cmd> {
    let Some(change) = app.selected_change() else {
        return Vec::new();
    };
    let id = change.id.clone();
    let mut state = DiffState::loading(id.clone());
    state.intent = intent;
    app.diff = Some(state);
    app.screen = Screen::Diff;
    app.mark_dirty();
    vec![Cmd::LoadDiff(id)]
}

/// Starts on the diff for `id`, as `review-buddy open` does. The queue behind it may still be
/// loading, so the change doesn't have to be known yet.
pub fn open_change(app: &mut App, id: &ChangeId) -> Vec<Cmd> {
    app.diff = Some(DiffState::loading(id.clone()));
    app.screen = Screen::Diff;
    app.mark_dirty();
    vec![Cmd::LoadDiff(id.clone())]
}

/// Leaves the diff, asking first when there are unsaved comments.
fn close(app: &mut App) -> Vec<Cmd> {
    drafts::leave(app)
}

/// The cursor's line, for putting it back later.
pub(super) fn cursor_id(state: &DiffState) -> Option<LineId> {
    state.view.rows.line_id(state.view.cursor)
}

pub(super) fn cursor_id_of(app: &App) -> Option<LineId> {
    app.diff.as_ref().and_then(cursor_id)
}

/// Moves to the line a comment hangs from, in the file at `path`.
pub(super) fn goto_anchor(app: &mut App, path: &str, side: rb_core::Side, line: u32) {
    let id = app
        .diff
        .as_ref()
        .and_then(|s| s.files().iter().find(|f| f.diff.path == path))
        .and_then(|f| f.diff.parsed())
        .and_then(|p| p.find_by_anchor(rb_diff::Anchor { side, line }));
    if let Some(id) = id {
        goto(app, path, id);
    }
}

/// Moves to `line` of the file at `path`, when both exist.
pub(super) fn goto(app: &mut App, path: &str, line: LineId) {
    let Some(index) = app
        .diff
        .as_ref()
        .and_then(|s| s.files().iter().position(|f| f.diff.path == path))
    else {
        return;
    };
    select_file(app, index);
    let height = usize::from(viewport(app).code.height);
    if let Some(state) = app.diff.as_mut() {
        if let Some(row) = state.view.rows.row_of(line) {
            state.view.cursor = row;
            reveal_cursor(state, height);
        }
    }
    settle(app);
}

/// The result of [`Cmd::LoadDiff`]. A result for a change the user has left is dropped.
pub fn on_loaded(app: &mut App, id: &ChangeId, result: Result<Box<DiffData>, String>) -> Vec<Cmd> {
    let Some(state) = app.diff.as_mut().filter(|s| &s.id == id) else {
        return Vec::new();
    };
    match result {
        Ok(data) => {
            state.data = Some(*data);
            state.phase = Phase::Ready;
            state.file = 0;
            state.files_scroll = 0;
        }
        Err(message) => state.phase = Phase::Failed(message),
    }
    rebuild(app, None);
    let cmds = drafts::restore(app);
    start_intent(app);
    app.mark_dirty();
    cmds
}

/// Starts what the dashboard asked for, with the cursor on the first changed line.
fn start_intent(app: &mut App) {
    let height = usize::from(viewport(app).code.height);
    let Some(state) = app.diff.as_mut() else {
        return;
    };
    let Some(intent) = state.intent.take() else {
        return;
    };
    if state.data.is_none() {
        return;
    }
    if let Some(row) = first_changed_row(state) {
        state.view.cursor = row;
        reveal_cursor(state, height);
    }
    match intent {
        Intent::Approve => review::open(app, Verdict::Approve),
        Intent::RequestChanges => review::open(app, Verdict::RequestChanges),
        Intent::Comment => composer::open_comment(app),
    };
}

fn first_changed_row(state: &DiffState) -> Option<usize> {
    let patch = state.current()?.diff.parsed()?;
    let (id, _) = patch
        .iter()
        .find(|(_, line)| line.kind != rb_diff::LineKind::Context)?;
    state.view.rows.row_of(id)
}

/// Swaps in freshly loaded threads for the open diff, keeping the cursor where it was.
pub(super) fn refresh_threads(app: &mut App, id: &ChangeId, threads: &[rb_core::Thread]) {
    let keep = app
        .diff
        .as_ref()
        .and_then(|s| s.view.rows.line_id(s.view.cursor));
    let Some(data) = app
        .diff
        .as_mut()
        .filter(|s| &s.id == id)
        .and_then(|s| s.data.as_mut())
    else {
        return;
    };
    if data.threads == threads {
        return;
    }
    data.threads = threads.to_vec();
    rebuild(app, keep);
    app.mark_dirty();
}

pub(super) fn viewport(app: &App) -> layout::DiffLayout {
    let extra = app.diff.as_ref().map_or(0, comments::review_extra);
    layout::diff_screen_with(layout::body(app.size), extra)
}

/// Rebuilds the rows and highlight for the current file, keeping the cursor on `keep` if it
/// still exists.
pub(super) fn rebuild(app: &mut App, keep: Option<rb_diff::LineId>) {
    let code = viewport(app).code;
    let now = app.state.now.unwrap_or(Timestamp(0));
    let tab_width = app.tab_width;
    let App {
        diff,
        syntax,
        palette,
        ..
    } = app;
    let Some(state) = diff.as_mut() else {
        return;
    };
    state.range = None;
    state.drag = None;
    state.select = None;
    let stale = state.stale;
    let Some(data) = state.data.as_ref() else {
        state.view = FileView::default();
        return;
    };
    let Some(file) = data.files.get(state.file) else {
        state.view = FileView::default();
        return;
    };
    let rows = Rows::build(
        &file.diff,
        Inputs {
            threads: &data.threads,
            drafts: &data.draft.comments,
            now,
            tab_width,
            stale,
        },
        code.width,
    );
    let highlight = match &file.diff.body {
        DiffBody::Text(patch) if patch.is_large() => Highlight::Windowed(HashMap::new()),
        DiffBody::Text(patch) => {
            Highlight::Whole(syntax.0.get_or_insert_with(Highlighter::new).highlight(
                &file.diff.path,
                patch,
                palette.theme(),
                tab_width,
            ))
        }
        DiffBody::Fallback(_) => Highlight::None,
    };
    let cursor = keep
        .and_then(|id| rows.row_of(id))
        .or_else(|| rows.first_line())
        .unwrap_or(0);
    state.view = FileView {
        rows,
        cursor,
        scroll: 0,
        highlight,
    };
    reveal_cursor(state, usize::from(code.height));
    settle(app);
}

/// Highlights the chunks of a windowed file that the viewport now covers.
fn settle(app: &mut App) {
    let height = usize::from(viewport(app).code.height);
    let tab_width = app.tab_width;
    let App {
        diff,
        syntax,
        palette,
        ..
    } = app;
    let Some(state) = diff.as_mut() else {
        return;
    };
    let Some(file) = state.data.as_ref().and_then(|d| d.files.get(state.file)) else {
        return;
    };
    let (Some(patch), Highlight::Windowed(chunks)) =
        (file.diff.parsed(), &mut state.view.highlight)
    else {
        return;
    };
    let view = &state.view.rows;
    for row in diffview::visible(state.view.scroll, height, view.len()) {
        let Some(id) = view.line_id(row) else {
            continue;
        };
        let key = (id.hunk, (id.line as usize / CHUNK_LINES) as u32);
        if chunks.contains_key(&key) {
            continue;
        }
        let Some(chunk) = chunk_patch(patch, key) else {
            continue;
        };
        let name = format!("{}:{}~/{}", key.0, key.1, file.diff.path);
        let done = syntax.0.get_or_insert_with(Highlighter::new).highlight(
            &name,
            &chunk,
            palette.theme(),
            tab_width,
        );
        chunks.insert(key, done);
    }
}

fn chunk_patch(patch: &ParsedPatch, (hunk, chunk): (u32, u32)) -> Option<ParsedPatch> {
    let source = patch.hunks.get(hunk as usize)?;
    let start = chunk as usize * CHUNK_LINES;
    let lines = source
        .lines
        .get(start..(start + CHUNK_LINES).min(source.lines.len()))?
        .to_vec();
    let first = lines.first()?;
    Some(ParsedPatch {
        hunks: vec![Hunk {
            header: HunkHeader {
                old_start: first.old_no.unwrap_or(0),
                old_len: lines.len() as u32,
                new_start: first.new_no.unwrap_or(0),
                new_len: lines.len() as u32,
                section: String::new(),
            },
            header_position: 0,
            lines,
        }],
    })
}

pub fn on_resize(app: &mut App) {
    let Some(state) = &app.diff else {
        return;
    };
    let keep = state.view.rows.line_id(state.view.cursor);
    let scroll = state.view.scroll;
    rebuild(app, keep);
    let height = usize::from(viewport(app).code.height);
    if let Some(state) = app.diff.as_mut() {
        let total = state.view.rows.len();
        state.view.scroll = scroll.min(diffview::max_scroll(total, height));
        reveal_cursor(state, height);
    }
    settle(app);
}

pub fn refresh_theme(app: &mut App) {
    if let Some(state) = &app.diff {
        let keep = state.view.rows.line_id(state.view.cursor);
        let scroll = state.view.scroll;
        rebuild(app, keep);
        if let Some(state) = app.diff.as_mut() {
            state.view.scroll = scroll;
        }
        settle(app);
    }
}

pub(super) fn reveal_cursor(state: &mut DiffState, height: usize) {
    let view = &mut state.view;
    let (top, bottom) = view.rows.reveal_span(view.cursor);
    view.scroll = diffview::reveal(view.scroll, top, bottom, height, view.rows.len());
}

pub(super) fn select_file(app: &mut App, index: usize) {
    let height = usize::from(viewport(app).file_list.height);
    let Some(state) = app.diff.as_mut() else {
        return;
    };
    if index >= state.files().len() || index == state.file {
        return;
    }
    state.file = index;
    state.files_scroll = diffview::reveal(
        state.files_scroll,
        index,
        index,
        height,
        state.files().len(),
    );
    rebuild(app, None);
}

fn step_file(app: &mut App, forward: bool) -> Vec<Cmd> {
    let Some(state) = &app.diff else {
        return Vec::new();
    };
    match rb_diff::adjacent_file(state.file, state.files().len(), forward) {
        Some(next) => {
            select_file(app, next);
            Vec::new()
        }
        None => {
            let text = if forward {
                "That's the last file."
            } else {
                "That's the first file."
            };
            set_status(app, Notice::new(NoticeKind::Info, text))
        }
    }
}

fn move_cursor(app: &mut App, to: impl FnOnce(&Rows, usize) -> Option<usize>) {
    let height = usize::from(viewport(app).code.height);
    let Some(state) = app.diff.as_mut() else {
        return;
    };
    if let Some(row) = to(&state.view.rows, state.view.cursor) {
        state.view.cursor = row;
        reveal_cursor(state, height);
    }
}

fn page(app: &mut App, forward: bool, fraction: usize) {
    let height = usize::from(viewport(app).code.height);
    let rows = (height / fraction).max(1);
    move_cursor(app, |rows_view, cursor| {
        let last = rows_view.len().checked_sub(1)?;
        if forward {
            let target = (cursor + rows).min(last);
            rows_view.nearest_line(target).or(rows_view.last_line())
        } else {
            let target = cursor.saturating_sub(rows);
            rows_view.line_from(target)
        }
    });
}

pub fn on_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    let Some(focus) = app.diff.as_ref().map(|s| s.focus) else {
        return Vec::new();
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let mut cmds = Vec::new();
    match key.code {
        KeyCode::Char('d') if ctrl => page(app, true, 2),
        KeyCode::Char('u') if ctrl => page(app, false, 2),
        _ if ctrl || key.modifiers.contains(KeyModifiers::ALT) => return Vec::new(),
        KeyCode::Esc
            if app
                .diff
                .as_ref()
                .is_some_and(|s| s.range.is_some() || s.select.is_some()) =>
        {
            if let Some(state) = app.diff.as_mut() {
                state.range = None;
                state.select = None;
            }
        }
        KeyCode::Esc | KeyCode::Char('q') => return close(app),
        KeyCode::Tab | KeyCode::BackTab => cycle_focus(app, key.code == KeyCode::BackTab),
        KeyCode::Enter if focus == DiffFocus::Files => {
            if let Some(state) = app.diff.as_mut() {
                state.focus = DiffFocus::Diff;
            }
        }
        KeyCode::Enter if focus == DiffFocus::Review => {
            let n = app.diff.as_ref().map_or(0, |s| s.review_sel);
            return comments::jump_entry(app, n);
        }
        KeyCode::Enter if focus == DiffFocus::Diff => return comments::edit_at_cursor(app),
        KeyCode::Char('e') if focus == DiffFocus::Review => return comments::edit_entry(app),
        KeyCode::Char('d') | KeyCode::Delete if focus == DiffFocus::Review => {
            return comments::delete_entry(app)
        }
        KeyCode::Char('e') if focus == DiffFocus::Diff => return comments::edit_at_cursor(app),
        KeyCode::Char('d') | KeyCode::Delete if focus == DiffFocus::Diff => {
            return comments::delete_at_cursor(app)
        }
        KeyCode::Char(',') if focus == DiffFocus::Diff => return comments::step_pick(app, false),
        KeyCode::Char('.') if focus == DiffFocus::Diff => return comments::step_pick(app, true),
        KeyCode::Char('V') if focus == DiffFocus::Diff => toggle_select(app),
        KeyCode::Right | KeyCode::Char(']') => cmds = step_file(app, true),
        KeyCode::Left | KeyCode::Char('[') => cmds = step_file(app, false),
        KeyCode::Char('c') => {
            let cmds = composer::open_comment(app);
            end_select(app);
            return cmds;
        }
        KeyCode::Char('r') => return composer::open_reply(app),
        KeyCode::Char('a') => return review::open(app, Verdict::Approve),
        KeyCode::Char('x') => return review::open(app, Verdict::RequestChanges),
        KeyCode::Char('R') => return review::open(app, Verdict::Comment),
        code => match move_kind(code) {
            Some(kind) => cmds = on_move(app, focus, kind, shift),
            None => return Vec::new(),
        },
    }
    settle(app);
    app.mark_dirty();
    cmds
}

/// Tab and ⇧Tab: Files, Diff, and the pending-comment list when there is one.
fn cycle_focus(app: &mut App, back: bool) {
    let Some(state) = app.diff.as_mut() else {
        return;
    };
    let mut order = vec![DiffFocus::Files, DiffFocus::Diff];
    if !comments::entries(state).is_empty() {
        order.push(DiffFocus::Review);
    }
    let at = order.iter().position(|f| *f == state.focus).unwrap_or(0);
    let next = if back {
        (at + order.len() - 1) % order.len()
    } else {
        (at + 1) % order.len()
    };
    state.focus = order[next];
}

#[derive(Debug, Clone, Copy)]
enum Move {
    Line(isize),
    Page(bool),
    Jump(bool),
    Hunk(bool),
}

fn move_kind(code: KeyCode) -> Option<Move> {
    Some(match code {
        KeyCode::Down | KeyCode::Char('j') => Move::Line(1),
        KeyCode::Up | KeyCode::Char('k') => Move::Line(-1),
        KeyCode::PageDown => Move::Page(true),
        KeyCode::PageUp => Move::Page(false),
        KeyCode::Char('g') | KeyCode::Home => Move::Jump(false),
        KeyCode::Char('G') | KeyCode::End => Move::Jump(true),
        KeyCode::Char('n' | '}') => Move::Hunk(true),
        KeyCode::Char('p' | '{') => Move::Hunk(false),
        _ => return None,
    })
}

/// Moves the cursor. In the diff pane the move extends the range when Shift is held with an
/// arrow or page key, or while a `V` selection is on; any other move drops the range, as a
/// click does.
fn on_move(app: &mut App, focus: DiffFocus, kind: Move, shift: bool) -> Vec<Cmd> {
    if let (DiffFocus::Files, Move::Line(delta)) = (focus, kind) {
        return step_file(app, delta > 0);
    }
    if focus == DiffFocus::Review {
        match kind {
            Move::Line(delta) => comments::move_selection(app, delta),
            Move::Jump(end) => {
                comments::move_selection(app, if end { isize::MAX } else { isize::MIN })
            }
            _ => {}
        }
        return Vec::new();
    }
    let Some(state) = app.diff.as_ref() else {
        return Vec::new();
    };
    let shifted = shift && matches!(kind, Move::Line(_) | Move::Page(_));
    let extending = shifted || state.select.is_some();
    let anchor = state
        .select
        .or(state.range.map(|r| r.anchor))
        .unwrap_or(state.view.cursor);
    match kind {
        Move::Line(delta) => move_cursor(app, |rows, c| Some(rows.step(c, delta))),
        Move::Page(forward) => page(app, forward, 1),
        Move::Jump(end) => jump(app, focus, end),
        Move::Hunk(forward) => move_cursor(app, |rows, c| {
            if forward {
                rows.next_hunk(c)
            } else {
                rows.prev_hunk(c)
            }
        }),
    }
    if let Some(state) = app.diff.as_mut() {
        if extending {
            clamp_to_hunk(state, anchor);
            state.range = RowRange::between(anchor, state.view.cursor);
        } else {
            state.range = None;
        }
    }
    Vec::new()
}

/// Pulls the cursor back toward `anchor` until both sit in one hunk: a range never spans two.
fn clamp_to_hunk(state: &mut DiffState, anchor: usize) {
    let rows = &state.view.rows;
    let Some(hunk) = rows.line_id(anchor).map(|id| id.hunk) else {
        return;
    };
    let mut head = state.view.cursor;
    while rows.line_id(head).is_some_and(|id| id.hunk != hunk) {
        let next = rows.step(head, if head > anchor { -1 } else { 1 });
        if next == head {
            break;
        }
        head = next;
    }
    state.view.cursor = head;
}

fn toggle_select(app: &mut App) {
    let Some(state) = app.diff.as_mut() else {
        return;
    };
    if state.select.take().is_none() {
        state.select = Some(state.range.map_or(state.view.cursor, |r| r.anchor));
    }
}

fn end_select(app: &mut App) {
    if let Some(state) = app.diff.as_mut() {
        state.select = None;
    }
}

fn jump(app: &mut App, focus: DiffFocus, to_end: bool) {
    match focus {
        DiffFocus::Diff => move_cursor(app, |rows, _| {
            if to_end {
                rows.last_line()
            } else {
                rows.first_line()
            }
        }),
        DiffFocus::Files => {
            let last = app
                .diff
                .as_ref()
                .map_or(0, |s| s.files().len().saturating_sub(1));
            select_file(app, if to_end { last } else { 0 });
        }
        DiffFocus::Review => {}
    }
}

pub fn on_action(app: &mut App, action: Action) -> Vec<Cmd> {
    match action {
        Action::CloseDiff => return close(app),
        Action::DiffFile(index) => {
            if let Some(state) = app.diff.as_mut() {
                state.focus = DiffFocus::Files;
            }
            select_file(app, index);
        }
        Action::DiffRow(row) => {
            if let Some(state) = app.diff.as_mut() {
                state.focus = DiffFocus::Diff;
                state.range = None;
                state.select = None;
            }
            move_cursor(app, |rows, _| rows.nearest_line(row));
        }
        Action::DiffFocus(focus) => {
            if let Some(state) = app.diff.as_mut() {
                state.focus = focus;
            }
        }
        _ => return Vec::new(),
    }
    settle(app);
    app.mark_dirty();
    Vec::new()
}

/// A left press on a diff row: places the cursor and starts a drag there. With shift held it
/// extends the range from the cursor (or the range's anchor) to the row instead.
pub fn on_press(app: &mut App, row: usize, extend: bool) -> Vec<Cmd> {
    if !extend {
        let cmds = on_action(app, Action::DiffRow(row));
        if let Some(state) = app.diff.as_mut() {
            state.drag = Some(state.view.cursor);
        }
        return cmds;
    }
    let Some(state) = app.diff.as_mut() else {
        return Vec::new();
    };
    let Some(head) = state.view.rows.nearest_line(row) else {
        return Vec::new();
    };
    let anchor = state.range.map_or(state.view.cursor, |r| r.anchor);
    state.focus = DiffFocus::Diff;
    state.range = RowRange::between(anchor, head);
    state.drag = None;
    move_cursor(app, |_, _| Some(head));
    settle(app);
    app.mark_dirty();
    Vec::new()
}

/// The pointer moved with the left button held: grows the range to the line under it, and
/// scrolls when the pointer is past the top or bottom of the code.
pub fn on_drag(app: &mut App, y: u16) {
    let code = viewport(app).code;
    let Some(state) = app.diff.as_mut() else {
        return;
    };
    let Some(anchor) = state.drag else {
        return;
    };
    let view = &mut state.view;
    let height = usize::from(code.height);
    let max = diffview::max_scroll(view.rows.len(), height);
    match range::edge_scroll(y, code.y, code.height) {
        -1 => view.scroll = view.scroll.saturating_sub(1),
        1 => view.scroll = (view.scroll + 1).min(max),
        _ => {}
    }
    let Some(row) = range::row_at(y, code.y, code.height, view.scroll, view.rows.len()) else {
        return;
    };
    let Some(head) = view.rows.nearest_line(row) else {
        return;
    };
    state.range = RowRange::between(anchor, head);
    state.view.cursor = head;
    settle(app);
    app.mark_dirty();
}

pub fn on_release(app: &mut App) {
    if let Some(state) = app.diff.as_mut() {
        state.drag = None;
    }
}

pub fn on_scroll(app: &mut App, column: u16, row: u16, down: bool) {
    let l = viewport(app);
    let at = ratatui::layout::Position::new(column, row);
    let height = usize::from(l.code.height);
    let Some(state) = app.diff.as_mut() else {
        return;
    };
    if l.file_list.contains(at) {
        let max = diffview::max_scroll(state.files().len(), usize::from(l.file_list.height));
        state.files_scroll = if down {
            (state.files_scroll + 1).min(max)
        } else {
            state.files_scroll.saturating_sub(1)
        };
    } else if l.diff.contains(at) {
        let view = &mut state.view;
        let max = diffview::max_scroll(view.rows.len(), height);
        view.scroll = if down {
            (view.scroll + WHEEL_ROWS).min(max)
        } else {
            view.scroll.saturating_sub(WHEEL_ROWS)
        };
        if view.cursor < view.scroll {
            view.cursor = view.rows.line_from(view.scroll).unwrap_or(view.cursor);
        } else if view.cursor >= view.scroll + height {
            view.cursor = view
                .rows
                .line_before(view.scroll + height)
                .unwrap_or(view.cursor);
        }
    } else {
        return;
    }
    settle(app);
    app.mark_dirty();
}

impl App {
    pub fn diff_state(&self) -> Option<&DiffState> {
        self.diff.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::diffview::tests::{draft, thread, PATCH};
    use crate::app::{update, AppConfig, Msg};
    use crossterm::event::KeyEvent;
    use rb_core::{FileStatus, ForgeKind, Side, SourceId};
    use rb_theme::ColourDepth;

    fn id() -> ChangeId {
        ChangeId {
            source_id: SourceId("s".into()),
            kind: ForgeKind::GitHub,
            repo: "o/r".into(),
            number: 1,
        }
    }

    fn patch(path: &str, body: Option<&str>) -> FilePatch {
        FilePatch {
            path: path.into(),
            old_path: None,
            status: FileStatus::Modified,
            adds: 2,
            dels: 1,
            patch: body.map(str::to_string),
        }
    }

    fn app_with(files: Vec<FilePatch>) -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        app.diff = Some(DiffState::loading(id()));
        app.screen = Screen::Diff;
        let mut a_thread = thread(2, Side::New);
        a_thread.path = Some("src/a.rs".into());
        let data = DiffData::new(
            files,
            vec![a_thread],
            ReviewDraft {
                body: String::new(),
                comments: vec![draft(4, "Plain.")],
            },
        );
        update(
            &mut app,
            Msg::DiffLoaded {
                id: id(),
                result: Ok(Box::new(data)),
            },
        );
        app
    }

    fn two_files() -> App {
        app_with(vec![
            patch("src/a.rs", Some(PATCH)),
            patch("src/b.rs", Some(PATCH)),
            patch("img.png", None),
        ])
    }

    fn press(app: &mut App, c: char) {
        update(app, Msg::Key(KeyEvent::from(KeyCode::Char(c))));
    }

    fn line(app: &App) -> (u32, u32) {
        let s = app.diff.as_ref().unwrap();
        let id = s.view.rows.line_id(s.view.cursor).expect("on a line");
        (id.hunk, id.line)
    }

    fn cursor(app: &App) -> usize {
        app.diff.as_ref().unwrap().view.cursor
    }

    #[test]
    fn opening_a_change_starts_on_its_diff_and_asks_for_the_patches() {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        let cmds = open_change(&mut app, &id());
        assert!(matches!(cmds.as_slice(), [Cmd::LoadDiff(got)] if *got == id()));
        assert_eq!(app.screen, Screen::Diff);
        assert_eq!(app.diff.as_ref().unwrap().phase, Phase::Loading);
    }

    #[test]
    fn loading_then_loaded_lands_on_the_first_line() {
        let app = two_files();
        let s = app.diff.as_ref().unwrap();
        assert_eq!(s.phase, Phase::Ready);
        assert_eq!(s.view.cursor, 1);
        assert!(s.view.rows.block(0).is_some(), "the thread is placed");
        assert!(matches!(s.view.highlight, Highlight::Whole(_)));
        assert_eq!(s.files().len(), 3);
    }

    fn loaded_with_intent(intent: Intent) -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        let mut state = DiffState::loading(id());
        state.intent = Some(intent);
        app.diff = Some(state);
        app.screen = Screen::Diff;
        let data = DiffData::new(
            vec![patch("src/a.rs", Some(PATCH))],
            Vec::new(),
            ReviewDraft::default(),
        );
        update(
            &mut app,
            Msg::DiffLoaded {
                id: id(),
                result: Ok(Box::new(data)),
            },
        );
        app
    }

    #[test]
    fn a_dashboard_approve_previews_once_the_diff_loads() {
        let app = loaded_with_intent(Intent::Approve);
        let s = app.diff.as_ref().unwrap();
        assert_eq!(s.review.as_ref().map(|m| m.verdict), Some(Verdict::Approve));
        assert!(s.intent.is_none());
    }

    #[test]
    fn a_dashboard_request_changes_opens_the_modal_on_that_verdict() {
        let app = loaded_with_intent(Intent::RequestChanges);
        let s = app.diff.as_ref().unwrap();
        assert_eq!(
            s.review.as_ref().map(|m| m.verdict),
            Some(Verdict::RequestChanges)
        );
        assert!(s.intent.is_none());
    }

    #[test]
    fn a_dashboard_comment_opens_the_composer_on_the_first_changed_line() {
        let app = loaded_with_intent(Intent::Comment);
        let s = app.diff.as_ref().unwrap();
        let composer = s.composer.as_ref().expect("the composer is open");
        let composer::Target::Line(anchor) = &composer.target else {
            panic!("expected a line target");
        };
        assert_eq!((anchor.side, anchor.line), (rb_core::Side::Old, 2));
    }

    #[test]
    fn failed_loads_keep_the_message() {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        app.diff = Some(DiffState::loading(id()));
        app.screen = Screen::Diff;
        update(
            &mut app,
            Msg::DiffLoaded {
                id: id(),
                result: Err("The forge didn't answer.".into()),
            },
        );
        assert_eq!(
            app.diff.as_ref().unwrap().phase,
            Phase::Failed("The forge didn't answer.".into())
        );
    }

    #[test]
    fn a_stale_result_is_dropped() {
        let mut app = two_files();
        let mut other = id();
        other.number = 2;
        update(
            &mut app,
            Msg::DiffLoaded {
                id: other,
                result: Err("nope".into()),
            },
        );
        assert_eq!(app.diff.as_ref().unwrap().phase, Phase::Ready);
    }

    #[test]
    fn j_and_k_move_across_hunks_and_the_thread_block() {
        let mut app = two_files();
        for _ in 0..4 {
            press(&mut app, 'j');
        }
        assert_eq!(line(&app), (0, 4), "steps over the thread blocks");
        press(&mut app, 'j');
        assert_eq!(line(&app), (1, 0), "jumps the second hunk's header");
        press(&mut app, 'k');
        assert_eq!(line(&app), (0, 4));
    }

    #[test]
    fn n_and_p_jump_between_hunks() {
        let mut app = two_files();
        let first = cursor(&app);
        press(&mut app, 'n');
        let second = cursor(&app);
        assert!(second > first);
        press(&mut app, 'n');
        assert_eq!(cursor(&app), second, "no further hunk");
        press(&mut app, 'p');
        assert_eq!(cursor(&app), first);
        press(&mut app, '}');
        assert_eq!(cursor(&app), second);
        press(&mut app, '{');
        assert_eq!(cursor(&app), first);
    }

    #[test]
    fn brackets_change_files_and_stop_at_the_ends() {
        let mut app = two_files();
        press(&mut app, '[');
        assert_eq!(app.diff.as_ref().unwrap().file, 0);
        press(&mut app, ']');
        assert_eq!(app.diff.as_ref().unwrap().file, 1);
        press(&mut app, ']');
        press(&mut app, ']');
        let s = app.diff.as_ref().unwrap();
        assert_eq!(s.file, 2);
        assert!(s.view.rows.is_empty(), "the binary file has no rows");
        assert!(matches!(s.view.highlight, Highlight::None));
    }

    #[test]
    fn g_and_capital_g_and_paging_move_the_cursor() {
        let mut app = two_files();
        press(&mut app, 'G');
        let last = app.diff.as_ref().unwrap().view.rows.last_line().unwrap();
        assert_eq!(cursor(&app), last);
        press(&mut app, 'g');
        assert_eq!(cursor(&app), 1);
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::PageDown)));
        assert_eq!(cursor(&app), last);
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::PageUp)));
        assert_eq!(cursor(&app), 1);
        update(
            &mut app,
            Msg::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        );
        assert!(cursor(&app) > 1);
        update(
            &mut app,
            Msg::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
        );
        assert_eq!(cursor(&app), 1);
    }

    #[test]
    fn tab_moves_focus_and_j_then_chooses_files() {
        let mut app = two_files();
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Tab)));
        assert_eq!(
            app.diff.as_ref().unwrap().focus,
            DiffFocus::Review,
            "a pending comment adds the list to the cycle"
        );
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Tab)));
        assert_eq!(app.diff.as_ref().unwrap().focus, DiffFocus::Files);
        press(&mut app, 'j');
        assert_eq!(app.diff.as_ref().unwrap().file, 1);
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Enter)));
        assert_eq!(app.diff.as_ref().unwrap().focus, DiffFocus::Diff);
    }

    #[test]
    fn esc_and_q_return_to_the_dashboard_without_quitting() {
        let mut app = two_files();
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Esc)));
        assert_eq!(app.screen, Screen::Dashboard);
        assert!(app.diff.is_none());
        assert!(!app.should_quit());
        app.diff = Some(DiffState::loading(id()));
        app.screen = Screen::Diff;
        press(&mut app, 'q');
        assert_eq!(app.screen, Screen::Dashboard);
        assert!(!app.should_quit());
        app.drafts.discard(&id());
        press(&mut app, 'q');
        assert!(app.should_quit());
    }

    #[test]
    fn clicks_pick_files_and_lines() {
        let mut app = two_files();
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Char('G'))));
        let cmds = update_action(&mut app, Action::DiffFile(1));
        assert!(cmds.is_empty());
        let s = app.diff.as_ref().unwrap();
        assert_eq!((s.file, s.focus), (1, DiffFocus::Files));
        update_action(&mut app, Action::DiffRow(0));
        let s = app.diff.as_ref().unwrap();
        assert_eq!((s.view.cursor, s.focus), (1, DiffFocus::Diff));
    }

    fn update_action(app: &mut App, action: Action) -> Vec<Cmd> {
        on_action(app, action)
    }

    #[test]
    fn wheel_scrolls_the_viewport_and_keeps_the_cursor_inside() {
        let mut big = String::from("@@ -1,200 +1,200 @@\n");
        for i in 0..200 {
            big.push_str(&format!(" l{i}\n"));
        }
        let mut app = app_with(vec![patch("src/big.rs", Some(&big))]);
        on_scroll(&mut app, 100, 10, true);
        let s = app.diff.as_ref().unwrap();
        assert_eq!(s.view.scroll, WHEEL_ROWS);
        assert!(s.view.cursor >= s.view.scroll);
        on_scroll(&mut app, 100, 10, false);
        assert_eq!(app.diff.as_ref().unwrap().view.scroll, 0);
        on_scroll(&mut app, 0, 0, true);
        assert_eq!(
            app.diff.as_ref().unwrap().view.scroll,
            0,
            "outside both panes"
        );
    }

    #[test]
    fn big_files_highlight_only_the_chunks_in_view() {
        let mut big = String::from("@@ -1,4000 +1,4000 @@\n");
        for i in 0..4000 {
            big.push_str(&format!(" let v{i} = {i};\n"));
        }
        let mut app = app_with(vec![patch("src/big.rs", Some(&big))]);
        let s = app.diff.as_ref().unwrap();
        assert!(matches!(s.view.highlight, Highlight::Windowed(_)));
        assert_eq!(s.view.chunks(), 1, "just the first window");
        assert!(s.view.spans(LineId { hunk: 0, line: 0 }).is_some());
        assert!(s
            .view
            .spans(LineId {
                hunk: 0,
                line: 3000
            })
            .is_none());
        press(&mut app, 'G');
        let s = app.diff.as_ref().unwrap();
        assert_eq!(s.view.chunks(), 2, "the end of the file adds one more");
        let tail = s
            .view
            .spans(LineId {
                hunk: 0,
                line: 3999,
            })
            .expect("highlighted");
        assert!(
            tail.iter().any(|p| p.role.is_some()),
            "keywords are coloured"
        );
    }

    #[test]
    fn resize_rebuilds_rows_but_keeps_the_cursor_line() {
        let mut app = two_files();
        press(&mut app, 'j');
        press(&mut app, 'j');
        let before = app
            .diff
            .as_ref()
            .and_then(|s| s.view.rows.line_id(s.view.cursor));
        update(&mut app, Msg::Resize(100, 30));
        let s = app.diff.as_ref().unwrap();
        assert_eq!(s.view.rows.line_id(s.view.cursor), before);
    }

    #[test]
    fn theme_change_keeps_position() {
        let mut app = two_files();
        press(&mut app, 'j');
        let at = cursor(&app);
        press(&mut app, 'T');
        assert_eq!(cursor(&app), at);
        assert!(matches!(
            app.diff.as_ref().unwrap().view.highlight,
            Highlight::Whole(_)
        ));
    }

    fn tap(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Vec<Cmd> {
        update(app, Msg::Key(KeyEvent::new(code, modifiers)))
    }

    fn range(app: &App) -> Option<(usize, usize)> {
        app.diff.as_ref().unwrap().range.map(RowRange::bounds)
    }

    #[test]
    fn shift_down_anchors_at_the_cursor_and_extends() {
        let mut app = two_files();
        let start = cursor(&app);
        tap(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
        assert_eq!(range(&app), Some((start, cursor(&app))));
        tap(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
        let (top, bottom) = range(&app).unwrap();
        assert_eq!(top, start, "the anchor stays put");
        assert_eq!(bottom, cursor(&app));
        assert_eq!(app.diff.as_ref().unwrap().range.unwrap().anchor, start);
    }

    #[test]
    fn shift_up_extends_upward_and_shrinking_back_to_the_anchor_clears_it() {
        let mut app = two_files();
        press(&mut app, 'j');
        press(&mut app, 'j');
        let start = cursor(&app);
        tap(&mut app, KeyCode::Up, KeyModifiers::SHIFT);
        let (top, bottom) = range(&app).unwrap();
        assert_eq!((top, bottom), (cursor(&app), start));
        tap(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
        assert_eq!(range(&app), None, "one line is just the cursor");
    }

    #[test]
    fn a_plain_move_clears_the_range_like_a_click_does() {
        let mut app = two_files();
        tap(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
        assert!(range(&app).is_some());
        press(&mut app, 'j');
        assert_eq!(range(&app), None);
    }

    #[test]
    fn shift_page_extends_by_a_page_and_stays_in_one_hunk() {
        let mut app = two_files();
        tap(&mut app, KeyCode::PageDown, KeyModifiers::SHIFT);
        let s = app.diff.as_ref().unwrap();
        let (top, bottom) = s.range.unwrap().bounds();
        let first = s.view.rows.line_id(top).unwrap().hunk;
        let last = s.view.rows.line_id(bottom).unwrap().hunk;
        assert_eq!(first, last, "the range stops at the end of the hunk");
        assert_eq!(bottom, s.view.cursor);
    }

    #[test]
    fn a_range_does_not_cross_into_the_next_hunk() {
        let mut app = two_files();
        for _ in 0..20 {
            tap(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
        }
        let s = app.diff.as_ref().unwrap();
        let (top, bottom) = s.range.unwrap().bounds();
        assert_eq!(s.view.rows.line_id(top).unwrap().hunk, 0);
        assert_eq!(s.view.rows.line_id(bottom).unwrap().hunk, 0);
    }

    #[test]
    fn v_starts_a_selection_that_plain_moves_extend_until_v_or_esc() {
        let mut app = two_files();
        let start = cursor(&app);
        press(&mut app, 'V');
        assert_eq!(
            range(&app),
            None,
            "nothing is selected until the cursor moves"
        );
        press(&mut app, 'j');
        press(&mut app, 'j');
        assert_eq!(range(&app), Some((start, cursor(&app))));
        press(&mut app, 'k');
        assert_eq!(range(&app).map(|r| r.0), Some(start));
        press(&mut app, 'V');
        let kept = range(&app);
        assert!(kept.is_some(), "ending the mode keeps the range for c");
        press(&mut app, 'j');
        assert_eq!(range(&app), None, "a plain move after that clears it");

        press(&mut app, 'V');
        press(&mut app, 'j');
        assert!(range(&app).is_some());
        tap(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(range(&app), None);
        assert_eq!(app.diff.as_ref().unwrap().select, None);
        assert_eq!(
            app.screen,
            Screen::Diff,
            "esc cleared first and didn't leave"
        );
    }

    #[test]
    fn a_keyboard_range_comments_on_the_whole_range() {
        let mut app = two_files();
        press(&mut app, 'V');
        press(&mut app, 'j');
        press(&mut app, 'j');
        press(&mut app, 'c');
        let s = app.diff.as_ref().unwrap();
        let composer = s.composer.as_ref().expect("the composer opens");
        let composer::Target::Line(anchor) = &composer.target else {
            panic!("a line target");
        };
        assert!(anchor.start_line.is_some());
        assert_eq!(s.select, None, "the mode ends when the composer opens");
    }

    #[test]
    fn right_and_left_flip_files_and_say_so_at_the_ends() {
        let mut app = two_files();
        let status = |app: &App| app.status.as_ref().map(|e| e.notice.text.clone());
        tap(&mut app, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(status(&app).as_deref(), Some("That's the first file."));
        tap(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(app.diff.as_ref().unwrap().file, 1);
        tap(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(app.diff.as_ref().unwrap().file, 2);
        tap(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(status(&app).as_deref(), Some("That's the last file."));
        tap(&mut app, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(app.diff.as_ref().unwrap().file, 1);
        press(&mut app, '[');
        assert_eq!(app.diff.as_ref().unwrap().file, 0);
    }

    #[test]
    fn arrows_belong_to_the_composer_while_it_is_open() {
        let mut app = two_files();
        press(&mut app, 'c');
        assert!(app.diff.as_ref().unwrap().composer.is_some());
        tap(&mut app, KeyCode::Right, KeyModifiers::NONE);
        tap(&mut app, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(app.diff.as_ref().unwrap().file, 0);
    }

    #[cfg(unix)]
    #[test]
    fn ctrl_z_asks_the_runtime_to_suspend_on_every_screen() {
        let mut app = two_files();
        let cmds = tap(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(matches!(cmds.as_slice(), [Cmd::Suspend]));
        press(&mut app, 'c');
        press(&mut app, 'h');
        let cmds = tap(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(matches!(cmds.as_slice(), [Cmd::Suspend]));
        let composer = app.diff.as_ref().unwrap().composer.as_ref().unwrap();
        assert_eq!(composer.editor.text(), "h", "unsent text is kept");
        app.help = true;
        let cmds = tap(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(matches!(cmds.as_slice(), [Cmd::Suspend]));
    }
}
