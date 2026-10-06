//! The unified diff screen (frames 1d and 1f): the Files pane with its review block, and the
//! Diff pane with a line cursor. Only the rows in view are built into styled lines.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use rb_core::{ChangeSummary, MyReview};
use rb_diff::{expand_tabs, DiffBody, DiffLine, LineKind};
use rb_theme::Role;
use unicode_width::UnicodeWidthChar;

use super::text::cells;
use super::{chrome::truncate, layout, style, HitMap};
use crate::app::diffview::{self, BlockKind, BlockLine, BlockLineKind, Row, GUTTER};
use crate::app::{Action, App, DiffFocus, DiffState, Phase};

const PLACEHOLDER: &str = "·  ·  ·";

pub fn draw(frame: &mut Frame, app: &App, body: ratatui::layout::Rect, hits: &mut HitMap) {
    let Some(state) = app.diff_state() else {
        return;
    };
    let l = layout::diff_screen(body);
    hits.push(l.files, Action::DiffFocus(DiffFocus::Files));
    hits.push(l.diff, Action::DiffFocus(DiffFocus::Diff));
    draw_files(frame, app, state, &l, hits);
    draw_diff(frame, app, state, &l, hits);
    super::composer::draw(frame, app, state, &l, body, hits);
}

fn pane_block(app: &App, title: String, focused: bool) -> Block<'static> {
    let palette = &app.palette;
    let (border, title_style) = if focused {
        (
            style::fg(palette, Role::Accent),
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            style::fg(palette, Role::Line),
            style::fg(palette, Role::TextSecondary),
        )
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Line::styled(format!(" {title} "), title_style))
}

fn muted(app: &App, text: impl Into<String>) -> Line<'static> {
    Line::styled(text.into(), style::fg(&app.palette, Role::Muted))
}

fn draw_files(
    frame: &mut Frame,
    app: &App,
    state: &DiffState,
    l: &layout::DiffLayout,
    hits: &mut HitMap,
) {
    let palette = &app.palette;
    let focused = state.focus == DiffFocus::Files;
    let files = state.files();
    let title = if files.is_empty() {
        "files".to_string()
    } else {
        format!("files {}", files.len())
    };
    frame.render_widget(pane_block(app, title, focused), l.files);

    if files.is_empty() {
        let text = match &state.phase {
            Phase::Loading => PLACEHOLDER,
            Phase::Failed(_) => "No files to show.",
            Phase::Ready => "This change has no files.",
        };
        frame.render_widget(
            Paragraph::new(muted(app, text)).alignment(Alignment::Center),
            Rect::new(
                l.file_list.x,
                l.file_list.y + 1,
                l.file_list.width,
                1.min(l.file_list.height),
            ),
        );
    }

    let width = usize::from(l.file_list.width);
    let window = diffview::visible(
        state.files_scroll,
        usize::from(l.file_list.height),
        files.len(),
    );
    for (offset, index) in window.enumerate() {
        let file = &files[index];
        let current = index == state.file;
        let rect = Rect::new(
            l.file_list.x,
            l.file_list.y + offset as u16,
            l.file_list.width,
            1,
        );
        let counts = format!("+{} −{}", file.adds, file.dels);
        let room = width.saturating_sub(4 + cells(&counts) + 1);
        let name = tail_fit(&file.diff.path, room);
        let name_style = if current {
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD)
        } else {
            style::fg(palette, Role::Text)
        };
        let mut spans = vec![
            Span::styled(
                if current { "› " } else { "  " },
                style::fg(palette, Role::Accent),
            ),
            Span::styled("· ", style::fg(palette, Role::Muted)),
            Span::styled(name.clone(), name_style),
        ];
        let pad = width.saturating_sub(4 + cells(&name) + cells(&counts));
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(
            format!("+{}", file.adds),
            style::fg(palette, Role::Success),
        ));
        spans.push(Span::styled(
            format!(" −{}", file.dels),
            style::fg(palette, Role::Danger),
        ));
        let base = if current && focused {
            style::bg(palette, Role::Selection)
        } else {
            Style::default()
        };
        frame.render_widget(Paragraph::new(Line::from(spans)).style(base), rect);
        hits.push(rect, Action::DiffFile(index));
    }
    draw_review(frame, app, state, l.review);
}

