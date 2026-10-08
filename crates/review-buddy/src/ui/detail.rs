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
use crate::app::{links, queue, Action, App, ChangeInfo, Chip, Selected, Tab};
use crate::images::extract::{self, ImageRef, Segment};
use crate::images::{self, Slot, View};

/// Rows above the tab content: meta, stats, gap, chips, gap, tabs, rule (plus the title).
const FIXED_ROWS: u16 = 7;
const MAX_TITLE_LINES: usize = 2;
const EXCERPT_LINES: usize = 8;
/// Below this width the Reviewers and Checks columns stack instead of sitting side by side.
const SIDE_BY_SIDE: u16 = 54;

fn content_rect(app: &App) -> Rect {
    layout::detail_content(layout::dashboard(layout::body(app.size), app.layout).detail)
}

/// A short pane (Detail stacked under or over the Queue) drops the title to one line and
/// the two blank rows, so the chips and tabs stay on screen with room for the body.
const COMPACT_BELOW: u16 = 14;

fn compact(height: u16) -> bool {
    height < COMPACT_BELOW
}

fn title_lines(change: &ChangeSummary, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines = wrap(&change.title, width);
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        let last = lines[max_lines - 1].clone();
        lines[max_lines - 1] = truncate(&format!("{last}…"), width);
    }
    lines
}

fn max_title_lines(height: u16) -> usize {
    if compact(height) {
        1
    } else {
        MAX_TITLE_LINES
    }
}

fn header_height(title: &[String], height: u16) -> u16 {
    let gaps = if compact(height) { 2 } else { 0 };
    title.len().max(1) as u16 + FIXED_ROWS - gaps
}

/// The rows the tab content has to scroll in, below the header.
fn view_rows(app: &App, change: &ChangeSummary) -> (u16, u16) {
    let area = content_rect(app);
    let title = title_lines(
        change,
        usize::from(area.width),
        max_title_lines(area.height),
    );
    let header = header_height(&title, area.height);
    (area.width, area.height.saturating_sub(header))
}

/// How far the tab content can scroll.
pub fn max_scroll(app: &App) -> u16 {
    let Some(change) = app.selected_change() else {
        return 0;
    };
    let (width, view) = view_rows(app, change);
    let body = body_lines(app, change, width).len();
    u16::try_from(body).unwrap_or(u16::MAX).saturating_sub(view)
}

/// The scroll offset that brings the `index`th image of the description (and its caption) into
/// view, or `None` when it is already there or isn't laid out.
pub fn scroll_to_image(app: &App, index: usize) -> Option<u16> {
    let change = app.selected_change()?;
    let (width, view) = view_rows(app, change);
    let body = body(app, change, width);
    let (line, rows) = body.anchors.get(index).copied()?;
    let now = usize::from(app.dashboard.detail_scroll.min(max_scroll(app)));
    let view = usize::from(view);
    let want = if line < now {
        line
    } else if line + rows > now + view {
        (line + rows).saturating_sub(view).min(line)
    } else {
        return None;
    };
    u16::try_from(want).ok()
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
    let short = compact(content.height);
    let title = title_lines(change, width, max_title_lines(content.height));
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
        if y < content.bottom() {
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(content.x, y, content.width, 1),
            );
        }
        y += 1;
    }
    if !short {
        y += 1;
    }

    if y < content.bottom() {
        draw_strip(
            frame,
            content.x,
            y,
            content.width,
            chips(app, change),
            "  ",
            hits,
        );
    }
    y += if short { 1 } else { 2 };
    if y < content.bottom() {
        draw_strip(
            frame,
            content.x,
            y,
            content.width,
            tabs(app, change),
            " ",
            hits,
        );
    }
    y += 1;
    let rule = "─".repeat(width);
    if y < content.bottom() {
        frame.render_widget(
            Paragraph::new(Line::styled(rule, style::fg(palette, Role::Line))),
            Rect::new(content.x, y, content.width, 1),
        );
    }
    y += 1;

    let body = body(app, change, content.width);
    let view = content.bottom().saturating_sub(y);
    let skip = usize::from(app.dashboard.detail_scroll.min(max_scroll(app)));
    for (i, line) in body
        .lines
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
    let pictures = Rect::new(content.x, y, content.width, view);
    for slot in &body.slots {
        let top = slot.line as i64 - skip as i64;
        if top >= i64::from(view) || top + i64::from(slot.rows) <= 0 {
            continue;
        }
        let top = i16::try_from(top).unwrap_or(i16::MIN);
        app.images.render(frame, pictures, slot, top);
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

fn chips(app: &App, change: &ChangeSummary) -> Vec<(Vec<Span<'static>>, Option<Action>)> {
    let palette = &app.palette;
    Chip::ALL
        .iter()
        .filter(|chip| {
            chip.feature()
                .is_none_or(|action| app.state.supports(&change.id.source_id, action))
        })
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

/// The tab's lines, plus where pictures go in them.
pub struct Body {
    pub lines: Vec<Line<'static>>,
    /// Blocks of blank lines that hold a drawn picture.
    pub slots: Vec<Slot>,
    /// For each image in the description, its first line and the rows it takes with its caption.
    pub anchors: Vec<(usize, usize)>,
}

impl From<Vec<Line<'static>>> for Body {
    fn from(lines: Vec<Line<'static>>) -> Self {
        Self {
            lines,
            slots: Vec::new(),
            anchors: Vec::new(),
        }
    }
}

pub fn body_lines(app: &App, change: &ChangeSummary, width: u16) -> Vec<Line<'static>> {
    body(app, change, width).lines
}

