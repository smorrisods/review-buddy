//! The 1a dashboard: Sources, Queue and Detail panes, or Queue and Detail under a strip of
//! source tabs when the terminal is narrower than [`layout::COLLAPSE_BELOW`].

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use rb_core::{CiState, Source};
use rb_theme::Role;

use super::text::{cells, justify};
use super::{chrome::truncate, detail, layout, style, HitMap};
use crate::app::{
    queue::{self, Item, Queue, Row},
    Action, App, Pane, Selected,
};

const PLACEHOLDER: &str = "·  ·  ·";
const RULE: &str = "▌ ";
const BLANK_RULE: &str = "  ";

pub fn ci_look(state: CiState) -> (&'static str, Role) {
    match state {
        CiState::Pass => ("●", Role::Success),
        CiState::Running => ("◐", Role::Warning),
        CiState::Fail => ("✕", Role::Danger),
        CiState::None => ("·", Role::Muted),
        CiState::Neutral => ("○", Role::Muted),
        CiState::Skipped => ("↷", Role::Muted),
        CiState::Cancelled => ("⊘", Role::Muted),
    }
}

pub fn draw(frame: &mut Frame, app: &App, body: Rect, hits: &mut HitMap) {
    let l = layout::dashboard(body);
    let mut panes = vec![(l.queue, Pane::Queue), (l.detail, Pane::Detail)];
    if let Some(strip) = l.strip {
        draw_source_strip(frame, app, strip, hits);
    }
    if let Some(sources) = l.sources {
        panes.push((sources, Pane::Sources));
        let inner = pane(frame, app, sources, "sources", Pane::Sources);
        draw_sources(frame, app, inner, hits);
    }
    let inner = pane(frame, app, l.queue, "queue", Pane::Queue);
    draw_queue(frame, app, inner, hits);
    let title = app
        .selected_change()
        .map_or_else(|| "detail".to_string(), |c| c.id.short_ref());
    pane(frame, app, l.detail, &title, Pane::Detail);
    detail::draw(frame, app, l.detail, hits);
    hits.set_panes(panes);
}

/// Draws a pane's border and title. The focused pane gets the accent.
fn pane(frame: &mut Frame, app: &App, area: Rect, title: &str, which: Pane) -> Rect {
    let palette = &app.palette;
    let focused = app.dashboard.focus == which;
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
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Line::styled(format!(" {title} "), title_style));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

fn centred(frame: &mut Frame, app: &App, area: Rect, lines: Vec<Line<'static>>) {
    let height = (lines.len() as u16).min(area.height);
    let rect = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(height) / 2,
        area.width,
        height,
    );
    let _ = app;
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), rect);
}

fn selection_style(app: &App) -> Style {
    style::bg(&app.palette, Role::Selection)
}

fn rule(app: &App, focused: bool) -> Span<'static> {
    let role = if focused { Role::Accent } else { Role::Muted };
    Span::styled(RULE, style::fg(&app.palette, role))
}

fn draw_line(
    frame: &mut Frame,
    area: Rect,
    y: u16,
    line: Line<'static>,
    selected: bool,
    app: &App,
) {
    let base = if selected {
        selection_style(app)
    } else {
        Style::default()
    };
    frame.render_widget(
        Paragraph::new(line).style(base),
        Rect::new(area.x, y, area.width, 1),
    );
}

fn draw_sources(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    if !app.state.loaded {
        let lines = if app.state.loading {
            vec![Line::styled(PLACEHOLDER, style::fg(palette, Role::Muted))]
        } else {
            vec![Line::styled(
                "No sources yet.",
                style::fg(palette, Role::Muted),
            )]
        };
        centred(frame, app, area, lines);
        return;
    }
    let focused = app.dashboard.focus == Pane::Sources;
    let width = usize::from(area.width);
    let mut y = area.y;
    for n in 0..=app.state.sources.len() {
        if y + 2 > area.bottom() {
            break;
        }
        let source = n.checked_sub(1).and_then(|i| app.state.sources.get(i));
        let selected = app.dashboard.source == n;
        let (name, sub, count, dot) = source_row(app, source);
        let lead = if selected {
            rule(app, focused)
        } else {
            Span::raw(BLANK_RULE)
        };
        let name_style = if selected {
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD)
        } else {
            style::fg(palette, Role::Text)
        };
        let room = width.saturating_sub(2 + 2 + cells(&count) + 2);
        let first = justify(
            vec![
                lead,
                Span::styled("● ", dot),
                Span::styled(truncate(&name, room), name_style),
            ],
            vec![Span::styled(
                format!("{count} "),
                style::fg(palette, Role::Muted),
            )],
            width,
        );
        let second = Line::from(vec![
            Span::raw("    "),
            Span::styled(
                truncate(&sub, width.saturating_sub(5)),
                style::fg(palette, Role::Muted),
            ),
        ]);
        draw_line(frame, area, y, first, selected, app);
        draw_line(frame, area, y + 1, second, selected, app);
        hits.push(Rect::new(area.x, y, area.width, 2), Action::SelectSource(n));
        y += 2;
    }
}

