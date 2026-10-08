//! The unified diff screen (frames 1d and 1f): the Files pane with its review block, and the
//! Diff pane with a line cursor. Only the rows in view are built into styled lines.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use rb_core::{ChangeSummary, FeatureAction, MyReview};
use rb_diff::{expand_tabs, DiffBody, DiffLine, LineKind};
use rb_theme::Role;
use unicode_width::UnicodeWidthChar;

use super::text::{cells, split_spans, wrap_points, Join};
use super::textmap::{RegionKey, TextRegion, TextRow};
use super::{chrome::truncate, layout, style, HitMap};
use crate::app::diffview::{self, BlockKind, BlockLine, BlockLineKind, Row, GUTTER};
use crate::app::{Action, App, DiffFocus, DiffState, Phase};

/// The most cells the source label takes in the change header.
const SOURCE_LABEL_MAX: usize = 24;

const PLACEHOLDER: &str = "·  ·  ·";

pub fn draw(frame: &mut Frame, app: &App, body: ratatui::layout::Rect, hits: &mut HitMap) {
    let Some(state) = app.diff_state() else {
        return;
    };
    let l = layout::diff_screen_with(body, crate::app::comments::review_extra(state));
    draw_header(frame, app, state, l.header);
    hits.push(l.files, Action::DiffFocus(DiffFocus::Files));
    hits.push(l.diff, Action::DiffFocus(DiffFocus::Diff));
    draw_files(frame, app, state, &l, hits);
    draw_diff(frame, app, state, &l, hits);
    super::composer::draw(frame, app, state, &l, body, hits);
}