pub fn body(app: &App, change: &ChangeSummary, width: u16) -> Body {
    let info = app.state.details.get(&change.id);
    match app.dashboard.tab {
        Tab::Overview => overview(app, change, info, width),
        Tab::Files => files(app, change).into(),
        Tab::Checks => match info {
            Some(info) => checks(app, info).into(),
            None => placeholder(app, change).into(),
        },
        Tab::Conversation => match info {
            Some(info) => conversation(app, info).into(),
            None => placeholder(app, change).into(),
        },
    }
}

fn overview(app: &App, change: &ChangeSummary, info: Option<&ChangeInfo>, width: u16) -> Body {
    let palette = &app.palette;
    let Some(info) = info else {
        return placeholder(app, change).into();
    };
    let mut out = Vec::new();
    let mut slots = Vec::new();
    let mut anchors = Vec::new();
    description(app, change, info, width, &mut out, &mut slots, &mut anchors);
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
    out.extend(latest_comment(app, change, info, width));
    out.push(Line::raw(""));
    out.push(Line::from(vec![
        Span::styled("Your review  ", style::fg(palette, Role::TextSecondary)),
        Span::styled(my_review_text(change), style::fg(palette, Role::Text)),
    ]));
    Body {
        lines: out,
        slots,
        anchors,
    }
}

/// The description: its first few lines of text, with each image where it sits in the text.
fn description(
    app: &App,
    change: &ChangeSummary,
    info: &ChangeInfo,
    width: u16,
    out: &mut Vec<Line<'static>>,
    slots: &mut Vec<Slot>,
    anchors: &mut Vec<(usize, usize)>,
) {
    let palette = &app.palette;
    let base = links::change_url(app, &change.id).and_then(|u| url::Url::parse(&u).ok());
    let segments = extract::segments(&info.body, base.as_ref());
    if segments.is_empty() {
        out.push(muted(app, "No description."));
        return;
    }
    let mut budget = EXCERPT_LINES;
    let mut cut = false;
    let mut index = 0;
    for segment in segments {
        match segment {
            Segment::Text(text) => {
                for line in wrap(&plain_markdown(&text), usize::from(width)) {
                    if budget == 0 {
                        if !cut {
                            out.push(muted(app, "…"));
                            cut = true;
                        }
                        break;
                    }
                    out.push(Line::styled(line, style::fg(palette, Role::Text)));
                    budget -= 1;
                }
            }
            Segment::Image(image) => {
                if index < images::MAX_IMAGES {
                    let start = out.len();
                    picture(app, change, &image, index, width, out, slots);
                    anchors.push((start, out.len() - start));
                }
                index += 1;
            }
        }
    }
    if index > images::MAX_IMAGES {
        let more = index - images::MAX_IMAGES;
        let noun = if more == 1 { "image" } else { "images" };
        out.push(muted(
            app,
            format!("+{more} more {noun}. o opens the change to see them."),
        ));
    }
}

/// One image in the description: a block for the picture and its caption when it can be drawn,
/// otherwise a note that says why not.
fn picture(
    app: &App,
    change: &ChangeSummary,
    image: &ImageRef,
    index: usize,
    width: u16,
    out: &mut Vec<Line<'static>>,
    slots: &mut Vec<Slot>,
) {
    let focused = matches!(&app.images.focus, Some((id, at)) if *id == change.id && *at == index);
    let label = image.label();
    let note = |detail: Option<String>| {
        let mut text = format!("▣ image: {label}");
        if let Some(detail) = detail {
            text.push_str(" – ");
            text.push_str(&detail);
        }
        text
    };
    let text = match app.images.view(image) {
        View::Ready(img) => {
            let (cols, rows) = images::layout::fit(
                (img.width(), img.height()),
                (image.width, image.height),
                app.images.font(),
                width,
            );
            if let Some(url) = &image.url {
                slots.push(Slot {
                    line: out.len(),
                    cols,
                    rows,
                    url: url.to_string(),
                });
                out.extend((0..rows).map(|_| Line::raw("")));
            }
            note(None)
        }
        View::Loading if image.url.is_some() => note(Some("loading…".into())),
        View::Failed(failure) => note(Some(failure.reason())),
        View::Off | View::Loading => {
            let why = if image.url.is_none() {
                "not a web address"
            } else {
                "can't be drawn in this terminal"
            };
            note(Some(why.into()))
        }
    };
    let (marker, text, role) = if focused {
        ("▸ ", format!("{text} (open with o)"), Role::Accent)
    } else {
        ("", text, Role::Muted)
    };
    let mut style = style::fg(&app.palette, role);
    if focused {
        style = style.add_modifier(Modifier::BOLD);
    }
    for line in wrap(&format!("{marker}{text}"), usize::from(width)) {
        out.push(Line::styled(line, style));
    }
}

fn my_review_text(change: &ChangeSummary) -> &'static str {
    use rb_core::MyReview;
    match change.my_review {
        MyReview::None => "not started",
        MyReview::Approved => "✓ approved",
        MyReview::ChangesRequested => "✎ changes requested",
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

fn latest_comment(
    app: &App,
    change: &ChangeSummary,
    info: &ChangeInfo,
    width: u16,
) -> Vec<Line<'static>> {
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
    let base = links::change_url(app, &change.id).and_then(|u| url::Url::parse(&u).ok());
    let text = extract::flatten(&comment.body, base.as_ref());
    for line in wrap(&plain_markdown(&text), usize::from(width))
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
