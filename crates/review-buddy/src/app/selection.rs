//! Text selection: a drag, a double- or triple-click, or the keyboard copy mode picks text out of
//! one region of the screen (see [`crate::ui::textmap`]), highlights it, and copies the logical
//! text. The geometry here is pure: positions are a row number and a cell column inside the
//! region the selection started in, so scrolling during a drag cannot move them.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Action, App, Cmd, DiffFocus, Notice, NoticeKind, Screen};
use crate::ui::textmap::{Join, RegionKey, TextMap, TextRegion, TextRow};

/// A place in a region: the row's `ord` and a cell column in its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub ord: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(ord: usize, col: usize) -> Self {
        Self { ord, col }
    }
}

/// How much text one click or one step of a drag takes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Char,
    Word,
    Line,
}

/// The most rows (in screen rows) beyond the view that the diff records while a selection is on,
/// so a selection can grow past the edge before the view catches up.
pub const LOOKAHEAD: usize = 64;

#[derive(Debug, Clone)]
pub struct Selection {
    pub key: RegionKey,
    pub rect: Rect,
    pub anchor: Pos,
    pub head: Pos,
    pub unit: Unit,
    /// The first word or line picked, which stays selected whichever way the drag goes.
    base: (Pos, Pos),
    /// The button is still down.
    pub dragging: bool,
    /// Started with `v`: the keys move the end.
    pub keyboard: bool,
    seen: BTreeMap<usize, TextRow>,
    print: u64,
}

impl Selection {
    pub fn new(region: &TextRegion, at: Pos, unit: Unit) -> Self {
        let mut sel = Self {
            key: region.key,
            rect: region.rect,
            anchor: at,
            head: at,
            unit,
            base: (at, at),
            dragging: false,
            keyboard: false,
            seen: BTreeMap::new(),
            print: 0,
        };
        sel.take(region);
        sel.base = sel.span_of(at);
        sel
    }

    fn take(&mut self, region: &TextRegion) {
        self.rect = region.rect;
        for row in &region.rows {
            self.seen.insert(row.ord, row.clone());
        }
    }

    /// Remembers the rows of the latest frame, so text that scrolled out of view is still there
    /// to copy.
    pub fn absorb(&mut self, map: &TextMap) {
        if let Some(region) = map.region(self.key) {
            self.take(region);
        }
    }

    pub fn row(&self, ord: usize) -> Option<&TextRow> {
        self.seen.get(&ord)
    }

    fn next_ord(&self, ord: usize) -> Option<usize> {
        self.seen.range(ord + 1..).next().map(|(&o, _)| o)
    }

    fn prev_ord(&self, ord: usize) -> Option<usize> {
        self.seen.range(..ord).next_back().map(|(&o, _)| o)
    }

    /// The first and last row of the source line that row `ord` belongs to: soft-wrapped rows
    /// belong to the row they continue.
    fn line_of(&self, ord: usize) -> (usize, usize) {
        let mut first = ord;
        while self.row(first).is_some_and(|r| r.join != Join::Break) {
            match self.prev_ord(first) {
                Some(prev) => first = prev,
                None => break,
            }
        }
        let mut last = ord;
        while let Some(next) = self.next_ord(last) {
            if self.row(next).is_some_and(|r| r.join == Join::Break) {
                break;
            }
            last = next;
        }
        (first, last)
    }

    /// The first and last place the unit under `at` covers, both included.
    fn span_of(&self, at: Pos) -> (Pos, Pos) {
        match self.unit {
            Unit::Char => (at, at),
            Unit::Word => match self.row(at.ord) {
                Some(row) => {
                    let (a, b) = word_at(&row.text, at.col);
                    (Pos::new(at.ord, a), Pos::new(at.ord, b))
                }
                None => (at, at),
            },
            Unit::Line => {
                let (first, last) = self.line_of(at.ord);
                (Pos::new(first, 0), Pos::new(last, usize::MAX))
            }
        }
    }

    /// The two ends in reading order, both included.
    pub fn bounds(&self) -> (Pos, Pos) {
        match self.unit {
            Unit::Char => (self.anchor.min(self.head), self.anchor.max(self.head)),
            _ => {
                let head = self.span_of(self.head);
                (self.base.0.min(head.0), self.base.1.max(head.1))
            }
        }
    }

