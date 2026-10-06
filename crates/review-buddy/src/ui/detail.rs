//! The detail pane: title, meta, action chips, tabs and the selected tab's content.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use rb_core::{ChangeSummary, CiState, ReviewerState};
use rb_theme::Role;

use super::dashboard::ci_look;
use super::text::{columns, plain_markdown, spans_width, wrap};
use super::{chrome::truncate, layout, style, HitMap};
use crate::app::{queue, Action, App, ChangeInfo, Chip, Selected, Tab};

/// Rows above the tab content: meta, stats, gap, chips, gap, tabs, rule (plus the title).
const FIXED_ROWS: u16 = 7;
const MAX_TITLE_LINES: usize = 2;
const EXCERPT_LINES: usize = 8;
/// Below this width the Reviewers and Checks columns stack instead of sitting side by side.
const SIDE_BY_SIDE: u16 = 54;

fn content_rect(app: &App) -> Rect {
    layout::detail_content(layout::dashboard(layout::body(app.size)).detail)
}

fn title_lines(change: &ChangeSummary, width: usize) -> Vec<String> {
    let mut lines = wrap(&change.title, width);
    if lines.len() > MAX_TITLE_LINES {
        lines.truncate(MAX_TITLE_LINES);
        let last = lines[MAX_TITLE_LINES - 1].clone();
        lines[MAX_TITLE_LINES - 1] = truncate(&format!("{last}…"), width);
    }
    lines
}

fn header_height(title: &[String]) -> u16 {
    title.len().max(1) as u16 + FIXED_ROWS
}

/// How far the tab content can scroll.
pub fn max_scroll(app: &App) -> u16 {
    let Some(change) = app.selected_change() else {
        return 0;
    };
    let area = content_rect(app);
    let header = header_height(&title_lines(change, usize::from(area.width)));
    let view = area.height.saturating_sub(header);
    let body = body_lines(app, change, area.width).len();
    u16::try_from(body).unwrap_or(u16::MAX).saturating_sub(view)
}

