//! The detail pane: title, meta, action chips, tabs and the selected tab's content.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use rb_core::{ChangeSummary, CiState, ReviewerState, Thread};
use rb_theme::Role;

use super::dashboard::ci_look;
use super::text::{columns, plain_markdown, spans_width, wrap, wrap_marked, Join};
use super::textmap::{RegionKey, TextRegion, TextRow};
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
    layout::detail_content(
        layout::dashboard(crate::app::terminal::app_body(app), app.layout).detail,
    )
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
    let most = u16::try_from(body.lines.len())
        .unwrap_or(u16::MAX)
        .saturating_sub(view);
    let skip = usize::from(app.dashboard.detail_scroll.min(most));
    let visible = |line: usize| line >= skip && line < skip + usize::from(view);
    for (i, line) in body
        .lines
        .iter()
        .skip(skip)
        .take(usize::from(view))
        .enumerate()
    {
        frame.render_widget(
            Paragraph::new(line.clone()),
            Rect::new(content.x, y + i as u16, content.width, 1),
        );
    }
    for (index, &(start, _)) in body.threads.iter().enumerate() {
        if visible(start) {
            let row = y + (start - skip) as u16;
            hits.push(
                Rect::new(content.x, row, content.width, 1),
                Action::ToggleThread(index),
            );
        }
    }
    for (key, rows) in &body.texts {
        if !rows.iter().any(|r| visible(r.line)) {
            continue;
        }
        let mut region = TextRegion::new(*key, Rect::new(content.x, y, content.width, view));
        for (ord, row) in rows.iter().enumerate() {
            let at_y = if visible(row.line) {
                y + (row.line - skip) as u16
            } else {
                TextRow::OFF_SCREEN
            };
            let at = (content.x + row.indent, at_y);
            let span = content.width.saturating_sub(row.indent);
            region
                .rows
                .push(TextRow::new(ord, at, span, row.text.clone()).joined(row.join));
        }
        hits.texts.push(region);
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

/// One selectable row of the body: its line, its text, how it joins the row before, and how
/// many cells in from the pane's left edge the text starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyText {
    pub line: usize,
    pub text: String,
    pub join: Join,
    pub indent: u16,
}

impl BodyText {
    fn new(line: usize, text: String, join: Join) -> Self {
        Self {
            line,
            text,
            join,
            indent: 0,
        }
    }
}

/// The tab's lines, plus where pictures go in them.
pub struct Body {
    pub lines: Vec<Line<'static>>,
    /// Blocks of blank lines that hold a drawn picture.
    pub slots: Vec<Slot>,
    /// For each image in the description, its first line and the rows it takes with its caption.
    pub anchors: Vec<(usize, usize)>,
    /// The selectable text: for each region, its rows as `(line, text, join)`.
    pub texts: Vec<(RegionKey, Vec<BodyText>)>,
    /// On the Conversation tab, each thread's first line (its header) and the rows it takes.
    pub threads: Vec<(usize, usize)>,
}