    /// Nothing is selected yet: a click that has not moved.
    pub fn is_empty(&self) -> bool {
        self.unit == Unit::Char && !self.keyboard && self.anchor == self.head
    }

    /// The columns of row `ord` that are selected, as a half-open range, before the text is
    /// consulted.
    fn raw_range(&self, ord: usize) -> Option<(usize, usize)> {
        let (start, end) = self.bounds();
        if ord < start.ord || ord > end.ord {
            return None;
        }
        let from = if ord == start.ord { start.col } else { 0 };
        let to = if ord == end.ord {
            end.col.saturating_add(1)
        } else {
            usize::MAX
        };
        Some((from, to))
    }

    /// The cells of `row` to draw highlighted: half-open, snapped to whole characters.
    pub fn highlight(&self, row: &TextRow) -> Option<(usize, usize)> {
        if self.is_empty() {
            return None;
        }
        let (from, to) = self.raw_range(row.ord)?;
        included(&row.text, from, to)
    }

    /// The logical text: rows joined by newlines, wrapped rows joined as they were written,
    /// trailing spaces trimmed.
    pub fn text(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let (start, end) = self.bounds();
        let mut out = String::new();
        let mut first = true;
        for (&ord, row) in self.seen.range(start.ord..=end.ord) {
            let interior = ord > start.ord && ord < end.ord;
            let Some((from, to)) = self.raw_range(ord) else {
                continue;
            };
            let piece = match &row.copy {
                Some(copy) => included(&row.text, from, to).map(|_| copy.clone()),
                None => included(&row.text, from, to).map(|(a, b)| slice(&row.text, a, b)),
            };
            let piece = match piece {
                Some(piece) => piece,
                None if interior => String::new(),
                None => continue,
            };
            if !first {
                match row.join {
                    Join::Break => {
                        out.truncate(out.trim_end_matches([' ', '\t']).len());
                        out.push('\n');
                    }
                    Join::Space => out.push(' '),
                    Join::Glue => {}
                }
            }
            first = false;
            out.push_str(&piece);
        }
        out.truncate(out.trim_end().len());
        out
    }

    // Keyboard movement. `head` moves; `anchor` stays.

    fn width_of(&self, ord: usize) -> usize {
        self.row(ord)
            .map_or(0, |r| UnicodeWidthStr::width(r.text.as_str()))
    }

    fn last_col(&self, ord: usize) -> usize {
        self.width_of(ord).saturating_sub(1)
    }

    pub fn step_right(&mut self) {
        let Some(row) = self.row(self.head.ord) else {
            return;
        };
        if let Some(next) = graphemes_from(&row.text).find(|g| g.start > self.head.col) {
            self.head.col = next.start;
        }
    }

    pub fn step_left(&mut self) {
        let Some(row) = self.row(self.head.ord) else {
            return;
        };
        if let Some(prev) = graphemes_from(&row.text)
            .take_while(|g| g.start < self.head.col)
            .last()
        {
            self.head.col = prev.start;
        }
    }

    pub fn step_down(&mut self) {
        if let Some(next) = self.next_ord(self.head.ord) {
            self.head = Pos::new(next, self.head.col.min(self.last_col(next)));
        }
    }

    pub fn step_up(&mut self) {
        if let Some(prev) = self.prev_ord(self.head.ord) {
            self.head = Pos::new(prev, self.head.col.min(self.last_col(prev)));
        }
    }

    pub fn line_start(&mut self) {
        self.head.col = 0;
    }

    pub fn line_end(&mut self) {
        self.head.col = self.last_col(self.head.ord);
    }

    /// `w`: the start of the next word, on this row or a later one.
    pub fn word_forward(&mut self) {
        let mut ord = self.head.ord;
        let col = self.head.col;
        // Leave the run under the cursor, then any space after it.
        if let Some(row) = self.row(ord) {
            let gs: Vec<Grapheme> = graphemes_from(&row.text).collect();
            let at = gs.iter().position(|g| g.start >= col).unwrap_or(gs.len());
            let class = gs.get(at).map(|g| g.class);
            let mut i = at;
            while i < gs.len() && Some(gs[i].class) == class && class != Some(Class::Space) {
                i += 1;
            }
            while i < gs.len() && gs[i].class == Class::Space {
                i += 1;
            }
            if let Some(g) = gs.get(i) {
                self.head = Pos::new(ord, g.start);
                return;
            }
        }
        // Nothing more on this row: the first word of a later one.
        while let Some(next) = self.next_ord(ord) {
            ord = next;
            let Some(row) = self.row(ord) else { continue };
            if let Some(g) = graphemes_from(&row.text).find(|g| g.class != Class::Space) {
                self.head = Pos::new(ord, g.start);
                return;
            }
        }
        self.head = Pos::new(ord, self.last_col(ord));
    }