fn source_row(app: &App, source: Option<&Source>) -> (String, String, String, Style) {
    let palette = &app.palette;
    match source {
        None => (
            "All".to_string(),
            "every source".to_string(),
            queue::count_in(&app.state, None).to_string(),
            style::fg(palette, Role::Accent),
        ),
        Some(s) => {
            let failure = app.state.failures.get(&s.id);
            (
                s.label.clone(),
                match failure {
                    Some(f) => f.short().to_string(),
                    None if s.in_all => s.host.clone(),
                    None => format!("not in All · {}", s.host),
                },
                queue::count_in(&app.state, Some(&s.id)).to_string(),
                match failure {
                    Some(_) => style::fg(palette, Role::Warning),
                    None => style::tag_fg(palette, Some(s), s.kind),
                },
            )
        }
    }
}

const TAB_NAME_MAX: usize = 18;
const MORE_LEFT: &str = "‹ ";
const MORE_RIGHT: &str = " ›";

/// Which tabs fit in `width` cells: always the active one, scrolled into view, with room kept
/// for a `‹`/`›` marker on each side that has tabs hidden.
pub(super) fn strip_window(
    widths: &[usize],
    gap: usize,
    width: usize,
    active: usize,
) -> std::ops::Range<usize> {
    let n = widths.len();
    if n == 0 {
        return 0..0;
    }
    let active = active.min(n - 1);
    let (left, right) = (cells(MORE_LEFT), cells(MORE_RIGHT));
    let span = |from: usize, to: usize| {
        widths[from..to].iter().sum::<usize>() + gap * (to - from).saturating_sub(1)
    };
    let mut start = active;
    while start > 0 {
        let reserve = if start - 1 > 0 { left } else { 0 };
        if span(start - 1, active + 1) + reserve > width {
            break;
        }
        start -= 1;
    }
    let reserve_left = if start > 0 { left } else { 0 };
    let mut end = active + 1;
    while end < n {
        let reserve = if end + 1 < n { right } else { 0 };
        if span(start, end + 1) + reserve_left + reserve > width {
            break;
        }
        end += 1;
    }
    start..end
}

fn draw_source_strip(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    if !app.state.loaded {
        return;
    }
    let total = app.state.sources.len() + 1;
    let tabs: Vec<Vec<Span<'static>>> = (0..total)
        .map(|n| {
            let source = n.checked_sub(1).and_then(|i| app.state.sources.get(i));
            let (name, _, count, dot) = source_row(app, source);
            let active = app.dashboard.source == n;
            let base = if active {
                style::fg(palette, Role::TextBright)
                    .patch(style::bg(palette, Role::Selection))
                    .add_modifier(Modifier::BOLD)
            } else {
                style::fg(palette, Role::TextSecondary)
            };
            vec![
                Span::styled(
                    format!(" {} ", n + 1),
                    style::fg(palette, Role::Accent).patch(base),
                ),
                Span::styled("● ", dot.patch(base)),
                Span::styled(truncate(&name, TAB_NAME_MAX), base),
                Span::styled(
                    format!(" {count} "),
                    if active {
                        base
                    } else {
                        style::fg(palette, Role::Muted)
                    },
                ),
            ]
        })
        .collect();
    let widths: Vec<usize> = tabs
        .iter()
        .map(|t| t.iter().map(|s| cells(&s.content)).sum())
        .collect();
    let x0 = area.x + 1;
    let width = usize::from(area.width.saturating_sub(1));
    let active = app.dashboard.source;
    let window = strip_window(&widths, 1, width, active);
    let muted = style::fg(palette, Role::Muted);
    let mut spans = Vec::new();
    let mut at = 0usize;
    if window.start > 0 {
        spans.push(Span::styled(MORE_LEFT, muted));
        hits.push(
            Rect::new(x0, area.y, cells(MORE_LEFT) as u16, 1),
            Action::SelectSource(active.saturating_sub(1)),
        );
        at += cells(MORE_LEFT);
    }
    for n in window.clone() {
        if n > window.start {
            spans.push(Span::raw(" "));
            at += 1;
        }
        hits.push(
            Rect::new(x0 + at as u16, area.y, widths[n] as u16, 1),
            Action::SelectSource(n),
        );
        spans.extend(tabs[n].clone());
        at += widths[n];
    }
    if window.end < total {
        spans.push(Span::styled(MORE_RIGHT, muted));
        hits.push(
            Rect::new(x0 + at as u16, area.y, cells(MORE_RIGHT) as u16, 1),
            Action::SelectSource((active + 1).min(total - 1)),
        );
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(x0, area.y, width as u16, 1),
    );
}