/// The line naming the change under review: forge badge, `owner/repo#number`, source, and title,
/// then author and branches when the whole of them fits. The title gives way first.
fn draw_header(frame: &mut Frame, app: &App, state: &DiffState, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let palette = &app.palette;
    let id = &state.id;
    let change = app.state.changes.iter().find(|c| c.id == *id);
    let source = app.state.source(&id.source_id);
    let sep = || Span::styled(" · ", style::fg(palette, Role::Muted));
    let room = usize::from(area.width);

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            id.kind.tag(),
            style::tag_fg(palette, source, id.kind).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(id.short_ref(), style::fg(palette, Role::Cyan)),
    ];
    if let Some(source) = source {
        spans.push(sep());
        spans.push(Span::styled(
            truncate(&source.label, SOURCE_LABEL_MAX),
            style::fg(palette, Role::TextSecondary),
        ));
    }
    spans.push(sep());
    let used: usize = spans.iter().map(|s| cells(&s.content)).sum();
    let title = change.map(|c| c.title.as_str());
    let room_for_title = room.saturating_sub(used + 1);
    let extras = change.filter(|c| {
        let extra = 3 + cells(&c.author) + 3 + cells(&c.branch) + 3 + cells(&c.base);
        title.map_or(0, cells) + extra <= room_for_title
    });
    match title {
        Some(title) => spans.push(Span::styled(
            truncate(title, room_for_title),
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        )),
        None => spans.push(Span::styled(
            truncate("title not loaded yet", room_for_title),
            style::fg(palette, Role::Muted),
        )),
    }
    if let Some(change) = extras {
        spans.push(sep());
        spans.push(Span::styled(
            change.author.clone(),
            style::fg(palette, Role::Interactive),
        ));
        spans.push(sep());
        spans.push(Span::styled(
            change.branch.clone(),
            style::fg(palette, Role::Cyan),
        ));
        spans.push(Span::styled(" → ", style::fg(palette, Role::Muted)));
        spans.push(Span::styled(
            change.base.clone(),
            style::fg(palette, Role::Cyan),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
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
    let mut paths = TextRegion::new(RegionKey::Files, l.file_list);
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
        let mut row = TextRow::new(
            index,
            (rect.x + 4, rect.y),
            cells(&name) as u16,
            name.clone(),
        );
        if name != file.diff.path {
            row = row.copying(file.diff.path.clone());
        }
        paths.rows.push(row);
    }
    hits.texts.push(paths);
    draw_review(frame, app, state, l.review, hits);
}

/// The Your review block: pending comments, files viewed, and where your verdict stands.
fn draw_review(frame: &mut Frame, app: &App, state: &DiffState, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let focused = state.focus == DiffFocus::Review;
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(style::fg(
            palette,
            if focused { Role::Accent } else { Role::Line },
        ))
        .title(Line::styled(
            " Your review · pending ",
            if focused {
                style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD)
            } else {
                style::fg(palette, Role::TextSecondary)
            },
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
        lines.push(text(format!("Your review: {}", verdict_text(change))));
    }
    if let (Some(verdict), None) = (state.verdict, &state.review) {
        lines.push(text(format!(
            "Draft verdict: {}",
            crate::app::review::verdict_label(verdict).to_lowercase()
        )));
    }
    if state.stale {
        lines.push(muted(app, "Code changed since you wrote this"));
    }
    list_pending(app, state, inner, &mut lines, hits);
    if let Some(modal) = &state.review {
        lines.push(text(format!(
            "Choosing: {}",
            crate::app::review::verdict_label(modal.verdict).to_lowercase()
        )));
    } else if state.data.is_some() {
        lines.push(muted(app, "a approve · c comment"));
        let can_request = app
            .state
            .supports(&state.id.source_id, FeatureAction::RequestChanges);
        lines.push(muted(
            app,
            if can_request {
                "x request changes · R review"
            } else {
                "R post a review"
            },
        ));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// One row per pending comment (the window around the selection), clickable.
fn list_pending(
    app: &App,
    state: &DiffState,
    inner: Rect,
    lines: &mut Vec<Line<'static>>,
    hits: &mut HitMap,
) {
    use crate::app::comments::{entries, LIST_ROWS};
    let palette = &app.palette;
    let all = entries(state);
    if all.is_empty() {
        return;
    }
    let selected = state.review_sel.min(all.len() - 1);
    let start = selected
        .saturating_sub(LIST_ROWS - 1)
        .min(all.len().saturating_sub(LIST_ROWS));
    let width = usize::from(inner.width);
    for (n, entry) in all.iter().enumerate().skip(start).take(LIST_ROWS) {
        let at = state.focus == DiffFocus::Review && n == selected;
        let mut label = entry.label();
        if entry.outdated {
            label.push_str(" · outdated");
        }
        let text = format!("{} {}", if at { "▸" } else { " " }, label);
        let y = inner.y + lines.len() as u16;
        if y < inner.bottom() {
            hits.push(
                Rect::new(inner.x, y, inner.width, 1),
                Action::JumpComment(n),
            );
        }
        let role = if entry.outdated {
            Role::Muted
        } else {
            Role::Accent
        };
        let line = Line::styled(
            super::chrome::truncate(&text, width),
            style::fg(palette, role),
        );
        lines.push(if at {
            line.style(style::bg(palette, Role::Selection))
        } else {
            line
        });
    }
}

fn verdict_text(change: &ChangeSummary) -> &'static str {
    match change.my_review {
        MyReview::None => "not started",
        MyReview::Approved => "✓ approved",
        MyReview::ChangesRequested => "✎ changes requested",
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
    if let Some(status) = bottom_status(state, app.diff_wrap) {
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

fn bottom_status(state: &DiffState, wrap: bool) -> Option<String> {
    let data = state.data.as_ref()?;
    let view = &state.view;
    let mut parts = Vec::new();
    if view.rows.hunk_count() > 0 {
        let (at, total) = view.rows.hunk_position(view.cursor);
        parts.push(format!("hunk {at} of {total}"));
    }
    parts.push(format!("file {} of {}", state.file + 1, data.files.len()));
    if wrap {
        parts.push("wrap".to_string());
    }
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
    let map = &view.screen;
    let height = usize::from(area.height);
    let width = usize::from(area.width);
    let Some((first, skip)) = map.locate(view.scroll) else {
        return;
    };
    let mut y = 0;
    let mut index = first;
    while y < height && index < view.rows.len() {
        let skip = if index == first { skip } else { 0 };
        let wrapped = match view.rows.row(index) {
            Some(Row::Line(id)) if map.wrapping() => Some(id),
            _ => None,
        };
        if let Some(id) = wrapped {
            y += draw_wrapped(frame, app, state, (index, id, skip), (area, y), hits);
        } else {
            let rect = Rect::new(area.x, area.y + y as u16, area.width, 1);
            let (line, base) = row_line(app, state, index, width);
            frame.render_widget(Paragraph::new(line).style(base), rect);
            hits.push(rect, Action::DiffRow(index));
            draw_reply_hint(state, index, rect, area, hits);
            y += 1;
        }
        index += 1;
    }
    record_text(app, state, area, hits);
}

/// Records the code and the comment text on screen, as logical rows a selection can copy: no
/// gutter, border, padding or `-`/`+` prefix, and a wrapped line's rows marked as continuing it.
fn record_text(app: &App, state: &DiffState, area: Rect, hits: &mut HitMap) {
    let view = &state.view;
    let map = &view.screen;
    let Some(patch) = state.current().and_then(|f| f.diff.parsed()) else {
        return;
    };
    let height = usize::from(area.height);
    let on_screen = |ord: usize| {
        (view.scroll..view.scroll + height)
            .contains(&ord)
            .then(|| area.y + (ord - view.scroll) as u16)
    };
    let gutter = GUTTER.min(area.width);
    let code = Rect::new(area.x + gutter, area.y, area.width - gutter, area.height);
    let text_width = usize::from(code.width).max(1);
    let ahead = if app
        .selection
        .as_ref()
        .is_some_and(|s| s.key == RegionKey::Diff)
    {
        crate::app::selection::LOOKAHEAD
    } else {
        0
    };
    let from = view.scroll.saturating_sub(ahead);
    let mut region = TextRegion::new(RegionKey::Diff, code);
    for index in map.rows_in(from, height + (view.scroll - from) + ahead) {
        let Some(Row::Line(id)) = view.rows.row(index) else {
            continue;
        };
        let Some(line) = patch.line(id) else {
            continue;
        };
        let expanded = expand_tabs(&line.text, app.tab_width);
        let pieces: Vec<String> = if map.wrapping() {
            let points = wrap_points(&line.text, app.tab_width, text_width);
            split_spans(vec![Span::raw(expanded)], &points)
                .into_iter()
                .map(|row| row.iter().map(|s| s.content.as_ref()).collect())
                .collect()
        } else {
            vec![expanded]
        };
        for (sub, text) in pieces.into_iter().enumerate() {
            let ord = map.start(index) + sub;
            let y = on_screen(ord).unwrap_or(TextRow::OFF_SCREEN);
            let join = if sub == 0 { Join::Break } else { Join::Glue };
            region
                .rows
                .push(TextRow::new(ord, (code.x, y), code.width, text).joined(join));
        }
    }
    hits.texts.push(region);

    // Comment and thread blocks: one region each, its lines as written, inside the border.
    let mut touched = std::collections::BTreeMap::new();
    for index in map.rows_in(view.scroll, height) {
        if let Some(Row::Block { block, line }) = view.rows.row(index) {
            touched.insert(block, index - line as usize);
        }
    }
    let total = usize::from(area.width)
        .saturating_sub(usize::from(GUTTER))
        .max(6);
    let inner = total.saturating_sub(4);
    let x = area.x + GUTTER + 2;
    for (block, first) in touched {
        let Some(b) = view.rows.block(block) else {
            continue;
        };
        let mut rows = Vec::new();
        for (i, content) in b.lines.iter().enumerate() {
            let prefix = block_prefix(content.kind);
            let ord = map.start(first + 1 + i);
            let y = on_screen(ord).unwrap_or(TextRow::OFF_SCREEN);
            let span = inner.saturating_sub(prefix.len());
            rows.push(
                TextRow::new(
                    i,
                    (x + prefix.len() as u16, y),
                    span as u16,
                    content.text.clone(),
                )
                .joined(content.join),
            );
        }
        let shown: Vec<u16> = rows.iter().filter(|r| r.on_screen()).map(|r| r.y).collect();
        let (Some(&top), Some(&bottom)) = (shown.iter().min(), shown.iter().max()) else {
            continue;
        };
        let mut region = TextRegion::new(
            RegionKey::Block(block),
            Rect::new(x, top, inner as u16, bottom - top + 1),
        );
        region.rows = rows;
        hits.texts.push(region);
    }
}

/// The marker a suggestion line carries inside its block, which is never part of the text.
fn block_prefix(kind: BlockLineKind) -> &'static str {
    match kind {
        BlockLineKind::Removed => "- ",
        BlockLineKind::Added => "+ ",
        _ => "",
    }
}

fn draw_reply_hint(state: &DiffState, index: usize, rect: Rect, area: Rect, hits: &mut HitMap) {
    let Some(Row::Block { block, line }) = state.view.rows.row(index) else {
        return;
    };
    let Some(b) = state.view.rows.block(block) else {
        return;
    };
    if line as usize + 1 == b.height() {
        let total = usize::from(area.width)
            .saturating_sub(usize::from(GUTTER))
            .max(6);
        if let Some(fill) = reply_hint_offset(b, total) {
            let x = area.x + GUTTER + 1 + fill as u16;
            let hint = Rect::new(x, rect.y, cells(REPLY_HINT) as u16, 1);
            hits.push(hint, Action::ReplyAt(index));
        }
    }
}

/// Draws the screen rows of a wrapped diff line from `skip`, starting `y` rows into `area`, and
/// returns how many it drew.
fn draw_wrapped(
    frame: &mut Frame,
    app: &App,
    state: &DiffState,
    (index, id, skip): (usize, rb_diff::LineId, usize),
    (area, y): (Rect, usize),
    hits: &mut HitMap,
) -> usize {
    let Some(line) = state
        .current()
        .and_then(|f| f.diff.parsed())
        .and_then(|p| p.line(id))
    else {
        return 1;
    };
    let view = &state.view;
    let flags = (
        index == view.cursor,
        state.range.is_some_and(|r| r.contains(index)),
    );
    let (lines, base) = wrapped_line(
        app,
        state,
        (id, line),
        flags,
        usize::from(area.width),
        view.screen.height(index),
    );
    let room = usize::from(area.height) - y;
    let mut drawn = 0;
    for line in lines.into_iter().skip(skip).take(room) {
        let rect = Rect::new(area.x, area.y + (y + drawn) as u16, area.width, 1);
        frame.render_widget(Paragraph::new(line).style(base), rect);
        hits.push(rect, Action::DiffRow(index));
        drawn += 1;
    }
    drawn
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
            Some(b) => (
                block_line(
                    app,
                    b,
                    line as usize,
                    width,
                    line == 0 && crate::app::comments::picked_block(state) == Some(block),
                ),
                Style::default(),
            ),
            None => (Line::raw(""), Style::default()),
        },
        None => (Line::raw(""), Style::default()),
    }
}

fn line_look(kind: LineKind) -> (Option<Role>, &'static str, Role) {
    match kind {
        LineKind::Added => (Some(Role::AddedBg), "+", Role::Success),
        LineKind::Removed => (Some(Role::RemovedBg), "-", Role::Danger),
        LineKind::Context => (None, " ", Role::Muted),
    }
}

fn marker_span(app: &App, at_cursor: bool, in_range: bool) -> Span<'static> {
    let palette = &app.palette;
    if at_cursor {
        Span::styled(
            "› ",
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        )
    } else if in_range {
        Span::styled("▌ ", style::fg(palette, Role::Accent))
    } else {
        Span::raw("  ")
    }
}

/// The row's background: the selection while it is the cursor line or in a range, else the
/// added or removed tint.
fn line_base(
    app: &App,
    state: &DiffState,
    tint: Option<Role>,
    (at_cursor, in_range): (bool, bool),
) -> Style {
    let palette = &app.palette;
    if in_range || (at_cursor && state.focus == DiffFocus::Diff) {
        style::bg(palette, Role::Selection)
    } else {
        tint.map_or_else(Style::default, |role| style::bg(palette, role))
    }
}

/// The styled code of a line: syntax spans when highlighted, plain text otherwise, tabs expanded.
fn code_spans(
    app: &App,
    state: &DiffState,
    id: rb_diff::LineId,
    line: &DiffLine,
) -> Vec<Span<'static>> {
    let palette = &app.palette;
    match state.view.spans(id) {
        Some(parts) => parts
            .iter()
            .map(|p| Span::styled(p.text.clone(), style::style(p.style(palette))))
            .collect(),
        None => vec![Span::styled(
            expand_tabs(&line.text, app.tab_width),
            style::fg(palette, Role::Text),
        )],
    }
}