/// The Your review block: pending comments, files viewed, and where your verdict stands.
fn draw_review(frame: &mut Frame, app: &App, state: &DiffState, area: Rect) {
    let palette = &app.palette;
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(style::fg(palette, Role::Line))
        .title(Line::styled(
            " Your review · pending ",
            style::fg(palette, Role::TextSecondary),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = |s: String| Line::styled(s, style::fg(palette, Role::Text));
    let mut lines = Vec::new();
    match &state.data {
        None => lines.push(muted(app, PLACEHOLDER)),
        Some(data) => {
            let (suggestions, comments) = data.draft.comments.iter().fold((0, 0), |(s, c), d| {
                if d.body.contains("```suggestion") {
                    (s + 1, c)
                } else {
                    (s, c + 1)
                }
            });
            lines.push(if suggestions + comments == 0 {
                muted(app, "Nothing pending yet.")
            } else {
                text(format!(
                    "{} · {}",
                    plural(suggestions, "suggestion"),
                    plural(comments, "comment")
                ))
            });
            lines.push(text(format!(
                "0 of {} viewed",
                plural(data.files.len(), "file")
            )));
        }
    }
    if let Some(change) = app.state.changes.iter().find(|c| c.id == state.id) {
        lines.push(text(format!("Your verdict: {}", verdict_text(change))));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn verdict_text(change: &ChangeSummary) -> &'static str {
    match change.my_review {
        MyReview::None => "not started",
        MyReview::Approved => "✓ approved",
        MyReview::ChangesRequested => "✎ asked for changes",
        MyReview::Commented => "✎ commented",
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn draw_diff(
    frame: &mut Frame,
    app: &App,
    state: &DiffState,
    l: &layout::DiffLayout,
    hits: &mut HitMap,
) {
    let palette = &app.palette;
    let focused = state.focus == DiffFocus::Diff;
    let current = state.current();
    let title = match current {
        Some(f) => match &f.diff.old_path {
            Some(old) if *old != f.diff.path => format!("{old} → {}", f.diff.path),
            _ => f.diff.path.clone(),
        },
        None => "diff".to_string(),
    };
    let mut block = pane_block(
        app,
        truncate(&title, usize::from(l.diff.width.saturating_sub(6))),
        focused,
    );
    if let Some(status) = bottom_status(state) {
        block = block.title_bottom(Line::styled(
            format!(" {status} "),
            style::fg(palette, Role::Muted),
        ));
    }
    frame.render_widget(block, l.diff);

    let area = l.code;
    match (&state.phase, current) {
        (Phase::Loading, _) => centre(frame, area, vec![muted(app, "Loading the diff…")]),
        (Phase::Failed(message), _) => centre(
            frame,
            area,
            vec![
                Line::styled(
                    format!("The diff didn't load: {message}"),
                    style::fg(palette, Role::Warning),
                ),
                muted(app, "Press esc to go back, then try again."),
            ],
        ),
        (Phase::Ready, None) => centre(frame, area, vec![muted(app, "This change has no files.")]),
        (Phase::Ready, Some(file)) => match &file.diff.body {
            DiffBody::Fallback(reason) => centre(
                frame,
                area,
                vec![
                    Line::styled(file.diff.path.clone(), style::fg(palette, Role::Text)),
                    muted(app, reason.message()),
                ],
            ),
            DiffBody::Text(_) => draw_rows(frame, app, state, area, hits),
        },
    }
}

fn bottom_status(state: &DiffState) -> Option<String> {
    let data = state.data.as_ref()?;
    let view = &state.view;
    let mut parts = Vec::new();
    if view.rows.hunk_count() > 0 {
        let (at, total) = view.rows.hunk_position(view.cursor);
        parts.push(format!("hunk {at} of {total}"));
    }
    parts.push(format!("file {} of {}", state.file + 1, data.files.len()));
    Some(parts.join(" · "))
}

fn centre(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    let height = (lines.len() as u16).min(area.height);
    let rect = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(height) / 2,
        area.width,
        height,
    );
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), rect);
}

fn draw_rows(frame: &mut Frame, app: &App, state: &DiffState, area: Rect, hits: &mut HitMap) {
    let view = &state.view;
    let window = diffview::visible(view.scroll, usize::from(area.height), view.rows.len());
    for (offset, index) in window.enumerate() {
        let rect = Rect::new(area.x, area.y + offset as u16, area.width, 1);
        let (line, base) = row_line(app, state, index, usize::from(area.width));
        frame.render_widget(Paragraph::new(line).style(base), rect);
        hits.push(rect, Action::DiffRow(index));
    }
}

fn row_line(app: &App, state: &DiffState, index: usize, width: usize) -> (Line<'static>, Style) {
    let palette = &app.palette;
    let view = &state.view;
    match view.rows.row(index) {
        Some(Row::Hunk(h)) => {
            let text = state
                .current()
                .and_then(|f| f.diff.parsed())
                .and_then(|p| p.hunk(h as usize))
                .map(|hunk| hunk.header.render())
                .unwrap_or_default();
            let spans = clip(
                vec![
                    Span::raw(" ".repeat(usize::from(GUTTER))),
                    Span::styled(
                        expand_tabs(&text, app.tab_width),
                        style::fg(palette, Role::Cyan),
                    ),
                ],
                width,
            );
            (Line::from(spans), style::bg(palette, Role::Raised))
        }
        Some(Row::Line(id)) => {
            let Some(line) = state
                .current()
                .and_then(|f| f.diff.parsed())
                .and_then(|p| p.line(id))
            else {
                return (Line::raw(""), Style::default());
            };
            let at_cursor = index == view.cursor;
            let in_range = state.range.is_some_and(|r| r.contains(index));
            diff_line(app, state, id, line, (at_cursor, in_range), width)
        }
        Some(Row::Block { block, line }) => match view.rows.block(block) {
            Some(b) => (block_line(app, b, line as usize, width), Style::default()),
            None => (Line::raw(""), Style::default()),
        },
        None => (Line::raw(""), Style::default()),
    }
}

fn diff_line(
    app: &App,
    state: &DiffState,
    id: rb_diff::LineId,
    line: &DiffLine,
    (at_cursor, in_range): (bool, bool),
    width: usize,
) -> (Line<'static>, Style) {
    let palette = &app.palette;
    let (tint, sign, sign_role) = match line.kind {
        LineKind::Added => (Some(Role::AddedBg), "+", Role::Success),
        LineKind::Removed => (Some(Role::RemovedBg), "-", Role::Danger),
        LineKind::Context => (None, " ", Role::Muted),
    };
    let number = |n: Option<u32>| match n {
        Some(n) => format!("{n:>4} "),
        None => "     ".to_string(),
    };
    let marker = if at_cursor {
        Span::styled(
            "› ",
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        )
    } else if in_range {
        Span::styled("▌ ", style::fg(palette, Role::Accent))
    } else {
        Span::raw("  ")
    };
    let mut spans = vec![
        marker,
        Span::styled(number(line.old_no), style::fg(palette, Role::Muted)),
        Span::styled(number(line.new_no), style::fg(palette, Role::Muted)),
        Span::styled(format!("{sign} "), style::fg(palette, sign_role)),
    ];
    match state.view.spans(id) {
        Some(parts) => spans.extend(
            parts
                .iter()
                .map(|p| Span::styled(p.text.clone(), style::style(p.style(palette)))),
        ),
        None => spans.push(Span::styled(
            expand_tabs(&line.text, app.tab_width),
            style::fg(palette, Role::Text),
        )),
    }
    let base = if in_range || (at_cursor && state.focus == DiffFocus::Diff) {
        style::bg(palette, Role::Selection)
    } else {
        tint.map_or_else(Style::default, |role| style::bg(palette, role))
    };
    (Line::from(clip(spans, width)), base)
}

fn block_line(
    app: &App,
    block: &crate::app::diffview::Block,
    line: usize,
    width: usize,
) -> Line<'static> {
    let palette = &app.palette;
    let gutter = usize::from(GUTTER);
    let total = width.saturating_sub(gutter).max(6);
    let border_role = match block.kind {
        BlockKind::Thread { resolved: true, .. } => Role::Muted,
        BlockKind::Thread { .. } => Role::Interactive,
        BlockKind::Pending { .. } => Role::Accent,
    };
    let border = style::fg(palette, border_role);
    let mut spans = vec![Span::raw(" ".repeat(gutter))];
    let last = block.height() - 1;
    if line == 0 {
        let head = format!("╭─ {} ", truncate(&block.title, total.saturating_sub(6)));
        let fill = total.saturating_sub(cells(&head) + 1);
        spans.push(Span::styled(head, border.add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!("{}╮", "─".repeat(fill)), border));
    } else if line == last {
        spans.push(Span::styled(
            format!("╰{}╯", "─".repeat(total.saturating_sub(2))),
            border,
        ));
    } else if let Some(content) = block.lines.get(line - 1) {
        let inner = total.saturating_sub(4);
        let (prefix, text_style, tint) = block_line_look(app, content);
        let mut body = clip(
            vec![Span::styled(
                format!("{prefix}{}", content.text),
                text_style,
            )],
            inner,
        );
        let used: usize = body.iter().map(|s| cells(&s.content)).sum();
        body.push(Span::raw(" ".repeat(inner.saturating_sub(used))));
        spans.push(Span::styled("│ ", border));
        spans.extend(body.into_iter().map(|s| match tint {
            Some(bg) => s.patch_style(bg),
            None => s,
        }));
        spans.push(Span::styled(" │", border));
    }
    Line::from(spans)
}

fn block_line_look(app: &App, line: &BlockLine) -> (&'static str, Style, Option<Style>) {
    let palette = &app.palette;
    match line.kind {
        BlockLineKind::Meta => (
            "",
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
            None,
        ),
        BlockLineKind::Text => ("", style::fg(palette, Role::Text), None),
        BlockLineKind::Removed => (
            "- ",
            style::fg(palette, Role::Danger),
            Some(style::bg(palette, Role::RemovedBg)),
        ),
        BlockLineKind::Added => (
            "+ ",
            style::fg(palette, Role::Success),
            Some(style::bg(palette, Role::AddedBg)),
        ),
    }
}

/// Cuts a row to `max` cells, ending in an ellipsis when something was lost.
pub fn clip(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|s| cells(&s.content)).sum();
    if total <= max {
        return spans;
    }
    let budget = max.saturating_sub(1);
    let mut used = 0;
    let mut out = Vec::new();
    let mut last_style = Style::default();
    for span in spans {
        let mut kept = String::new();
        for ch in span.content.chars() {
            let w = ch.width().unwrap_or(0);
            if used + w > budget {
                break;
            }
            used += w;
            kept.push(ch);
        }
        let whole = kept.len() == span.content.len();
        last_style = span.style;
        if !kept.is_empty() {
            out.push(Span::styled(kept, span.style));
        }
        if !whole {
            break;
        }
    }
    if max > 0 {
        out.push(Span::styled("…", last_style));
    }
    out
}