fn draw_queue(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    if !app.state.loaded {
        let lines = if app.state.loading {
            vec![Line::styled(PLACEHOLDER, style::fg(palette, Role::Muted))]
        } else {
            vec![
                Line::styled(
                    "Nothing connected yet.",
                    style::fg(palette, Role::TextBright),
                ),
                Line::styled(
                    "Run `review-buddy --setup`,",
                    style::fg(palette, Role::Muted),
                ),
                Line::styled(
                    "or see docs/configuration.md.",
                    style::fg(palette, Role::Muted),
                ),
            ]
        };
        centred(frame, app, area, lines);
        return;
    }
    let queue = app.queue();
    if queue.is_empty() {
        let (head, sub) = empty_copy(app);
        centred(
            frame,
            app,
            area,
            vec![
                Line::styled(head, style::fg(palette, Role::TextBright)),
                Line::styled(sub, style::fg(palette, Role::Muted)),
            ],
        );
        return;
    }
    let items = queue.items();
    let focused = app.dashboard.focus == Pane::Queue;
    let scroll = i32::from(app.dashboard.queue_scroll);
    let mut top = -scroll;
    for row in queue.rows() {
        let height = i32::from(row.height());
        let item_pos = match row {
            Row::Item(item) => items.iter().position(|i| *i == item),
            _ => None,
        };
        let selected = match row {
            Row::Item(item) => is_selected(app, item),
            _ => false,
        };
        for (k, line) in row_lines(app, &queue, row, selected, focused, area.width)
            .into_iter()
            .enumerate()
        {
            let vy = top + k as i32;
            if vy < 0 || vy >= i32::from(area.height) {
                continue;
            }
            let y = area.y + vy as u16;
            draw_line(frame, area, y, line, selected, app);
            if let Some(n) = item_pos {
                hits.push(Rect::new(area.x, y, area.width, 1), Action::SelectItem(n));
            }
        }
        top += height;
    }
}

/// What an empty queue says: nothing connected, a sign-in or other failure, or all clear.
fn empty_copy(app: &App) -> (String, String) {
    let state = &app.state;
    if state.sources.is_empty() {
        return (
            "Nothing connected yet.".to_string(),
            "Run `review-buddy --setup`, or see docs/configuration.md.".to_string(),
        );
    }
    let visible = state
        .sources
        .iter()
        .enumerate()
        .filter(|(i, _)| app.dashboard.source == 0 || app.dashboard.source == i + 1);
    if let Some(failure) = visible.clone().find_map(|(_, s)| state.failures.get(&s.id)) {
        return (failure.headline(), failure.next_step.clone());
    }
    if visible.clone().next().is_some() && state.pending_sources > 0 {
        return (
            "Checking your sources…".to_string(),
            "This usually takes a moment.".to_string(),
        );
    }
    (
        "That's everything.".to_string(),
        "Nothing is waiting on you.".to_string(),
    )
}

fn is_selected(app: &App, item: Item) -> bool {
    match (&app.dashboard.selected, item) {
        (Some(Selected::Noise), Item::Noise) => true,
        (Some(Selected::Change(id)), Item::Change(i)) => {
            app.state.changes.get(i).is_some_and(|c| &c.id == id)
        }
        _ => false,
    }
}