impl From<Vec<Line<'static>>> for Body {
    fn from(lines: Vec<Line<'static>>) -> Self {
        Self {
            lines,
            slots: Vec::new(),
            anchors: Vec::new(),
            texts: Vec::new(),
            threads: Vec::new(),
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
            Some(info) => conversation(app, change, info, width),
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
    let mut described: Vec<BodyText> = Vec::new();
    description(
        app,
        change,
        info,
        width,
        (&mut out, &mut slots, &mut anchors),
        &mut described,
    );
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
    let (comment, said) = latest_comment(app, change, info, width);
    let said: Vec<_> = said
        .into_iter()
        .map(|mut row| {
            row.line += out.len();
            row
        })
        .collect();
    out.extend(comment);
    out.push(Line::raw(""));
    out.push(Line::from(vec![
        Span::styled("Your review  ", style::fg(palette, Role::TextSecondary)),
        Span::styled(my_review_text(change), style::fg(palette, Role::Text)),
    ]));
    let mut texts = vec![(RegionKey::Description, described)];
    texts.push((RegionKey::LatestComment, said));
    Body {
        lines: out,
        slots,
        anchors,
        texts,
        threads: Vec::new(),
    }
}

/// The description: its first few lines of text, with each image where it sits in the text.
fn description(
    app: &App,
    change: &ChangeSummary,
    info: &ChangeInfo,
    width: u16,
    (out, slots, anchors): (
        &mut Vec<Line<'static>>,
        &mut Vec<Slot>,
        &mut Vec<(usize, usize)>,
    ),
    described: &mut Vec<BodyText>,
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
                for (line, join) in wrap_marked(&plain_markdown(&text), usize::from(width)) {
                    if budget == 0 {
                        if !cut {
                            out.push(muted(app, "…"));
                            cut = true;
                        }
                        break;
                    }
                    described.push(BodyText::new(out.len(), line.clone(), join));
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
) -> (Vec<Line<'static>>, Vec<BodyText>) {
    let palette = &app.palette;
    let now = app.state.now.unwrap_or(rb_core::Timestamp(0));
    let latest = info
        .threads
        .iter()
        .flat_map(|t| t.comments.iter().map(move |c| (t, c)))
        .max_by_key(|(_, c)| c.created_at);
    let Some((thread, comment)) = latest else {
        return (vec![muted(app, "No comments yet")], Vec::new());
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
    let mut said = Vec::new();
    for (line, join) in wrap_marked(&plain_markdown(&text), usize::from(width))
        .into_iter()
        .take(3)
    {
        said.push(BodyText::new(out.len(), line.clone(), join));
        out.push(Line::styled(line, style::fg(palette, Role::Text)));
    }
    (out, said)
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

/// Where a thread sits: `menus.rs:43`, `menubar.rs:5–7`, or `general`.
fn place_of(thread: &Thread) -> String {
    match (&thread.path, thread.line) {
        (Some(path), Some(line)) => {
            let name = path.rsplit('/').next().unwrap_or(path);
            match thread.start_line {
                Some(start) if start != line => format!("{name}:{start}–{line}"),
                _ => format!("{name}:{line}"),
            }
        }
        _ => "general".to_string(),
    }
}

/// Whether the thread shows only its first line: resolved threads start folded, and `z` flips
/// either kind.
pub fn thread_folded(app: &App, thread: &Thread) -> bool {
    thread.resolved != app.dashboard.flipped.contains(&thread.id)
}

fn thread_labels(thread: &Thread) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if thread.resolved {
        labels.push("resolved");
    }
    if thread.outdated {
        labels.push("outdated");
    }
    if thread.pending {
        labels.push("pending");
    }
    labels
}

fn conversation(app: &App, change: &ChangeSummary, info: &ChangeInfo, width: u16) -> Body {
    let palette = &app.palette;
    let comments = conversation_count(info);
    if comments == 0 {
        return vec![muted(app, "No comments yet.")].into();
    }
    let now = app.state.now.unwrap_or(rb_core::Timestamp(0));
    let base = links::change_url(app, &change.id).and_then(|u| url::Url::parse(&u).ok());
    let width = usize::from(width);
    let cursor = app
        .dashboard
        .thread
        .min(info.threads.len().saturating_sub(1));
    let focused = app.dashboard.focus == crate::app::Pane::Detail;

    let mut out = vec![Line::styled(
        format!(
            "{} in {}",
            plural(comments, "comment"),
            plural(info.threads.len(), "thread")
        ),
        style::fg(palette, Role::Text),
    )];
    let keys = if focused {
        "n and N move between threads · z folds one · Z folds all · c replies · ⏎ opens the line in the diff"
    } else {
        "Focus this pane (l or tab) to move between threads, fold them, reply or jump to the diff."
    };
    out.extend(wrap(keys, width).into_iter().map(|l| muted(app, l)));
    out.push(Line::raw(""));

    let mut texts: Vec<(RegionKey, Vec<BodyText>)> = Vec::new();
    let mut spans = Vec::new();
    let mut flat = 0u32;
    for (index, thread) in info.threads.iter().enumerate() {
        let start = out.len();
        let folded = thread_folded(app, thread);
        let here = index == cursor;
        let mut labels: Vec<String> = thread_labels(thread)
            .iter()
            .map(|l| l.to_string())
            .collect();
        labels.push(plural(thread.comments.len(), "comment"));
        if folded {
            labels.push(if here {
                "folded, z expands".to_string()
            } else {
                "folded".to_string()
            });
        }
        let mut place_style = style::fg(palette, Role::Cyan);
        let mut rest_style = style::fg(palette, Role::Muted);
        let mut mark_style = style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD);
        if here {
            let on = style::bg(palette, Role::Selection);
            place_style = place_style.patch(on).add_modifier(Modifier::BOLD);
            rest_style = rest_style.patch(on);
            mark_style = mark_style.patch(on);
        }
        let head = format!(" · {}", labels.join(" · "));
        let room = width.saturating_sub(4);
        let place = truncate(&place_of(thread), room);
        let used = 4 + unicode_width::UnicodeWidthStr::width(place.as_str());
        out.push(Line::from(vec![
            Span::styled(if here { "› " } else { "  " }, mark_style),
            Span::styled(if folded { "▸ " } else { "▾ " }, mark_style),
            Span::styled(place, place_style),
            Span::styled(truncate(&head, width.saturating_sub(used)), rest_style),
        ]));

        if folded {
            if let Some(first) = thread.comments.first() {
                let line = format!(
                    "{}: {}",
                    first.author,
                    plain_markdown(&crate::app::comments::first_words(&first.body, 120))
                );
                out.push(muted(
                    app,
                    format!(
                        "{INDENT_PAD}{}",
                        truncate(&line, width.saturating_sub(INDENT))
                    ),
                ));
            }
        } else {
            for (n, comment) in thread.comments.iter().enumerate() {
                if n > 0 {
                    out.push(Line::raw(""));
                }
                let age = queue::age(now, comment.created_at);
                let when = if age == "now" {
                    "just now".to_string()
                } else {
                    format!("{age} ago")
                };
                let mut meta = vec![
                    Span::raw("  "),
                    Span::styled(
                        comment.author.clone(),
                        style::fg(palette, Role::Interactive),
                    ),
                    Span::styled(format!(" · {when}"), style::fg(palette, Role::Muted)),
                ];
                if comment.pending {
                    meta.push(Span::styled(" · pending", style::fg(palette, Role::Muted)));
                }
                out.push(Line::from(meta));
                let text = extract::flatten(&comment.body, base.as_ref());
                let mut said = Vec::new();
                let room = width.saturating_sub(INDENT);
                for (line, join) in wrap_marked(&plain_markdown(&text), room) {
                    let mut row = BodyText::new(out.len(), line.clone(), join);
                    row.indent = INDENT as u16;
                    said.push(row);
                    out.push(Line::styled(
                        format!("{INDENT_PAD}{line}"),
                        style::fg(palette, Role::Text),
                    ));
                }
                if !said.is_empty() {
                    texts.push((RegionKey::Comment(flat), said));
                }
                flat += 1;
            }
        }
        if folded {
            flat += thread.comments.len() as u32;
        }
        spans.push((start, out.len() - start));
        out.push(Line::raw(""));
    }
    out.pop();
    Body {
        lines: out,
        slots: Vec::new(),
        anchors: Vec::new(),
        texts,
        threads: spans,
    }
}

const INDENT: usize = 4;
const INDENT_PAD: &str = "    ";

/// The scroll offset that brings the `index`th thread of the Conversation into view, or `None`
/// when it already is.
pub fn scroll_to_thread(app: &App, index: usize) -> Option<u16> {
    let change = app.selected_change()?;
    let (width, view) = view_rows(app, change);
    let body = body(app, change, width);
    let (line, rows) = body.threads.get(index).copied()?;
    let now = usize::from(app.dashboard.detail_scroll.min(max_scroll(app)));
    let view = usize::from(view);
    let want = if line < now {
        if index == 0 {
            0
        } else {
            line
        }
    } else if line + rows > now + view {
        (line + rows).saturating_sub(view).min(line)
    } else {
        return None;
    };
    u16::try_from(want).ok()
}

/// The scroll offset that brings row `ord` of the text region `key` into view, or `None` when
/// it already is (or isn't laid out).
pub fn scroll_to_text(app: &App, key: RegionKey, ord: usize) -> Option<u16> {
    let change = app.selected_change()?;
    let (width, view) = view_rows(app, change);
    let body = body(app, change, width);
    let line = body.texts.iter().find(|(k, _)| *k == key)?.1.get(ord)?.line;
    let now = usize::from(app.dashboard.detail_scroll.min(max_scroll(app)));
    let view = usize::from(view);
    let want = if line < now {
        line
    } else if line >= now + view {
        line + 1 - view
    } else {
        return None;
    };
    u16::try_from(want).ok()
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