    /// `b`: the start of the word before the cursor.
    pub fn word_back(&mut self) {
        let mut ord = self.head.ord;
        let mut before = Some(self.head.col);
        loop {
            if let Some(row) = self.row(ord) {
                let gs: Vec<Grapheme> = graphemes_from(&row.text).collect();
                let limit = before.unwrap_or(usize::MAX);
                let mut i = gs.iter().take_while(|g| g.start < limit).count();
                while i > 0 && gs[i - 1].class == Class::Space {
                    i -= 1;
                }
                if i > 0 {
                    let class = gs[i - 1].class;
                    while i > 0 && gs[i - 1].class == class {
                        i -= 1;
                    }
                    self.head = Pos::new(ord, gs[i].start);
                    return;
                }
            }
            match self.prev_ord(ord) {
                Some(prev) => {
                    ord = prev;
                    before = None;
                }
                None => {
                    self.head.col = 0;
                    return;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Other,
}

fn class_of(g: &str) -> Class {
    let c = g.chars().next().unwrap_or(' ');
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Other
    }
}

#[derive(Debug, Clone, Copy)]
struct Grapheme {
    /// The first cell.
    start: usize,
    /// Cells taken (at least 1).
    cells: usize,
    class: Class,
}

fn graphemes_from(text: &str) -> std::vec::IntoIter<Grapheme> {
    let mut at = 0;
    let all: Vec<Grapheme> = text
        .graphemes(true)
        .map(|g| {
            let out = Grapheme {
                start: at,
                cells: UnicodeWidthStr::width(g).max(1),
                class: class_of(g),
            };
            at += UnicodeWidthStr::width(g);
            out
        })
        .collect();
    all.into_iter()
}

/// The first and last cell of the word, space run or punctuation run under `col`.
pub fn word_at(text: &str, col: usize) -> (usize, usize) {
    let gs: Vec<Grapheme> = graphemes_from(text).collect();
    let Some(mut lo) = gs
        .iter()
        .position(|g| col < g.start + g.cells)
        .or(gs.len().checked_sub(1))
    else {
        return (0, 0);
    };
    let class = gs[lo].class;
    let mut hi = lo;
    while lo > 0 && gs[lo - 1].class == class {
        lo -= 1;
    }
    while hi + 1 < gs.len() && gs[hi + 1].class == class {
        hi += 1;
    }
    (gs[lo].start, gs[hi].start + gs[hi].cells - 1)
}

/// The cells, snapped to whole characters, that `[from, to)` takes in `text`; `None` when it
/// touches none.
fn included(text: &str, from: usize, to: usize) -> Option<(usize, usize)> {
    let mut found: Option<(usize, usize)> = None;
    for g in graphemes_from(text) {
        let end = g.start + g.cells;
        if g.start < to && end > from {
            found = Some(match found {
                Some((a, _)) => (a, end),
                None => (g.start, end),
            });
        }
    }
    found
}

/// The part of `text` in the cells `[from, to)`, which must lie on character boundaries.
fn slice(text: &str, from: usize, to: usize) -> String {
    let mut at = 0;
    let mut out = String::new();
    for g in text.graphemes(true) {
        if at >= from && at < to {
            out.push_str(g);
        }
        at += UnicodeWidthStr::width(g);
    }
    out
}

/// The column of the cell at `x` in `row`, clamped to the row's text (one past its end when the
/// pointer is beyond it).
fn col_in(row: &TextRow, x: u16) -> usize {
    usize::from(x.saturating_sub(row.x)).min(UnicodeWidthStr::width(row.text.as_str()))
}

/// Maps a pointer at `(x, y)` to a place in `region`: on a row, its cell; above the first row,
/// the start of it; below the last, the end; between two rows, the end of the one above.
pub fn locate(region: &TextRegion, x: u16, y: u16) -> Option<Pos> {
    let shown: Vec<&TextRow> = region.rows.iter().filter(|r| r.on_screen()).collect();
    let (first, last) = (shown.first()?, shown.last()?);
    if let Some(row) = shown.iter().find(|r| r.y == y) {
        return Some(Pos::new(row.ord, col_in(row, x)));
    }
    if y < first.y {
        return Some(Pos::new(first.ord, 0));
    }
    if y > last.y {
        return Some(Pos::new(
            last.ord,
            UnicodeWidthStr::width(last.text.as_str()),
        ));
    }
    let above = shown.iter().rev().find(|r| r.y < y)?;
    Some(Pos::new(
        above.ord,
        UnicodeWidthStr::width(above.text.as_str()),
    ))
}

// What follows touches the app.

/// Two presses this many ticks apart (a tick is 250 ms), on the same row, count as a double-click.
const MULTI_CLICK_TICKS: u64 = 2;

/// What identifies the screen a selection was made on. When it changes the selection no longer
/// means anything and is dropped.
pub fn fingerprint(app: &App) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        format!("{:?}", app.screen),
        app.size,
        app.diff_wrap,
        app.tab_width,
    )
        .hash(&mut h);
    (app.help, app.show.open, app.pending.is_some()).hash(&mut h);
    (app.term.visible, format!("{:?}", app.term.position)).hash(&mut h);
    match app.screen {
        Screen::Dashboard => {
            format!(
                "{:?}{:?}{}",
                app.selected_change().map(|c| &c.id),
                app.dashboard.tab,
                app.layout.detail_open()
            )
            .hash(&mut h);
        }
        Screen::Diff => {
            if let Some(s) = app.diff.as_ref() {
                (
                    s.file,
                    s.view.rows.len(),
                    s.view.screen.total(),
                    s.has_overlay(),
                )
                    .hash(&mut h);
            }
        }
        _ => {}
    }
    h.finish()
}

pub fn clear(app: &mut App) {
    if app.selection.take().is_some() {
        app.mark_dirty();
    }
}

/// Drops the selection when the screen it was made on has changed under it.
pub fn validate(app: &mut App) {
    if let Some(sel) = &app.selection {
        if sel.print != fingerprint(app) {
            clear(app);
        }
    }
}

/// Where a press at `(x, y)` would start a selection: only when the top target there is plain
/// text, never a button, toast or a seam.
pub fn start_target(app: &App, x: u16, y: u16, hit: Option<&Action>) -> Option<(RegionKey, Pos)> {
    if !matches!(app.screen, Screen::Dashboard | Screen::Diff) || !app.mouse {
        return None;
    }
    if !matches!(
        hit,
        None | Some(
            Action::DiffRow(_) | Action::DiffFile(_) | Action::DiffFocus(_) | Action::FocusPane(_)
        )
    ) {
        return None;
    }
    let (region, row) = app.hits.texts.at(x, y)?;
    Some((region.key, Pos::new(row.ord, col_in(row, x))))
}

/// Starts a selection at `at`, after the click's own action has run. Repeated presses on one row
/// widen it: a word, then a line.
pub fn begin(app: &mut App, key: RegionKey, at: Pos) {
    let count = match app.text_click {
        Some((k, ord, tick, n))
            if k == key && ord == at.ord && app.ticks.saturating_sub(tick) <= MULTI_CLICK_TICKS =>
        {
            n % 3 + 1
        }
        _ => 1,
    };
    app.text_click = Some((key, at.ord, app.ticks, count));
    let unit = match count {
        2 => Unit::Word,
        3 => Unit::Line,
        _ => Unit::Char,
    };
    let Some(region) = app.hits.texts.region(key) else {
        return;
    };
    let mut sel = Selection::new(region, at, unit);
    sel.dragging = true;
    sel.print = fingerprint(app);
    app.selection = Some(sel);
    app.mark_dirty();
}

/// Mouse events while a text drag is on: the pointer may be anywhere, even over the terminal
/// pane, because the selection stays in the region it started in. `None` for anything else.
pub fn on_mouse(app: &mut App, mouse: MouseEvent) -> Option<Vec<Cmd>> {
    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
        return None;
    }
    let dragging = app.selection.as_ref().is_some_and(|s| s.dragging);
    if !dragging {
        return None;
    }
    match mouse.kind {
        MouseEventKind::Drag(MouseButton::Left) => {
            drag(app, mouse.column, mouse.row);
            Some(Vec::new())
        }
        MouseEventKind::Up(MouseButton::Left) => Some(release(app)),
        _ => None,
    }
}