/// The detail pane's contents inside `area` (the bordered pane's rectangle).
pub fn draw(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let content = layout::detail_content(area);
    if content.width == 0 || content.height == 0 {
        return;
    }
    let Some(change) = app.selected_change() else {
        let text = match app.dashboard.selected {
            Some(Selected::More(_)) => "The rest of this bucket sits behind ⏎ on the +N more row.",
            Some(Selected::Noise) => "Bot updates are tucked away. ⏎ on the Noise row shows them.",
            _ if app.state.loading => "·  ·  ·",
            _ => "Pick a change to read it here.",
        };
        frame.render_widget(
            Paragraph::new(Line::styled(text, style::fg(palette, Role::Muted))),
            content,
        );
        return;
    };
    let now = app.state.now.unwrap_or(rb_core::Timestamp(0));
    let width = usize::from(content.width);
    let title = title_lines(change, width);
    let mut y = content.y;

    let mut lines: Vec<(Line<'static>, u16)> = Vec::new();
    for t in &title {
        lines.push((
            Line::styled(
                t.clone(),
                style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
            ),
            1,
        ));
    }
    lines.push((
        Line::from(vec![
            Span::styled(change.author.clone(), style::fg(palette, Role::Interactive)),
            Span::styled(" wants ", style::fg(palette, Role::Muted)),
            Span::styled(change.branch.clone(), style::fg(palette, Role::Cyan)),
            Span::styled(" → ", style::fg(palette, Role::Muted)),
            Span::styled(change.base.clone(), style::fg(palette, Role::Cyan)),
        ]),
        1,
    ));
    lines.push((
        Line::from(vec![
            Span::styled(
                format!("+{}", change.adds),
                style::fg(palette, Role::Success),
            ),
            Span::raw(" "),
            Span::styled(
                format!("−{}", change.dels),
                style::fg(palette, Role::Danger),
            ),
            Span::styled(
                format!(
                    " · {} {} · {}",
                    change.files,
                    if change.files == 1 { "file" } else { "files" },
                    queue::opened_phrase(now, change.created_at)
                ),
                style::fg(palette, Role::Muted),
            ),
        ]),
        1,
    ));
    for (line, _) in lines {
        frame.render_widget(
            Paragraph::new(line),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 1;
    }
    y += 1;

    draw_strip(frame, content.x, y, content.width, chips(app), "  ", hits);
    y += 2;
    draw_strip(
        frame,
        content.x,
        y,
        content.width,
        tabs(app, change),
        " ",
        hits,
    );
    y += 1;
    let rule = "─".repeat(width);
    frame.render_widget(
        Paragraph::new(Line::styled(rule, style::fg(palette, Role::Line))),
        Rect::new(content.x, y, content.width, 1),
    );
    y += 1;

    let body = body_lines(app, change, content.width);
    let view = content.bottom().saturating_sub(y);
    let skip = usize::from(app.dashboard.detail_scroll.min(max_scroll(app)));
    for (i, line) in body
        .into_iter()
        .skip(skip)
        .take(usize::from(view))
        .enumerate()
    {
        frame.render_widget(
            Paragraph::new(line),
            Rect::new(content.x, y + i as u16, content.width, 1),
        );
    }
}

/// Draws clickable items left to right on one row and registers their hit areas.
pub fn draw_strip(
    frame: &mut Frame,
    x: u16,
    y: u16,
    width: u16,
    items: Vec<(Vec<Span<'static>>, Option<Action>)>,
    gap: &str,
    hits: &mut HitMap,
) {
    let mut spans = Vec::new();
    let mut at = 0usize;
    for (item, action) in items {
        let w = spans_width(&item);
        let lead = if spans.is_empty() {
            0
        } else {
            gap.chars().count()
        };
        if at + lead + w > usize::from(width) {
            break;
        }
        if lead > 0 {
            spans.push(Span::raw(gap.to_string()));
        }
        let start = at + lead;
        if let Some(action) = action {
            hits.push(Rect::new(x + start as u16, y, w as u16, 1), action);
        }
        spans.extend(item);
        at = start + w;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), Rect::new(x, y, width, 1));
}

fn chips(app: &App) -> Vec<(Vec<Span<'static>>, Option<Action>)> {
    let palette = &app.palette;
    Chip::ALL
        .iter()
        .map(|chip| {
            let spans = vec![
                Span::styled(
                    chip.key(),
                    style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {}", chip.label()), style::fg(palette, Role::Text)),
            ];
            (spans, Some(Action::Chip(*chip)))
        })
        .collect()
}

fn tabs(app: &App, change: &ChangeSummary) -> Vec<(Vec<Span<'static>>, Option<Action>)> {
    let palette = &app.palette;
    let info = app.state.details.get(&change.id);
    Tab::ALL
        .iter()
        .map(|tab| {
            let count = match tab {
                Tab::Overview => None,
                Tab::Files => Some(change.files as usize),
                Tab::Checks => info.map(|i| i.checks.len()),
                Tab::Conversation => info.map(conversation_count),
            };
            let active = *tab == app.dashboard.tab;
            let base = if active {
                style::fg(palette, Role::TextBright)
                    .patch(style::bg(palette, Role::Selection))
                    .add_modifier(Modifier::BOLD)
            } else {
                style::fg(palette, Role::TextSecondary)
            };
            let mut spans = vec![Span::styled(format!(" {}", tab.label()), base)];
            if let Some(n) = count {
                let count_style = if active {
                    base
                } else {
                    style::fg(palette, Role::Muted)
                };
                spans.push(Span::styled(format!(" {n}"), count_style));
            }
            spans.push(Span::styled(" ", base));
            (spans, Some(Action::SelectTab(*tab)))
        })
        .collect()
}

fn conversation_count(info: &ChangeInfo) -> usize {
    info.threads.iter().map(|t| t.comments.len()).sum()
}

fn muted(app: &App, text: impl Into<String>) -> Line<'static> {
    Line::styled(text.into(), style::fg(&app.palette, Role::Muted))
}

fn heading(app: &App, text: &str) -> Line<'static> {
    Line::styled(
        text.to_string(),
        style::fg(&app.palette, Role::TextSecondary).add_modifier(Modifier::BOLD),
    )
}

fn placeholder(app: &App, change: &ChangeSummary) -> Vec<Line<'static>> {
    match app.state.failures.get(&change.id.source_id) {
        Some(failure) => vec![
            muted(app, failure.summary.clone()),
            muted(app, failure.next_step.clone()),
        ],
        None => vec![muted(app, "·  ·  ·")],
    }
}

pub fn body_lines(app: &App, change: &ChangeSummary, width: u16) -> Vec<Line<'static>> {
    let info = app.state.details.get(&change.id);
    match app.dashboard.tab {
        Tab::Overview => overview(app, change, info, width),
        Tab::Files => files(app, change),
        Tab::Checks => match info {
            Some(info) => checks(app, info),
            None => placeholder(app, change),
        },
        Tab::Conversation => match info {
            Some(info) => conversation(app, info),
            None => placeholder(app, change),
        },
    }
}