fn row_lines(
    app: &App,
    queue: &Queue,
    row: Row,
    selected: bool,
    focused: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let palette = &app.palette;
    let width = usize::from(width);
    match row {
        Row::Gap => vec![Line::raw("")],
        Row::More(n) => vec![Line::from(vec![
            Span::raw(BLANK_RULE),
            Span::styled(
                format!("+{n} more · raise triage.bucket_limit to see them"),
                style::fg(palette, Role::Muted),
            ),
        ])],
        Row::Heading(bucket, count) => vec![Line::from(vec![
            Span::raw(BLANK_RULE),
            Span::styled(
                bucket.title(),
                style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" · {count}"), style::fg(palette, Role::Muted)),
        ])],
        Row::End => {
            let n = queue::count_in(&app.state, app.active_source().map(|s| &s.id));
            vec![
                Line::from(vec![
                    Span::raw(BLANK_RULE),
                    Span::styled("── That's everything.", style::fg(palette, Role::Muted)),
                ]),
                Line::from(vec![
                    Span::raw(BLANK_RULE),
                    Span::styled(
                        format!("{n} in this view."),
                        style::fg(palette, Role::Muted),
                    ),
                ]),
            ]
        }
        Row::Item(Item::Noise) => {
            let n = queue.noise.len();
            let (glyph, action) = if queue.noise_open {
                ("▾", "collapse")
            } else {
                ("▸", "expand")
            };
            let lead = if selected {
                rule(app, focused)
            } else {
                Span::raw(BLANK_RULE)
            };
            let word = if n == 1 { "bot update" } else { "bot updates" };
            vec![Line::from(vec![
                lead,
                Span::styled(
                    format!("{glyph} Noise · {n} {word} · ⏎ to {action}"),
                    style::fg(palette, Role::Muted),
                ),
            ])]
        }
        Row::Item(Item::Change(i)) => {
            let Some(change) = app.state.changes.get(i) else {
                return Vec::new();
            };
            let now = app.state.now.unwrap_or(rb_core::Timestamp(0));
            let parts = queue::row_parts(change, now);
            let lead = |sel: bool| {
                if sel {
                    rule(app, focused)
                } else {
                    Span::raw(BLANK_RULE)
                }
            };
            let (glyph, glyph_role) = ci_look(parts.ci);
            let title_style = if selected {
                style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD)
            } else {
                style::fg(palette, Role::Text)
            };
            let room = width.saturating_sub(2 + 2 + cells(&parts.age) + 2);
            let first = justify(
                vec![
                    lead(selected),
                    Span::styled(format!("{glyph} "), style::fg(palette, glyph_role)),
                    Span::styled(truncate(&parts.title, room), title_style),
                ],
                vec![Span::styled(
                    format!("{} ", parts.age),
                    style::fg(palette, Role::Muted),
                )],
                width,
            );
            let forge_style = style::tag_fg(
                palette,
                app.state.source(&change.id.source_id),
                change.id.kind,
            );
            let used =
                2 + cells(parts.forge) + 1 + cells(&parts.reference) + 3 + cells(&parts.author) + 3;
            let status = truncate(parts.status, width.saturating_sub(used));
            let second = Line::from(vec![
                lead(selected),
                Span::styled(parts.forge, forge_style),
                Span::raw(" "),
                Span::styled(parts.reference, style::fg(palette, Role::Cyan)),
                Span::styled(" · ", style::fg(palette, Role::Muted)),
                Span::styled(parts.author, style::fg(palette, Role::Interactive)),
                Span::styled(" · ", style::fg(palette, Role::Muted)),
                Span::styled(status, style::fg(palette, Role::TextSecondary)),
            ]);
            vec![first, second]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::strip_window;

    #[test]
    fn everything_fits_when_there_is_room() {
        assert_eq!(strip_window(&[10, 10, 10], 1, 40, 0), 0..3);
        assert_eq!(strip_window(&[], 1, 40, 0), 0..0);
    }

    #[test]
    fn the_active_tab_is_always_inside_the_window() {
        let widths = [12; 8];
        for active in 0..8 {
            for width in [14, 30, 45, 70, 120] {
                let w = strip_window(&widths, 1, width, active);
                assert!(w.contains(&active), "active {active} width {width}: {w:?}");
                let used: usize = widths[w.clone()].iter().sum::<usize>() + w.len() - 1;
                let marks = usize::from(w.start > 0) * 2 + usize::from(w.end < 8) * 2;
                assert!(used + marks <= width.max(16), "{w:?} {width}");
            }
        }
    }

    #[test]
    fn scrolling_keeps_neighbours_in_view_and_marks_hidden_tabs() {
        let widths = [12; 6];
        assert_eq!(strip_window(&widths, 1, 40, 0), 0..3);
        assert_eq!(strip_window(&widths, 1, 40, 5), 3..6);
        let mid = strip_window(&widths, 1, 40, 3);
        assert!(mid.start > 0 && mid.end < 6);
    }
}