fn drag(app: &mut App, x: u16, y: u16) {
    let key = match &mut app.selection {
        Some(sel) => {
            sel.absorb(&app.hits.texts);
            sel.key
        }
        None => return,
    };
    let Some(region) = app.hits.texts.region(key).cloned() else {
        return;
    };
    let mut head = locate(&region, x, y);
    let above = y < region.rect.y;
    if key == RegionKey::Diff && (above || y >= region.rect.bottom()) {
        super::diff::nudge_scroll(app, if above { -1 } else { 1 });
        let shown: Vec<&TextRow> = region.rows.iter().filter(|r| r.on_screen()).collect();
        let edge = if above { shown.first() } else { shown.last() };
        if let (Some(edge), Some(sel)) = (edge, &app.selection) {
            let beyond = if above {
                sel.prev_ord(edge.ord)
            } else {
                sel.next_ord(edge.ord)
            };
            head = Some(Pos::new(beyond.unwrap_or(edge.ord), col_in(edge, x)));
        }
    }
    if let (Some(sel), Some(head)) = (&mut app.selection, head) {
        sel.head = head;
        app.mark_dirty();
    }
}

fn release(app: &mut App) -> Vec<Cmd> {
    let Some(sel) = &mut app.selection else {
        return Vec::new();
    };
    sel.absorb(&app.hits.texts);
    sel.dragging = false;
    if sel.is_empty() {
        app.selection = None;
        app.mark_dirty();
        return Vec::new();
    }
    app.mark_dirty();
    copy(app)
}