fn overview(
    app: &App,
    change: &ChangeSummary,
    info: Option<&ChangeInfo>,
    width: u16,
) -> Vec<Line<'static>> {
    let palette = &app.palette;
    let Some(info) = info else {
        return placeholder(app, change);
    };
    let mut out = Vec::new();
    let text = plain_markdown(&info.body);
    let wrapped = wrap(&text, usize::from(width));
    let cut = wrapped.len() > EXCERPT_LINES;
    for line in wrapped.iter().take(EXCERPT_LINES) {
        out.push(Line::styled(line.clone(), style::fg(palette, Role::Text)));
    }
    if cut {
        out.push(muted(app, "…"));
    }
    if wrapped.is_empty() {
        out.push(muted(app, "No description."));
    }
    out.push(Line::raw(""));

    let reviewers = reviewer_lines(app, change);
    let check_lines = check_summary_lines(app, info);
    if width >= SIDE_BY_SIDE {
        out.extend(columns(reviewers, check_lines, usize::from(width) / 2));
    } else {
        out.extend(reviewers);
        out.push(Line::raw(""));
        out.extend(check_lines);
    }
    out.push(Line::raw(""));

    out.push(heading(app, "Latest comment"));
    out.extend(latest_comment(app, info, width));
    out.push(Line::raw(""));
    out.push(Line::from(vec![
        Span::styled("Your review  ", style::fg(palette, Role::TextSecondary)),
        Span::styled(my_review_text(change), style::fg(palette, Role::Text)),
    ]));
    out
}

fn my_review_text(change: &ChangeSummary) -> &'static str {
    use rb_core::MyReview;
    match change.my_review {
        MyReview::None => "not started",
        MyReview::Approved => "✓ approved",
        MyReview::ChangesRequested => "✎ asked for changes",
        MyReview::Commented => "✎ commented",
    }
}

fn reviewer_lines(app: &App, change: &ChangeSummary) -> Vec<Line<'static>> {
    let palette = &app.palette;
    let mut out = vec![heading(app, "Reviewers")];
    if change.reviewers.is_empty() {
        out.push(muted(app, "Nobody yet"));
    }
    for r in &change.reviewers {
        let (glyph, role, word) = match r.state {
            ReviewerState::Approved => ("✓", Role::Success, "approved"),
            ReviewerState::Requested => ("◌", Role::Muted, "requested"),
            ReviewerState::Commented => ("✎", Role::Interactive, "commented"),
            ReviewerState::ChangesRequested => ("✎", Role::Warning, "asked for changes"),
        };
        out.push(Line::from(vec![
            Span::styled(format!("{glyph} "), style::fg(palette, role)),
            Span::styled(r.login.clone(), style::fg(palette, Role::Interactive)),
            Span::styled(format!(" {word}"), style::fg(palette, Role::Muted)),
        ]));
    }
    out
}

pub fn ci_word(state: CiState) -> &'static str {
    match state {
        CiState::Pass => "passing",
        CiState::Running => "running",
        CiState::Fail => "failing",
        CiState::None => "no checks",
        CiState::Neutral => "neutral",
        CiState::Skipped => "skipped",
        CiState::Cancelled => "cancelled",
    }
}

fn duration_word(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {:02}s", s / 60, s % 60),
        s => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
    }
}

fn check_note(check: &rb_core::Check) -> String {
    let mut parts = vec![ci_word(check.state).to_string()];
    if let Some(secs) = check.duration_secs() {
        parts.push(duration_word(secs));
    }
    match check.required {
        Some(true) => parts.push("required".into()),
        Some(false) => parts.push("optional".into()),
        None => {}
    }
    format!("  {}", parts.join(" · "))
}

fn check_summary_lines(app: &App, info: &ChangeInfo) -> Vec<Line<'static>> {
    let palette = &app.palette;
    let mut out = vec![heading(app, "Checks")];
    if info.checks.is_empty() {
        out.push(muted(app, "No checks reported"));
        return out;
    }
    let groups: [&[CiState]; 2] = [
        &[CiState::Pass, CiState::Running, CiState::Fail],
        &[CiState::Neutral, CiState::Skipped, CiState::Cancelled],
    ];
    for group in groups {
        let mut counts = Vec::new();
        for &state in group {
            let n = info.checks.iter().filter(|c| c.state == state).count();
            if n > 0 {
                let (glyph, role) = ci_look(state);
                counts.push(Span::styled(
                    format!("{glyph} {n} {}  ", ci_word(state)),
                    style::fg(palette, role),
                ));
            }
        }
        if !counts.is_empty() {
            out.push(Line::from(counts));
        }
    }
    out
}