fn number(n: Option<u32>) -> String {
    match n {
        Some(n) => format!("{n:>4} "),
        None => "     ".to_string(),
    }
}

fn diff_line(
    app: &App,
    state: &DiffState,
    id: rb_diff::LineId,
    line: &DiffLine,
    flags: (bool, bool),
    width: usize,
) -> (Line<'static>, Style) {
    let palette = &app.palette;
    let (tint, sign, sign_role) = line_look(line.kind);
    let mut spans = vec![
        marker_span(app, flags.0, flags.1),
        Span::styled(number(line.old_no), style::fg(palette, Role::Muted)),
        Span::styled(number(line.new_no), style::fg(palette, Role::Muted)),
        Span::styled(format!("{sign} "), style::fg(palette, sign_role)),
    ];
    spans.extend(code_spans(app, state, id, line));
    (
        Line::from(clip(spans, width)),
        line_base(app, state, tint, flags),
    )
}

/// The screen rows of a wrapped line, `height` of them. The first row looks like an unwrapped
/// line; each continuation row has a blank cursor column (a range marker if the line is in a
/// range), a dim `↪` where the line numbers were, the sign, and the next piece of code. The
/// tint is the returned base style, which covers every row.
fn wrapped_line(
    app: &App,
    state: &DiffState,
    (id, line): (rb_diff::LineId, &DiffLine),
    flags: (bool, bool),
    width: usize,
    height: usize,
) -> (Vec<Line<'static>>, Style) {
    let palette = &app.palette;
    let (tint, sign, sign_role) = line_look(line.kind);
    let text_width = width.saturating_sub(usize::from(GUTTER)).max(1);
    let points = wrap_points(&line.text, app.tab_width, text_width);
    let mut pieces = split_spans(code_spans(app, state, id, line), &points);
    pieces.resize(height.max(1), Vec::new());
    let muted = style::fg(palette, Role::Muted);
    let arrow = |has: bool| {
        Span::styled(
            if has { "   ↪ " } else { "     " },
            muted.add_modifier(Modifier::DIM),
        )
    };
    let lines = pieces
        .into_iter()
        .enumerate()
        .map(|(row, piece)| {
            let mut spans = if row == 0 {
                vec![
                    marker_span(app, flags.0, flags.1),
                    Span::styled(number(line.old_no), muted),
                    Span::styled(number(line.new_no), muted),
                ]
            } else {
                vec![
                    marker_span(app, false, flags.1),
                    arrow(line.old_no.is_some()),
                    arrow(line.new_no.is_some()),
                ]
            };
            spans.push(Span::styled(
                format!("{sign} "),
                style::fg(palette, sign_role),
            ));
            spans.extend(piece);
            Line::from(spans)
        })
        .collect();
    (lines, line_base(app, state, tint, flags))
}