/// Copies the selected text, if any is selected. A copy-mode selection ends with the copy.
pub fn copy(app: &mut App) -> Vec<Cmd> {
    let Some(sel) = &app.selection else {
        return Vec::new();
    };
    let text = sel.text();
    if sel.keyboard {
        app.selection = None;
    }
    app.mark_dirty();
    if text.is_empty() {
        return Vec::new();
    }
    vec![Cmd::CopySelection(text)]
}

/// Whether `y` has something to copy: a selection that is not just a click.
pub fn has_text(app: &App) -> bool {
    app.selection.as_ref().is_some_and(|s| !s.is_empty())
}

const START_HINT: &str =
    "Drag code to copy text. Drag the line numbers to pick lines to comment on.";

/// Says once per session how a drag in the diff splits.
pub fn hint_once(app: &mut App) -> Vec<Cmd> {
    if app.selection_hinted {
        return Vec::new();
    }
    app.selection_hinted = true;
    super::update::set_status(app, Notice::new(NoticeKind::Info, START_HINT))
}

// Keyboard copy mode.

const COPY_MODE: &str = "Copy mode: move with h j k l or w b, y copies, esc cancels.";

/// `v`: starts a selection at the diff cursor line.
pub fn start_keyboard(app: &mut App) -> Vec<Cmd> {
    let Some(state) = app.diff.as_ref().filter(|s| s.focus == DiffFocus::Diff) else {
        return Vec::new();
    };
    let ord = state.view.screen.start(state.view.cursor);
    let found = app
        .hits
        .texts
        .region(RegionKey::Diff)
        .filter(|r| r.rows.iter().any(|row| row.ord == ord));
    let Some(region) = found else {
        let text = "Move onto a line of code first, then press v.";
        return super::update::set_status(app, Notice::new(NoticeKind::Info, text));
    };
    let mut sel = Selection::new(region, Pos::new(ord, 0), Unit::Char);
    sel.keyboard = true;
    sel.print = fingerprint(app);
    app.selection = Some(sel);
    app.mark_dirty();
    super::update::set_status(app, Notice::new(NoticeKind::Info, COPY_MODE))
}