fn latest_comment(app: &App, info: &ChangeInfo, width: u16) -> Vec<Line<'static>> {
    let palette = &app.palette;
    let now = app.state.now.unwrap_or(rb_core::Timestamp(0));
    let latest = info
        .threads
        .iter()
        .flat_map(|t| t.comments.iter().map(move |c| (t, c)))
        .max_by_key(|(_, c)| c.created_at);
    let Some((thread, comment)) = latest else {
        return vec![muted(app, "No comments yet")];
    };
    let place = match (&thread.path, thread.line) {
        (Some(path), Some(line)) => {
            format!(" · {}:{line}", path.rsplit('/').next().unwrap_or(path))
        }
        _ => String::new(),
    };
    let mut out = vec![Line::from(vec![
        Span::styled(
            comment.author.clone(),
            style::fg(palette, Role::Interactive),
        ),
        Span::styled(
            format!(" · {} ago{place}", queue::age(now, comment.created_at)),
            style::fg(palette, Role::Muted),
        ),
    ])];
    for line in wrap(&plain_markdown(&comment.body), usize::from(width))
        .into_iter()
        .take(3)
    {
        out.push(Line::styled(line, style::fg(palette, Role::Text)));
    }
    out
}

fn files(app: &App, change: &ChangeSummary) -> Vec<Line<'static>> {
    let palette = &app.palette;
    vec![
        Line::from(vec![
            Span::styled(
                format!("{} changed", plural(change.files as usize, "file")),
                style::fg(palette, Role::Text),
            ),
            Span::styled(
                format!("  +{}", change.adds),
                style::fg(palette, Role::Success),
            ),
            Span::styled(
                format!(" −{}", change.dels),
                style::fg(palette, Role::Danger),
            ),
        ]),
        Line::raw(""),
        hint(app, "⏎", "to open the diff"),
    ]
}

fn checks(app: &App, info: &ChangeInfo) -> Vec<Line<'static>> {
    let palette = &app.palette;
    if info.checks.is_empty() {
        return vec![muted(app, "No checks reported for this change.")];
    }
    info.checks
        .iter()
        .map(|c| {
            let (glyph, role) = ci_look(c.state);
            Line::from(vec![
                Span::styled(format!("{glyph} "), style::fg(palette, role)),
                Span::styled(c.name.clone(), style::fg(palette, Role::Text)),
                Span::styled(check_note(c), style::fg(palette, Role::Muted)),
            ])
        })
        .collect()
}

fn conversation(app: &App, info: &ChangeInfo) -> Vec<Line<'static>> {
    let palette = &app.palette;
    let comments = conversation_count(info);
    if comments == 0 {
        return vec![muted(app, "No comments yet.")];
    }
    let mut out = vec![
        Line::styled(
            format!(
                "{} in {}",
                plural(comments, "comment"),
                plural(info.threads.len(), "thread")
            ),
            style::fg(palette, Role::Text),
        ),
        Line::raw(""),
    ];
    for thread in &info.threads {
        let place = match (&thread.path, thread.line) {
            (Some(path), Some(line)) => {
                format!("{}:{line}", path.rsplit('/').next().unwrap_or(path))
            }
            _ => "general".to_string(),
        };
        let authors: Vec<&str> = thread.comments.iter().map(|c| c.author.as_str()).collect();
        out.push(Line::from(vec![
            Span::styled(format!("{place}  "), style::fg(palette, Role::Cyan)),
            Span::styled(authors.join(", "), style::fg(palette, Role::Interactive)),
        ]));
    }
    out.push(Line::raw(""));
    out.push(hint(app, "⏎", "to jump to the line in the diff"));
    out
}

fn hint(app: &App, key: &str, label: &str) -> Line<'static> {
    let palette = &app.palette;
    Line::from(vec![
        Span::styled(
            key.to_string(),
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {label}"), style::fg(palette, Role::Muted)),
    ])
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_forms() {
        assert_eq!(plural(1, "file"), "1 file");
        assert_eq!(plural(0, "file"), "0 files");
        assert_eq!(plural(3, "thread"), "3 threads");
    }

    #[test]
    fn ci_words() {
        assert_eq!(ci_word(CiState::Fail), "failing");
        assert_eq!(ci_word(CiState::None), "no checks");
        assert_eq!(ci_word(CiState::Skipped), "skipped");
        assert_eq!(ci_word(CiState::Cancelled), "cancelled");
        assert_eq!(ci_word(CiState::Neutral), "neutral");
    }

    #[test]
    fn durations_are_compact() {
        assert_eq!(duration_word(18), "18s");
        assert_eq!(duration_word(94), "1m 34s");
        assert_eq!(duration_word(3900), "1h 05m");
    }
}