const REPLY_HINT: &str = " r reply ";

/// Dashes before the `r reply` hint in a thread block's bottom border, when it fits.
fn reply_hint_offset(block: &crate::app::diffview::Block, total: usize) -> Option<usize> {
    let hint = cells(REPLY_HINT);
    (matches!(block.kind, BlockKind::Thread { .. }) && total >= hint + 8).then(|| total - 3 - hint)
}

fn block_line(
    app: &App,
    block: &crate::app::diffview::Block,
    line: usize,
    width: usize,
    picked: bool,
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
        let mark = if picked { "▸ " } else { "" };
        let head = format!(
            "╭─ {mark}{} ",
            truncate(&block.title, total.saturating_sub(6 + mark.len()))
        );
        let fill = total.saturating_sub(cells(&head) + 1);
        spans.push(Span::styled(head, border.add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!("{}╮", "─".repeat(fill)), border));
    } else if line == last {
        match reply_hint_offset(block, total) {
            Some(fill) => {
                spans.push(Span::styled(format!("╰{}", "─".repeat(fill)), border));
                spans.push(Span::styled(REPLY_HINT, style::fg(palette, Role::Muted)));
                spans.push(Span::styled("─╯", border));
            }
            None => spans.push(Span::styled(
                format!("╰{}╯", "─".repeat(total.saturating_sub(2))),
                border,
            )),
        }
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
            block_prefix(BlockLineKind::Removed),
            style::fg(palette, Role::Danger),
            Some(style::bg(palette, Role::RemovedBg)),
        ),
        BlockLineKind::Added => (
            block_prefix(BlockLineKind::Added),
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
