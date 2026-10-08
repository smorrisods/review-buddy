//! The unified diff as a flat list of rows: hunk headers, diff lines, and the thread and
//! pending-comment blocks that sit under their anchor lines. Pure, so cursor movement,
//! windowing and anchor mapping are all unit-testable. Only the rows in view are ever styled.

use std::collections::HashMap;
use std::ops::Range;

use ratatui::layout::Rect;
use rb_core::{DraftComment, Side, Thread, Timestamp};
use rb_diff::{expand_tabs, Anchor, DiffBody, FileDiff, LineId, ParsedPatch};

use super::queue::age;
use crate::ui::text::{wrap_marked, wrap_rows, Join};

/// Columns before the code: cursor (2), old number (5), new number (5), sign (2).
pub const GUTTER: u16 = 14;
/// Used when config doesn't say otherwise (`diff.tab_width`).
pub const TAB_WIDTH: u8 = 4;
const BLOCK_PADDING: u16 = 4;
const MIN_TEXT: u16 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Hunk(u32),
    Line(LineId),
    /// One terminal line of a block: 0 is the top border, the last is the bottom border.
    Block {
        block: u32,
        line: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Thread { resolved: bool, outdated: bool },
    Pending { suggestion: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockLineKind {
    /// `author · age`
    Meta,
    Text,
    Removed,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockLine {
    pub kind: BlockLineKind,
    pub text: String,
    /// How the line joins the one before when copied: a wrapped line continues it.
    pub join: Join,
}

/// Where a block's text comes from, so a pending comment can be found again to edit or delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Not something you can edit here.
    None,
    /// The nth comment of your local draft.
    Draft(usize),
    /// The nth thread of the change, which holds a pending comment of yours on the forge.
    Thread(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub title: String,
    pub lines: Vec<BlockLine>,
    pub origin: Origin,
    /// The whole comment or thread as written, for copying it (`Y`).
    pub text: String,
}

impl Block {
    /// Content lines plus the two borders.
    pub fn height(&self) -> usize {
        self.lines.len() + 2
    }
}

/// What a file's blocks are built from.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub threads: &'a [Thread],
    pub drafts: &'a [DraftComment],
    pub now: Timestamp,
    pub tab_width: u8,
    /// The change's head moved since the draft was written.
    pub stale: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rows {
    rows: Vec<Row>,
    blocks: Vec<Block>,
    /// Row indexes the cursor can rest on, ascending.
    line_rows: Vec<usize>,
    /// Row index of the first line of each non-empty hunk.
    hunk_starts: Vec<usize>,
}

/// Width available to wrapped text inside a block for a diff pane `width` cells wide.
pub fn block_text_width(width: u16) -> u16 {
    width.saturating_sub(GUTTER + BLOCK_PADDING).max(MIN_TEXT)
}

impl Rows {
    /// An empty set, for fallbacks and for a file that hasn't loaded.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn build(file: &FileDiff, inputs: Inputs<'_>, width: u16) -> Self {
        let DiffBody::Text(patch) = &file.body else {
            return Self::empty();
        };
        let text_width = usize::from(block_text_width(width));
        let mut blocks = Vec::new();
        let mut after: HashMap<LineId, Vec<u32>> = HashMap::new();
        let mut loose = Vec::new();
        let mut place = |block: Block, at: Option<LineId>, blocks: &mut Vec<Block>| {
            let index = blocks.len() as u32;
            blocks.push(block);
            match at {
                Some(id) => after.entry(id).or_default().push(index),
                None => loose.push(index),
            }
        };

        for (index, thread) in inputs
            .threads
            .iter()
            .enumerate()
            .filter(|(_, t)| t.path.as_deref() == Some(file.path.as_str()))
        {
            let at = thread
                .line
                .and_then(|line| {
                    patch.find_by_anchor(Anchor {
                        side: thread.side,
                        line,
                    })
                })
                .filter(|_| !thread.outdated);
            let mut block = thread_block(thread, inputs.now, text_width);
            if thread.comments.iter().any(|c| c.pending) {
                block.origin = Origin::Thread(index);
            }
            place(block, at, &mut blocks);
        }
        for (index, draft) in inputs
            .drafts
            .iter()
            .enumerate()
            .filter(|(_, d)| d.path == file.path)
        {
            let at = patch.find_by_anchor(Anchor {
                side: draft.side,
                line: draft.line,
            });
            let originals = original_lines(file, draft);
            let mut block = draft_block(draft, originals, text_width, inputs.tab_width);
            if at.is_none() {
                block.title.push_str(" · outdated");
            } else if inputs.stale {
                block.title.push_str(" · code changed");
            }
            block.origin = Origin::Draft(index);
            place(block, at, &mut blocks);
        }

        let mut rows = Vec::new();
        let mut line_rows = Vec::new();
        let mut hunk_starts = Vec::new();
        for (h, hunk) in patch.hunks.iter().enumerate() {
            rows.push(Row::Hunk(h as u32));
            for l in 0..hunk.lines.len() {
                let id = LineId {
                    hunk: h as u32,
                    line: l as u32,
                };
                if l == 0 {
                    hunk_starts.push(rows.len());
                }
                line_rows.push(rows.len());
                rows.push(Row::Line(id));
                for &b in after.get(&id).into_iter().flatten() {
                    push_block(&mut rows, &blocks, b);
                }
            }
        }
        for b in loose {
            push_block(&mut rows, &blocks, b);
        }
        Self {
            rows,
            blocks,
            line_rows,
            hunk_starts,
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, index: usize) -> Option<Row> {
        self.rows.get(index).copied()
    }

    pub fn block(&self, index: u32) -> Option<&Block> {
        self.blocks.get(index as usize)
    }

    /// Every block that hangs from the line at `row`, in the order they are drawn.
    pub fn blocks_after(&self, row: usize) -> Vec<u32> {
        let mut found = Vec::new();
        let mut at = row + 1;
        while let Some(Row::Block { block, line }) = self.rows.get(at).copied() {
            if line == 0 {
                found.push(block);
            }
            at += 1;
        }
        found
    }

    pub fn line_count(&self) -> usize {
        self.line_rows.len()
    }

    /// The editable blocks that hang from the line at `row`, in the order they're drawn.
    pub fn attached(&self, row: usize) -> Vec<(u32, Origin)> {
        let mut found = Vec::new();
        let mut at = row + 1;
        while let Some(Row::Block { block, line }) = self.rows.get(at).copied() {
            if line == 0 {
                if let Some(b) = self
                    .blocks
                    .get(block as usize)
                    .filter(|b| b.origin != Origin::None)
                {
                    found.push((block, b.origin));
                }
            }
            at += 1;
        }
        found
    }

    pub fn hunk_count(&self) -> usize {
        self.hunk_starts.len()
    }

    /// How many diff lines sit in rows `top..=bottom`, not counting headers and blocks.
    pub fn lines_between(&self, top: usize, bottom: usize) -> usize {
        self.line_rows.partition_point(|&r| r <= bottom)
            - self.line_rows.partition_point(|&r| r < top)
    }

    pub fn first_line(&self) -> Option<usize> {
        self.line_rows.first().copied()
    }

    pub fn last_line(&self) -> Option<usize> {
        self.line_rows.last().copied()
    }

    pub fn line_id(&self, row: usize) -> Option<LineId> {
        match self.rows.get(row)? {
            Row::Line(id) => Some(*id),
            _ => None,
        }
    }

    pub fn row_of(&self, id: LineId) -> Option<usize> {
        self.line_rows
            .iter()
            .copied()
            .find(|&r| self.rows[r] == Row::Line(id))
    }

    /// Moves the cursor `delta` lines, skipping headers and blocks, and stopping at either end.
    pub fn step(&self, cursor: usize, delta: isize) -> usize {
        let Some(last) = self.line_rows.len().checked_sub(1) else {
            return cursor;
        };
        let at = self.line_rows.partition_point(|&r| r < cursor);
        let at = if self.line_rows.get(at) == Some(&cursor) {
            at
        } else {
            at.saturating_sub(1)
        };
        self.line_rows[at.saturating_add_signed(delta).min(last)]
    }

    /// The line a click or a jump to `row` should land on: headers resolve to the first line
    /// below, blocks to the line they hang from.
    pub fn nearest_line(&self, row: usize) -> Option<usize> {
        let at = self.line_rows.partition_point(|&r| r < row);
        let after = self.line_rows.get(at).copied();
        let before = at.checked_sub(1).map(|i| self.line_rows[i]);
        match self.rows.get(row)? {
            Row::Line(_) => after,
            Row::Hunk(_) => after.or(before),
            Row::Block { .. } => before.or(after),
        }
    }

    /// The first line at or below `row`, else the last line.
    pub fn line_from(&self, row: usize) -> Option<usize> {
        let at = self.line_rows.partition_point(|&r| r < row);
        self.line_rows.get(at).or(self.line_rows.last()).copied()
    }

    /// The last line above `row`, else the first line.
    pub fn line_before(&self, row: usize) -> Option<usize> {
        let at = self.line_rows.partition_point(|&r| r < row);
        at.checked_sub(1)
            .map(|i| self.line_rows[i])
            .or(self.line_rows.first().copied())
    }

    pub fn next_hunk(&self, cursor: usize) -> Option<usize> {
        self.hunk_starts.iter().copied().find(|&s| s > cursor)
    }

    /// The start of the hunk the cursor is inside, or of the one before when it already sits
    /// on a start.
    pub fn prev_hunk(&self, cursor: usize) -> Option<usize> {
        self.hunk_starts.iter().rev().copied().find(|&s| s < cursor)
    }

    /// `(current, total)` hunk numbers for the cursor, 1-based.
    pub fn hunk_position(&self, cursor: usize) -> (usize, usize) {
        let current = self.hunk_starts.partition_point(|&s| s <= cursor);
        (
            current.max(1).min(self.hunk_starts.len()),
            self.hunk_starts.len(),
        )
    }

    /// The rows worth keeping on screen with the cursor: a hunk header above its first line,
    /// and the blocks hanging from it below.
    pub fn reveal_span(&self, cursor: usize) -> (usize, usize) {
        let top = match cursor.checked_sub(1).and_then(|i| self.rows.get(i)) {
            Some(Row::Hunk(_)) => cursor - 1,
            _ => cursor,
        };
        let mut bottom = cursor;
        while matches!(self.rows.get(bottom + 1), Some(Row::Block { .. })) {
            bottom += 1;
        }
        (top, bottom)
    }
}

/// How many terminal rows each logical row takes, for scrolling, revealing, drawing and mapping
/// the pointer. The cursor, ranges and navigation stay on logical rows ([`Rows`]); only a diff
/// line can be taller than one screen row, and only while wrap is on. Blocks and hunk headers
/// are already one screen row per row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScreenMap {
    /// Screen rows before each logical row, plus the total. Empty when every row is one screen
    /// row, so wrap off costs nothing.
    starts: Vec<usize>,
    len: usize,
    wrap: bool,
}

impl ScreenMap {
    /// One screen row per logical row.
    pub fn identity(len: usize) -> Self {
        Self {
            starts: Vec::new(),
            len,
            wrap: false,
        }
    }

    /// Measures every diff line of `patch` at `text_width` cells of code. O(total characters),
    /// with no styling, so it stays cheap for the biggest files.
    pub fn build(
        rows: &Rows,
        patch: &ParsedPatch,
        text_width: usize,
        tab_width: u8,
        wrap: bool,
    ) -> Self {
        if !wrap {
            return Self::identity(rows.len());
        }
        let mut starts = Vec::with_capacity(rows.len() + 1);
        let mut total = 0;
        for index in 0..rows.len() {
            starts.push(total);
            total += match rows.row(index) {
                Some(Row::Line(id)) => patch
                    .line(id)
                    .map_or(1, |l| wrap_rows(&l.text, tab_width, text_width)),
                _ => 1,
            };
        }
        starts.push(total);
        Self {
            starts,
            len: rows.len(),
            wrap: true,
        }
    }

    /// Whether wrap was on when this was measured.
    pub fn wrapping(&self) -> bool {
        self.wrap
    }

    /// Screen rows in all.
    pub fn total(&self) -> usize {
        if self.starts.is_empty() {
            self.len
        } else {
            self.starts[self.len]
        }
    }

    /// The first screen row of logical row `row`.
    pub fn start(&self, row: usize) -> usize {
        if self.starts.is_empty() {
            row
        } else {
            self.starts[row.min(self.len)]
        }
    }

    pub fn height(&self, row: usize) -> usize {
        if self.starts.is_empty() || row >= self.len {
            1
        } else {
            self.starts[row + 1] - self.starts[row]
        }
    }

    /// The last screen row of logical row `row`.
    pub fn end(&self, row: usize) -> usize {
        self.start(row) + self.height(row) - 1
    }

    /// The logical row drawn on screen row `screen`, and which of its rows that is (0 is the
    /// first). `None` past the end.
    pub fn locate(&self, screen: usize) -> Option<(usize, usize)> {
        if self.starts.is_empty() {
            return (screen < self.len).then_some((screen, 0));
        }
        if screen >= self.total() {
            return None;
        }
        let row = self.starts[..=self.len].partition_point(|&s| s <= screen) - 1;
        Some((row, screen - self.starts[row]))
    }

    /// The logical rows touched by `height` screen rows from `scroll`, including a first row
    /// that is only partly in view.
    pub fn rows_in(&self, scroll: usize, height: usize) -> Range<usize> {
        let Some((first, _)) = self.locate(scroll) else {
            return self.len..self.len;
        };
        let last = self
            .locate((scroll + height.max(1) - 1).min(self.total() - 1))
            .map_or(first, |(row, _)| row);
        first..last + 1
    }
}

/// The two places a click on a diff row can land: the gutter (cursor, numbers, sign) and the
/// code. Every screen row of a wrapped line has the same split. The draw registers one hit per
/// screen row; [`hit_at`] says which of the two a pointer is over.
pub fn row_rects(area: Rect, y: u16) -> (Rect, Rect) {
    let gutter = GUTTER.min(area.width);
    (
        Rect::new(area.x, y, gutter, 1),
        Rect::new(area.x + gutter, y, area.width - gutter, 1),
    )
}

/// What lies under a screen position in the diff's code area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenHit {
    /// The logical row.
    pub row: usize,
    /// Which screen row of that logical row (0 is the first).
    pub sub: usize,
    /// Cells from the left edge of the gutter when `gutter`, else from the left edge of the
    /// code. Add `ui::text::row_start(&wrap_points(..), sub)` to get the cell offset into the whole
    /// line.
    pub column: usize,
    pub gutter: bool,
}

/// Maps a pointer at screen `(x, y)` to a logical row, a sub-row and a cell column, for the code
/// area `area` scrolled to screen row `scroll`. `None` outside the area or past the last row.
pub fn hit_at(map: &ScreenMap, area: Rect, scroll: usize, x: u16, y: u16) -> Option<ScreenHit> {
    if x < area.x || x >= area.right() || y < area.y || y >= area.bottom() {
        return None;
    }
    let (row, sub) = map.locate(scroll + usize::from(y - area.y))?;
    let (gutter, code) = row_rects(area, area.y);
    let on_gutter = x < gutter.right();
    let column = if on_gutter { x - gutter.x } else { x - code.x };
    Some(ScreenHit {
        row,
        sub,
        column: usize::from(column),
        gutter: on_gutter,
    })
}

fn push_block(rows: &mut Vec<Row>, blocks: &[Block], index: u32) {
    for line in 0..blocks[index as usize].height() as u32 {
        rows.push(Row::Block { block: index, line });
    }
}

/// The rows to draw for a viewport starting at `scroll`.
pub fn visible(scroll: usize, height: usize, total: usize) -> Range<usize> {
    let start = scroll.min(total);
    start..(start + height).min(total)
}

pub fn max_scroll(total: usize, height: usize) -> usize {
    total.saturating_sub(height)
}

/// The smallest scroll change that brings rows `top..=bottom` into view. If they don't all fit,
/// `top` wins.
pub fn reveal(scroll: usize, top: usize, bottom: usize, height: usize, total: usize) -> usize {
    let height = height.max(1);
    let bottom = bottom.min(top + height - 1);
    let scroll = if top < scroll {
        top
    } else if bottom >= scroll + height {
        bottom + 1 - height
    } else {
        scroll
    };
    scroll.min(max_scroll(total, height))
}

fn thread_block(thread: &Thread, now: Timestamp, width: usize) -> Block {
    let place = match (thread.line, thread.start_line, thread.outdated) {
        (Some(line), Some(start), false) if start < line => format!("lines {start}–{line}"),
        (Some(line), _, false) => format!("line {line}"),
        _ => "outdated".to_string(),
    };
    let mut title = format!("thread · {place}");
    if thread.pending {
        title.push_str(" · ◌ pending");
    }
    if thread.resolved {
        title.push_str(" · ✓ resolved");
    }
    let mut lines = Vec::new();
    for comment in &thread.comments {
        lines.push(BlockLine {
            kind: BlockLineKind::Meta,
            text: format!("{} · {}", comment.author, age(now, comment.created_at)),
            join: Join::Break,
        });
        push_wrapped(&mut lines, &comment.body, width);
    }
    let text = thread
        .comments
        .iter()
        .map(|c| {
            format!(
                "{} · {}\n{}",
                c.author,
                age(now, c.created_at),
                c.body.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    Block {
        kind: BlockKind::Thread {
            resolved: thread.resolved,
            outdated: thread.outdated,
        },
        title,
        lines,
        origin: Origin::None,
        text,
    }
}

fn draft_block(draft: &DraftComment, originals: Vec<String>, width: usize, tab_width: u8) -> Block {
    let (note, replacement) = parse_suggestion(&draft.body);
    let suggestion = replacement.is_some();
    let place = match draft.start_line {
        Some(start) if start != draft.line => format!("lines {start}–{}", draft.line),
        _ => format!("line {}", draft.line),
    };
    let what = if suggestion { "suggestion" } else { "comment" };
    let mut lines = Vec::new();
    push_wrapped(&mut lines, &note, width);
    if let Some(replacement) = replacement {
        for text in originals {
            lines.push(BlockLine {
                kind: BlockLineKind::Removed,
                text: expand_tabs(&text, tab_width),
                join: Join::Break,
            });
        }
        for text in replacement {
            lines.push(BlockLine {
                kind: BlockLineKind::Added,
                text: expand_tabs(&text, tab_width),
                join: Join::Break,
            });
        }
    }
    Block {
        kind: BlockKind::Pending { suggestion },
        title: format!("pending {what} · {place}"),
        lines,
        origin: Origin::None,
        text: draft.body.trim().to_string(),
    }
}

fn push_wrapped(out: &mut Vec<BlockLine>, text: &str, width: usize) {
    let mut wrapped = wrap_marked(text.trim(), width);
    while wrapped.last().is_some_and(|(line, _)| line.is_empty()) {
        wrapped.pop();
    }
    out.extend(wrapped.into_iter().map(|(text, join)| BlockLine {
        kind: BlockLineKind::Text,
        text,
        join,
    }));
}

/// The text of the lines a draft comment covers, as the patch shows them.
fn original_lines(file: &FileDiff, draft: &DraftComment) -> Vec<String> {
    let Some(patch) = file.parsed() else {
        return Vec::new();
    };
    let start = draft.start_line.unwrap_or(draft.line).min(draft.line);
    (start..=draft.line)
        .filter_map(|n| match draft.side {
            Side::New => patch.find_by_new(n),
            Side::Old => patch.find_by_old(n),
        })
        .filter_map(|id| patch.line(id))
        .map(|line| line.text.clone())
        .collect()
}

/// Splits a comment body into its prose and, if it holds one, its suggestion block.
pub fn parse_suggestion(body: &str) -> (String, Option<Vec<String>>) {
    let mut prose = Vec::new();
    let mut replacement: Option<Vec<String>> = None;
    let mut inside = false;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if inside {
            if trimmed.starts_with("```") {
                inside = false;
            } else if let Some(r) = replacement.as_mut() {
                r.push(line.to_string());
            }
        } else if trimmed.starts_with("```suggestion") && replacement.is_none() {
            inside = true;
            replacement = Some(Vec::new());
        } else {
            prose.push(line);
        }
    }
    (prose.join("\n").trim().to_string(), replacement)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rb_core::{Comment, CommentId, FilePatch, FileStatus, ThreadId};

    pub const PATCH: &str = "@@ -1,4 +1,5 @@ fn a()\n one\n-two\n+TWO\n+two and a half\n three\n@@ -20,3 +21,3 @@ fn b()\n twenty\n-old\n+new\n end";

    pub fn file(patch: Option<&str>) -> FileDiff {
        FileDiff::from_file_patch(&FilePatch {
            path: "src/a.rs".into(),
            old_path: None,
            status: FileStatus::Modified,
            adds: 0,
            dels: 0,
            patch: patch.map(str::to_string),
        })
    }

    pub fn thread(line: u32, side: Side) -> Thread {
        Thread {
            id: ThreadId("t1".into()),
            path: Some("src/a.rs".into()),
            line: Some(line),
            side,
            start_line: None,
            start_side: None,
            pending: false,
            resolved: false,
            outdated: false,
            comments: vec![
                Comment {
                    id: CommentId("c1".into()),
                    author: "jo".into(),
                    body: "Should this wrap round?".into(),
                    created_at: Timestamp(0),
                    pending: false,
                },
                Comment {
                    id: CommentId("c2".into()),
                    author: "ada".into(),
                    body: "Yes.".into(),
                    created_at: Timestamp(3_600),
                    pending: false,
                },
            ],
        }
    }

    #[test]
    fn range_and_pending_threads_say_so_in_the_title() {
        let mut t = thread(7, Side::New);
        assert_eq!(thread_block(&t, Timestamp(0), 40).title, "thread · line 7");
        t.start_line = Some(5);
        t.start_side = Some(Side::New);
        t.pending = true;
        assert_eq!(
            thread_block(&t, Timestamp(0), 40).title,
            "thread · lines 5–7 · ◌ pending"
        );
        t.start_line = Some(7);
        assert_eq!(
            thread_block(&t, Timestamp(0), 40).title,
            "thread · line 7 · ◌ pending"
        );
    }

    pub fn draft(line: u32, body: &str) -> DraftComment {
        DraftComment {
            path: "src/a.rs".into(),
            side: Side::New,
            start_line: None,
            line,
            body: body.into(),
        }
    }

    fn build(threads: &[Thread], drafts: &[DraftComment], width: u16) -> Rows {
        Rows::build(
            &file(Some(PATCH)),
            Inputs {
                threads,
                drafts,
                now: Timestamp(7_200),
                tab_width: TAB_WIDTH,
                stale: false,
            },
            width,
        )
    }

    #[test]
    fn rows_interleave_headers_and_lines() {
        let rows = build(&[], &[], 100);
        assert_eq!(rows.len(), 2 + 5 + 4);
        assert_eq!(rows.row(0), Some(Row::Hunk(0)));
        assert_eq!(rows.first_line(), Some(1));
        assert_eq!(rows.line_count(), 9);
        assert_eq!(rows.hunk_count(), 2);
        assert_eq!(rows.row(6), Some(Row::Hunk(1)));
        assert_eq!(rows.last_line(), Some(10));
    }

    #[test]
    fn step_skips_headers_and_clamps_at_the_ends() {
        let rows = build(&[], &[], 100);
        assert_eq!(rows.step(1, 1), 2);
        assert_eq!(rows.step(5, 1), 7, "hops over the second hunk header");
        assert_eq!(rows.step(7, -1), 5);
        assert_eq!(rows.step(1, -1), 1);
        assert_eq!(rows.step(10, 5), 10);
        assert_eq!(rows.step(1, 100), 10);
        assert_eq!(Rows::empty().step(0, 1), 0);
    }

    #[test]
    fn hunk_navigation_crosses_hunks_and_stops_at_the_ends() {
        let rows = build(&[], &[], 100);
        assert_eq!(rows.next_hunk(1), Some(7));
        assert_eq!(rows.next_hunk(7), None);
        assert_eq!(rows.prev_hunk(9), Some(7));
        assert_eq!(rows.prev_hunk(7), Some(1));
        assert_eq!(rows.prev_hunk(1), None);
        assert_eq!(rows.hunk_position(3), (1, 2));
        assert_eq!(rows.hunk_position(9), (2, 2));
        assert_eq!(rows.hunk_position(0), (1, 2));
    }

    #[test]
    fn threads_hang_from_their_anchor_line_on_either_side() {
        let new_side = thread(2, Side::New);
        let rows = build(std::slice::from_ref(&new_side), &[], 100);
        let id = LineId { hunk: 0, line: 2 };
        let at = rows.row_of(id).unwrap();
        assert_eq!(
            rows.row(at + 1),
            Some(Row::Block { block: 0, line: 0 }),
            "the block starts on the row right below the line"
        );
        let block = rows.block(0).unwrap();
        assert_eq!(block.title, "thread · line 2");
        assert_eq!(block.lines[0].text, "jo · 2h");
        assert_eq!(block.lines[2].text, "ada · 1h");
        assert_eq!(
            rows.row(at + 1 + block.height()),
            Some(Row::Line(LineId { hunk: 0, line: 3 }))
        );

        let old_side = thread(2, Side::Old);
        let rows = build(&[old_side], &[], 100);
        let at = rows.row_of(LineId { hunk: 0, line: 1 }).unwrap();
        assert!(matches!(rows.row(at + 1), Some(Row::Block { .. })));
    }

    #[test]
    fn outdated_and_unmatched_threads_land_at_the_end() {
        let mut stale = thread(2, Side::New);
        stale.outdated = true;
        stale.resolved = true;
        let rows = build(&[stale, thread(999, Side::New)], &[], 100);
        let block = rows.block(0).unwrap();
        assert_eq!(block.title, "thread · outdated · ✓ resolved");
        let last = rows.len() - 1;
        assert!(matches!(rows.row(last), Some(Row::Block { block: 1, .. })));
        assert_eq!(
            rows.row(rows.last_line().unwrap() + 1),
            Some(Row::Block { block: 0, line: 0 })
        );
    }

    #[test]
    fn other_files_threads_are_ignored() {
        let mut elsewhere = thread(2, Side::New);
        elsewhere.path = Some("src/b.rs".into());
        let rows = build(&[elsewhere], &[], 100);
        assert_eq!(rows.len(), 11);
    }

    #[test]
    fn suggestions_show_the_original_and_the_replacement() {
        let mut d = draft(3, "Tidier:\n\n```suggestion\nTWO!\n```");
        d.start_line = Some(2);
        let rows = build(&[], &[d], 100);
        let block = rows.block(0).unwrap();
        assert_eq!(block.title, "pending suggestion · lines 2–3");
        let kinds: Vec<_> = block
            .lines
            .iter()
            .map(|l| (l.kind, l.text.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                (BlockLineKind::Text, "Tidier:"),
                (BlockLineKind::Removed, "TWO"),
                (BlockLineKind::Removed, "two and a half"),
                (BlockLineKind::Added, "TWO!"),
            ]
        );
        assert!(matches!(
            block.kind,
            BlockKind::Pending { suggestion: true }
        ));
    }

    #[test]
    fn plain_pending_comments_have_no_diff() {
        let rows = build(&[], &[draft(2, "Nit: naming.")], 100);
        let block = rows.block(0).unwrap();
        assert_eq!(block.title, "pending comment · line 2");
        assert_eq!(block.lines.len(), 1);
    }

    #[test]
    fn parse_suggestion_handles_ranges_and_missing_fences() {
        let (note, s) = parse_suggestion("a\n```suggestion:-1+1\nx\ny\n```\nb");
        assert_eq!(note, "a\nb");
        assert_eq!(s, Some(vec!["x".to_string(), "y".to_string()]));
        assert_eq!(parse_suggestion("just words"), ("just words".into(), None));
        assert_eq!(
            parse_suggestion("```suggestion\nopen").1,
            Some(vec!["open".into()])
        );
    }

    #[test]
    fn nearest_line_resolves_headers_down_and_blocks_up() {
        let rows = build(&[thread(2, Side::New)], &[], 100);
        assert_eq!(rows.nearest_line(0), Some(1));
        let at = rows.row_of(LineId { hunk: 0, line: 2 }).unwrap();
        assert_eq!(rows.nearest_line(at + 2), Some(at));
        assert_eq!(rows.nearest_line(at), Some(at));
        assert_eq!(rows.nearest_line(9999), None);
    }

    #[test]
    fn reveal_span_includes_the_header_above_and_blocks_below() {
        let rows = build(&[thread(2, Side::New)], &[], 100);
        assert_eq!(rows.reveal_span(1), (0, 1));
        let at = rows.row_of(LineId { hunk: 0, line: 2 }).unwrap();
        let (top, bottom) = rows.reveal_span(at);
        assert_eq!(top, at);
        assert_eq!(bottom, at + rows.block(0).unwrap().height());
    }

    #[test]
    fn line_from_and_before_clamp_into_the_list() {
        let rows = build(&[], &[], 100);
        assert_eq!(rows.line_from(0), Some(1));
        assert_eq!(rows.line_from(6), Some(7));
        assert_eq!(rows.line_from(500), Some(10));
        assert_eq!(rows.line_before(8), Some(7));
        assert_eq!(rows.line_before(0), Some(1));
    }

    #[test]
    fn fallback_files_have_no_rows() {
        let rows = Rows::build(
            &file(None),
            Inputs {
                threads: &[],
                drafts: &[],
                now: Timestamp(0),
                tab_width: TAB_WIDTH,
                stale: false,
            },
            100,
        );
        assert!(rows.is_empty());
        assert_eq!(rows.first_line(), None);
    }

    fn wrapped(width: usize) -> (Rows, ScreenMap) {
        let file = file(Some(
            "@@ -1,3 +1,3 @@\n short\n-0123456789abcdefghij\n+0123456789abcdefghijKLMNOPQRSTU\n tail",
        ));
        let patch = file.parsed().unwrap();
        let rows = Rows::build(
            &file,
            Inputs {
                threads: &[],
                drafts: &[],
                now: Timestamp(0),
                tab_width: TAB_WIDTH,
                stale: false,
            },
            100,
        );
        let map = ScreenMap::build(&rows, patch, width, TAB_WIDTH, true);
        (rows, map)
    }

    #[test]
    fn heights_and_prefix_sums_follow_the_wrapped_lines() {
        // rows: hunk header, short, removed (20 chars), added (31 chars), tail
        let (rows, map) = wrapped(10);
        assert_eq!(rows.len(), 5);
        let heights: Vec<_> = (0..5).map(|r| map.height(r)).collect();
        assert_eq!(heights, [1, 1, 2, 4, 1]);
        assert_eq!(map.total(), 9);
        assert_eq!(
            (0..5).map(|r| map.start(r)).collect::<Vec<_>>(),
            [0, 1, 2, 4, 8]
        );
        assert_eq!(map.end(3), 7);
        assert!(map.wrapping());
    }

    #[test]
    fn wrap_off_is_the_identity() {
        let f = file(Some("@@ -1 +1 @@\n a"));
        let rows = Rows::build(
            &f,
            Inputs {
                threads: &[],
                drafts: &[],
                now: Timestamp(0),
                tab_width: TAB_WIDTH,
                stale: false,
            },
            100,
        );
        let off = ScreenMap::build(&rows, f.parsed().unwrap(), 10, TAB_WIDTH, false);
        assert!(!off.wrapping());
        assert_eq!(off.total(), rows.len());
        assert_eq!(off.start(1), 1);
        assert_eq!(off.height(1), 1);
        assert_eq!(off.locate(1), Some((1, 0)));
        assert_eq!(off.locate(rows.len()), None);
    }

    #[test]
    fn locate_maps_screen_rows_back_to_logical_rows() {
        let (_, map) = wrapped(10);
        let got: Vec<_> = (0..9).map(|s| map.locate(s).unwrap()).collect();
        assert_eq!(
            got,
            [
                (0, 0),
                (1, 0),
                (2, 0),
                (2, 1),
                (3, 0),
                (3, 1),
                (3, 2),
                (3, 3),
                (4, 0)
            ]
        );
        assert_eq!(map.locate(9), None);
    }

    #[test]
    fn rows_in_includes_a_partly_visible_first_row() {
        let (_, map) = wrapped(10);
        assert_eq!(map.rows_in(0, 3), 0..3);
        assert_eq!(map.rows_in(3, 2), 2..4, "starts inside row 2");
        assert_eq!(map.rows_in(5, 100), 3..5);
        assert_eq!(map.rows_in(50, 3), 5..5);
    }

    #[test]
    fn reveal_works_in_screen_rows_for_a_tall_line() {
        let (rows, map) = wrapped(10);
        let cursor = rows.row_of(LineId { hunk: 0, line: 2 }).unwrap();
        let (top, bottom) = rows.reveal_span(cursor);
        let (st, sb) = (map.start(top), map.end(bottom));
        assert_eq!((st, sb), (4, 7));
        assert_eq!(
            reveal(0, st, sb, 3, map.total()),
            4,
            "top wins when too tall"
        );
        assert_eq!(
            reveal(0, st, sb, 5, map.total()),
            3,
            "bottom brought into view"
        );
        assert_eq!(reveal(4, st, sb, 5, map.total()), 4);
    }

    #[test]
    fn hit_at_splits_gutter_from_code_and_finds_the_sub_row() {
        let (_, map) = wrapped(10);
        let area = Rect::new(34, 3, 24, 6);
        // scrolled so screen row 3 (row 2, second sub-row) is the top
        let at = |x, y| hit_at(&map, area, 3, x, y);
        assert_eq!(
            at(34, 3),
            Some(ScreenHit {
                row: 2,
                sub: 1,
                column: 0,
                gutter: true
            })
        );
        assert_eq!(
            at(34 + GUTTER - 1, 3),
            Some(ScreenHit {
                row: 2,
                sub: 1,
                column: usize::from(GUTTER) - 1,
                gutter: true
            })
        );
        assert_eq!(
            at(34 + GUTTER, 4),
            Some(ScreenHit {
                row: 3,
                sub: 0,
                column: 0,
                gutter: false
            })
        );
        assert_eq!(
            at(34 + GUTTER + 7, 6),
            Some(ScreenHit {
                row: 3,
                sub: 2,
                column: 7,
                gutter: false
            })
        );
        assert_eq!(at(33, 3), None, "left of the area");
        assert_eq!(at(34, 9), None, "below the area");
        assert_eq!(at(34, 2), None, "above the area");
        assert_eq!(hit_at(&map, area, 6, 40, 8), None, "past the last row");
    }

    #[test]
    fn row_rects_split_at_the_gutter() {
        let (gutter, code) = row_rects(Rect::new(10, 2, 40, 9), 5);
        assert_eq!(gutter, Rect::new(10, 5, GUTTER, 1));
        assert_eq!(code, Rect::new(10 + GUTTER, 5, 40 - GUTTER, 1));
        let (g, c) = row_rects(Rect::new(0, 0, 5, 1), 0);
        assert_eq!((g.width, c.width), (5, 0));
    }

    #[test]
    fn windowing_math() {
        assert_eq!(visible(0, 10, 100), 0..10);
        assert_eq!(visible(95, 10, 100), 95..100);
        assert_eq!(visible(200, 10, 100), 100..100);
        assert_eq!(visible(0, 10, 3), 0..3);
        assert_eq!(max_scroll(100, 10), 90);
        assert_eq!(max_scroll(5, 10), 0);
        assert_eq!(reveal(0, 12, 12, 10, 100), 3);
        assert_eq!(reveal(20, 12, 12, 10, 100), 12);
        assert_eq!(reveal(5, 8, 10, 10, 100), 5);
        assert_eq!(
            reveal(0, 5, 40, 10, 100),
            5,
            "top wins when it can't all fit"
        );
        assert_eq!(reveal(0, 98, 99, 10, 100), 90);
    }

    #[test]
    fn large_patches_build_rows_quickly_and_window_cheaply() {
        let mut patch = String::from("@@ -1,5000 +1,5000 @@\n");
        for i in 0..5000 {
            patch.push_str(&format!(" line {i}\n"));
        }
        let big = file(Some(&patch));
        let rows = Rows::build(
            &big,
            Inputs {
                threads: &[],
                drafts: &[],
                now: Timestamp(0),
                tab_width: TAB_WIDTH,
                stale: false,
            },
            120,
        );
        assert_eq!(rows.line_count(), 5000);
        assert_eq!(visible(2500, 30, rows.len()).len(), 30);
        assert_eq!(rows.step(1, 4999), 5000);
    }
}