/// Keys in copy mode. `None` when copy mode is not on.
pub fn on_key(app: &mut App, key: KeyEvent) -> Option<Vec<Cmd>> {
    if !app.selection.as_ref().is_some_and(|s| s.keyboard) {
        return None;
    }
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return Some(Vec::new());
    }
    let map = &app.hits.texts;
    if let Some(sel) = app.selection.as_mut() {
        sel.absorb(map);
    }
    let sel = app.selection.as_mut()?;
    match key.code {
        KeyCode::Char('y') => return Some(copy(app)),
        KeyCode::Esc | KeyCode::Char('v') => {
            clear(app);
            return Some(Vec::new());
        }
        KeyCode::Tab => return Some(cycle_region(app)),
        KeyCode::Char('h') | KeyCode::Left => sel.step_left(),
        KeyCode::Char('l') | KeyCode::Right => sel.step_right(),
        KeyCode::Char('j') | KeyCode::Down => sel.step_down(),
        KeyCode::Char('k') | KeyCode::Up => sel.step_up(),
        KeyCode::Char('w') => sel.word_forward(),
        KeyCode::Char('b') => sel.word_back(),
        KeyCode::Char('0') | KeyCode::Home => sel.line_start(),
        KeyCode::Char('$') | KeyCode::End => sel.line_end(),
        _ => return Some(Vec::new()),
    }
    let (key, ord) = (sel.key, sel.head.ord);
    if key == RegionKey::Diff {
        super::diff::reveal_screen_row(app, ord);
    }
    app.mark_dirty();
    Some(Vec::new())
}