/// Shortens a path from the left so the file name stays readable.
fn tail_fit(path: &str, room: usize) -> String {
    if cells(path) <= room {
        return path.to_string();
    }
    let mut tail: Vec<char> = Vec::new();
    let mut used = 1;
    for ch in path.chars().rev() {
        let w = ch.width().unwrap_or(0);
        if used + w > room {
            break;
        }
        used += w;
        tail.push(ch);
    }
    tail.reverse();
    format!("…{}", tail.into_iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn clip_leaves_short_rows_alone_and_marks_cuts() {
        let spans = vec![Span::raw("hello "), Span::raw("world")];
        assert_eq!(text(&clip(spans.clone(), 20)), "hello world");
        assert_eq!(text(&clip(spans.clone(), 8)), "hello w…");
        assert_eq!(text(&clip(spans.clone(), 6)), "hello…");
        assert_eq!(text(&clip(spans, 0)), "");
    }

    #[test]
    fn clip_respects_wide_characters() {
        let out = clip(vec![Span::raw("日本語です")], 5);
        assert_eq!(text(&out), "日本…");
    }

    #[test]
    fn tail_fit_keeps_the_end_of_a_path() {
        assert_eq!(tail_fit("src/ui/menus.rs", 30), "src/ui/menus.rs");
        assert_eq!(tail_fit("src/ui/menus.rs", 9), "…menus.rs");
        assert_eq!(tail_fit("a", 0), "…");
    }
}
