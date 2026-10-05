//! The unified diff as a flat list of rows: hunk headers, diff lines, and the thread and
//! pending-comment blocks that sit under their anchor lines. Pure, so cursor movement,
//! windowing and anchor mapping are all unit-testable. Only the rows in view are ever styled.

use std::collections::HashMap;
use std::ops::Range;

use rb_core::{DraftComment, Side, Thread, Timestamp};
use rb_diff::{expand_tabs, Anchor, DiffBody, FileDiff, LineId};

use super::queue::age;
use crate::ui::text::wrap;

/// Columns before the code: cursor (2), old number (5), new number (5), sign (2).
pub const GUTTER: u16 = 14;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub title: String,
    pub lines: Vec<BlockLine>,
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

        for thread in inputs
            .threads
            .iter()
            .filter(|t| t.path.as_deref() == Some(file.path.as_str()))
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
            let block = thread_block(thread, inputs.now, text_width);
            place(block, at, &mut blocks);
        }
        for draft in inputs.drafts.iter().filter(|d| d.path == file.path) {
            let at = patch.find_by_anchor(Anchor {
                side: draft.side,
                line: draft.line,
            });
            let originals = original_lines(file, draft);
            place(draft_block(draft, originals, text_width), at, &mut blocks);
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

    pub fn line_count(&self) -> usize {
        self.line_rows.len()
    }

    pub fn hunk_count(&self) -> usize {
        self.hunk_starts.len()
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
    let place = match (thread.line, thread.outdated) {
        (Some(line), false) => format!("line {line}"),
        _ => "outdated".to_string(),
    };
    let mut title = format!("thread · {place}");
    if thread.resolved {
        title.push_str(" · ✓ resolved");
    }
    let mut lines = Vec::new();
    for comment in &thread.comments {
        lines.push(BlockLine {
            kind: BlockLineKind::Meta,
            text: format!("{} · {}", comment.author, age(now, comment.created_at)),
        });
        push_wrapped(&mut lines, &comment.body, width);
    }
    Block {
        kind: BlockKind::Thread {
            resolved: thread.resolved,
            outdated: thread.outdated,
        },
        title,
        lines,
    }
}

fn draft_block(draft: &DraftComment, originals: Vec<String>, width: usize) -> Block {
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
                text: expand_tabs(&text, TAB_WIDTH),
            });
        }
        for text in replacement {
            lines.push(BlockLine {
                kind: BlockLineKind::Added,
                text: expand_tabs(&text, TAB_WIDTH),
            });
        }
    }
    Block {
        kind: BlockKind::Pending { suggestion },
        title: format!("pending {what} · {place}"),
        lines,
    }
}

fn push_wrapped(out: &mut Vec<BlockLine>, text: &str, width: usize) {
    let mut wrapped = wrap(text.trim(), width);
    while wrapped.last().is_some_and(String::is_empty) {
        wrapped.pop();
    }
    out.extend(wrapped.into_iter().map(|text| BlockLine {
        kind: BlockLineKind::Text,
        text,
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
            resolved: false,
            outdated: false,
            comments: vec![
                Comment {
                    id: CommentId("c1".into()),
                    author: "jo".into(),
                    body: "Should this wrap round?".into(),
                    created_at: Timestamp(0),
                },
                Comment {
                    id: CommentId("c2".into()),
                    author: "ada".into(),
                    body: "Yes.".into(),
                    created_at: Timestamp(3_600),
                },
            ],
        }
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
            },
            100,
        );
        assert!(rows.is_empty());
        assert_eq!(rows.first_line(), None);
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
            },
            120,
        );
        assert_eq!(rows.line_count(), 5000);
        assert_eq!(visible(2500, 30, rows.len()).len(), 30);
        assert_eq!(rows.step(1, 4999), 5000);
    }
}