/// `tab` in copy mode: moves between the cursor line's code and the comments under it.
fn cycle_region(app: &mut App) -> Vec<Cmd> {
    let Some(state) = app.diff.as_ref() else {
        return Vec::new();
    };
    let mut order = vec![RegionKey::Diff];
    order.extend(
        state
            .view
            .rows
            .blocks_after(state.view.cursor)
            .into_iter()
            .map(RegionKey::Block),
    );
    let now = app.selection.as_ref().map_or(RegionKey::Diff, |s| s.key);
    let at = order.iter().position(|k| *k == now).unwrap_or(0);
    let next = order[(at + 1) % order.len()];
    let ord = match next {
        RegionKey::Diff => state.view.screen.start(state.view.cursor),
        _ => 0,
    };
    let Some(region) = app.hits.texts.region(next) else {
        return Vec::new();
    };
    let mut sel = Selection::new(region, Pos::new(ord, 0), Unit::Char);
    sel.keyboard = true;
    sel.print = fingerprint(app);
    app.selection = Some(sel);
    app.mark_dirty();
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(key: RegionKey, rows: &[(&str, Join)]) -> TextRegion {
        let mut r = TextRegion::new(key, Rect::new(10, 2, 40, rows.len() as u16));
        for (i, (text, join)) in rows.iter().enumerate() {
            r.rows
                .push(TextRow::new(i, (10, 2 + i as u16), 40, *text).joined(*join));
        }
        r
    }

    fn lines(rows: &[&str]) -> TextRegion {
        let rows: Vec<(&str, Join)> = rows.iter().map(|t| (*t, Join::Break)).collect();
        region(RegionKey::Description, &rows)
    }

    fn pick(region: &TextRegion, from: (usize, usize), to: (usize, usize)) -> Selection {
        let mut sel = Selection::new(region, Pos::new(from.0, from.1), Unit::Char);
        sel.head = Pos::new(to.0, to.1);
        sel
    }

    #[test]
    fn a_click_that_has_not_moved_selects_nothing() {
        let r = lines(&["hello world"]);
        let sel = pick(&r, (0, 3), (0, 3));
        assert!(sel.is_empty());
        assert_eq!(sel.text(), "");
        assert_eq!(sel.highlight(&r.rows[0]), None);
    }

    #[test]
    fn a_drag_takes_both_end_cells() {
        let r = lines(&["hello world"]);
        let sel = pick(&r, (0, 2), (0, 6));
        assert_eq!(sel.text(), "llo w");
        assert_eq!(sel.highlight(&r.rows[0]), Some((2, 7)));
    }

    #[test]
    fn the_ends_are_put_in_reading_order() {
        let r = lines(&["first row", "second row", "third row"]);
        let down = pick(&r, (0, 6), (2, 4));
        let up = pick(&r, (2, 4), (0, 6));
        assert_eq!(down.text(), up.text());
        assert_eq!(down.text(), "row\nsecond row\nthird");
        let back = pick(&r, (1, 5), (1, 1));
        assert_eq!(back.text(), "econd");
    }

    #[test]
    fn rows_between_are_whole_and_trailing_spaces_go() {
        let r = lines(&["abc   ", "  middle  ", "xyz"]);
        let sel = pick(&r, (0, 0), (2, 2));
        assert_eq!(sel.text(), "abc\n  middle\nxyz");
    }

    #[test]
    fn blank_rows_inside_a_selection_stay_blank() {
        let r = lines(&["one", "", "three"]);
        let sel = pick(&r, (0, 0), (2, 4));
        assert_eq!(sel.text(), "one\n\nthree");
    }

    #[test]
    fn wide_characters_are_never_split() {
        let r = lines(&["日本語です"]);
        // Cells 1 and 2 sit inside the first and second characters.
        let sel = pick(&r, (0, 1), (0, 2));
        assert_eq!(sel.text(), "日本");
        assert_eq!(sel.highlight(&r.rows[0]), Some((0, 4)));
        let last = pick(&r, (0, 8), (0, 9));
        assert_eq!(last.text(), "す");
    }

    #[test]
    fn combining_marks_travel_with_their_letter() {
        let r = lines(&["cafe\u{301} au lait"]);
        let sel = pick(&r, (0, 0), (0, 3));
        assert_eq!(sel.text(), "cafe\u{301}");
    }

    #[test]
    fn wrapped_rows_join_as_they_were_written() {
        let r = region(
            RegionKey::Description,
            &[
                ("the quick brown", Join::Break),
                ("fox jumps over", Join::Space),
                ("a lazyd", Join::Space),
                ("og", Join::Glue),
                ("next paragraph", Join::Break),
            ],
        );
        let sel = pick(&r, (0, 0), (4, 13));
        assert_eq!(
            sel.text(),
            "the quick brown fox jumps over a lazydog\nnext paragraph"
        );
    }

    #[test]
    fn wrapped_code_joins_with_nothing_and_keeps_its_spaces() {
        let r = region(
            RegionKey::Diff,
            &[("let x = foo(a,  ", Join::Break), ("  b);", Join::Glue)],
        );
        let sel = pick(&r, (0, 0), (1, 10));
        assert_eq!(sel.text(), "let x = foo(a,    b);");
    }

    #[test]
    fn a_double_click_takes_the_word_and_a_drag_grows_by_words() {
        let r = lines(&["let value_one = other.call(arg);"]);
        let mut sel = Selection::new(&r, Pos::new(0, 6), Unit::Word);
        assert_eq!(sel.text(), "value_one");
        sel.head = Pos::new(0, 19);
        assert_eq!(sel.text(), "value_one = other");
        sel.head = Pos::new(0, 0);
        assert_eq!(sel.text(), "let value_one");
    }

    #[test]
    fn word_at_knows_words_spaces_and_punctuation() {
        assert_eq!(word_at("foo.bar baz", 1), (0, 2));
        assert_eq!(word_at("foo.bar baz", 3), (3, 3));
        assert_eq!(word_at("foo  bar", 4), (3, 4));
        assert_eq!(word_at("foo", 99), (0, 2));
        assert_eq!(word_at("日本 x", 1), (0, 3));
        assert_eq!(word_at("", 0), (0, 0));
    }

    #[test]
    fn a_triple_click_takes_the_whole_source_line_across_wrapped_rows() {
        let r = region(
            RegionKey::Diff,
            &[
                ("before", Join::Break),
                ("first half ", Join::Break),
                ("second half", Join::Glue),
                ("after", Join::Break),
            ],
        );
        let sel = Selection::new(&r, Pos::new(2, 3), Unit::Line);
        assert_eq!(sel.text(), "first half second half");
        assert_eq!(sel.highlight(&r.rows[0]), None);
        assert_eq!(sel.highlight(&r.rows[1]), Some((0, 11)));
        assert_eq!(sel.highlight(&r.rows[3]), None);
    }

    #[test]
    fn a_shortened_path_copies_in_full() {
        let mut r = TextRegion::new(RegionKey::Files, Rect::new(0, 0, 20, 2));
        r.rows
            .push(TextRow::new(0, (4, 0), 12, "…ui/menus.rs").copying("src/ui/menus.rs".into()));
        r.rows.push(TextRow::new(1, (4, 1), 8, "mod.rs"));
        let sel = pick(&r, (0, 3), (1, 2));
        assert_eq!(sel.text(), "src/ui/menus.rs\nmod");
    }

    #[test]
    fn a_selection_only_knows_its_own_region() {
        let mut map = TextMap::default();
        map.push(lines(&["description text"]));
        let mut other = region(RegionKey::Diff, &[("code text", Join::Break)]);
        other.rect = Rect::new(60, 2, 40, 1);
        map.push(other);
        let region = map.region(RegionKey::Description).unwrap();
        let mut sel = Selection::new(region, Pos::new(0, 0), Unit::Char);
        sel.head = Pos::new(0, 99);
        // A pointer in the other region still resolves inside this one.
        let there = locate(region, 70, 2).unwrap();
        assert_eq!(there.ord, 0);
        sel.head = there;
        assert_eq!(sel.text(), "description text");
    }

    #[test]
    fn locate_clamps_above_below_and_between_rows() {
        let mut r = lines(&["alpha", "beta", "gamma"]);
        r.rows[1].y = 4;
        r.rows[2].y = 6;
        assert_eq!(locate(&r, 12, 2), Some(Pos::new(0, 2)));
        assert_eq!(locate(&r, 12, 0), Some(Pos::new(0, 0)), "above");
        assert_eq!(locate(&r, 30, 9), Some(Pos::new(2, 5)), "below");
        assert_eq!(
            locate(&r, 12, 5),
            Some(Pos::new(1, 4)),
            "between: the end above"
        );
        assert_eq!(locate(&r, 3, 4), Some(Pos::new(1, 0)), "left of the text");
        assert_eq!(locate(&r, 40, 4), Some(Pos::new(1, 4)), "right of the text");
    }

    #[test]
    fn rows_that_scrolled_away_are_still_copied() {
        let first = lines(&["one", "two", "three"]);
        let mut sel = Selection::new(&first, Pos::new(0, 0), Unit::Char);
        sel.head = Pos::new(1, 1);
        // The view scrolls: only the later rows are on the next frame.
        let mut later = TextRegion::new(RegionKey::Description, Rect::new(10, 2, 40, 2));
        later.rows.push(TextRow::new(2, (10, 2), 40, "three"));
        later.rows.push(TextRow::new(3, (10, 3), 40, "four"));
        let mut map = TextMap::default();
        map.push(later);
        sel.absorb(&map);
        sel.head = Pos::new(3, 2);
        assert_eq!(sel.text(), "one\ntwo\nthree\nfou");
    }

    #[test]
    fn keyboard_moves_stay_on_characters_and_rows() {
        let r = lines(&["日本 go", "next"]);
        let mut sel = Selection::new(&r, Pos::new(0, 0), Unit::Char);
        sel.keyboard = true;
        sel.step_right();
        assert_eq!(sel.head.col, 2, "past the whole wide character");
        sel.step_left();
        assert_eq!(sel.head.col, 0);
        sel.step_left();
        assert_eq!(sel.head.col, 0);
        sel.line_end();
        assert_eq!(sel.head.col, 6);
        sel.step_down();
        assert_eq!(sel.head, Pos::new(1, 3), "the column is clamped");
        sel.step_down();
        assert_eq!(sel.head.ord, 1, "nothing below");
        sel.step_up();
        assert_eq!(sel.head.ord, 0);
    }

    #[test]
    fn word_keys_cross_rows() {
        let r = lines(&["alpha beta", "  gamma"]);
        let mut sel = Selection::new(&r, Pos::new(0, 0), Unit::Char);
        sel.keyboard = true;
        sel.word_forward();
        assert_eq!(sel.head, Pos::new(0, 6));
        sel.word_forward();
        assert_eq!(sel.head, Pos::new(1, 2), "the next row's first word");
        sel.word_back();
        assert_eq!(sel.head, Pos::new(0, 6));
        sel.word_back();
        assert_eq!(sel.head, Pos::new(0, 0));
        assert_eq!(
            {
                sel.head = Pos::new(1, 4);
                sel.word_back();
                sel.head
            },
            Pos::new(1, 2)
        );
    }

    #[test]
    fn a_keyboard_selection_covers_its_start_cell() {
        let r = lines(&["hello"]);
        let mut sel = Selection::new(&r, Pos::new(0, 1), Unit::Char);
        sel.keyboard = true;
        assert_eq!(sel.text(), "e");
        sel.step_right();
        sel.step_right();
        assert_eq!(sel.text(), "ell");
    }
}
