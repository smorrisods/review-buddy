//! The submit-review modal: verdict choice, summary field, preview and buttons.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
    Frame,
};
use rb_core::Verdict;
use rb_theme::Role;
use unicode_width::UnicodeWidthStr;

use super::composer::clip;
use super::{chrome::truncate, style, HitMap};
use crate::app::composer::comment_line;
use crate::app::review::{key_hint, submit_label, verdict_label, ReviewFocus, ReviewModal};
use crate::app::{Action, App, DiffState};

const SUMMARY_ROWS: usize = 3;

pub fn draw(
    frame: &mut Frame,
    app: &App,
    state: &DiffState,
    modal: &ReviewModal,
    body: Rect,
    hits: &mut HitMap,
) {
    let palette = &app.palette;
    let Some(data) = state.data.as_ref() else {
        return;
    };
    let width = 66.min(body.width.saturating_sub(2));
    let inner_width = usize::from(width.saturating_sub(4));
    let pending = data.draft.comments.len();
    let text = |s: String| Line::styled(s, style::fg(palette, Role::Text));
    let muted = |s: String| Line::styled(s, style::fg(palette, Role::Muted));
    let bold = |s: String| {
        Line::styled(
            s,
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        )
    };

    let mut lines: Vec<Line> = Vec::new();
    let mut verdict_cells: Vec<(Verdict, u16, u16)> = Vec::new();
    lines.push(verdict_line(app, modal, &mut verdict_cells));
    let verdict_row = 0usize;
    lines.push(Line::raw(""));
    lines.push(bold(modal.preview(&data.draft)));
    match modal.problem(pending) {
        Some(why) => lines.push(Line::styled(
            format!("! {why}"),
            style::fg(palette, Role::Warning),
        )),
        None => lines.push(muted(format!(
            "Verdict: {}",
            verdict_label(modal.verdict).to_lowercase()
        ))),
    }
    lines.push(Line::raw(""));
    let required = modal.verdict == Verdict::RequestChanges;
    lines.push(muted(if required {
        "Summary (required)".to_string()
    } else {
        "Summary (optional)".to_string()
    }));
    let summary_row = lines.len();
    let (row, col) = modal.summary.cursor();
    let first = (row + 1).saturating_sub(SUMMARY_ROWS);
    let across = col.saturating_sub(inner_width.saturating_sub(3));
    let focused = modal.focus == ReviewFocus::Summary;
    let caret = Style::default().add_modifier(Modifier::REVERSED);
    for r in first..first + SUMMARY_ROWS {
        let line = modal.summary.lines().get(r).cloned().unwrap_or_default();
        let shown: Vec<char> = line.chars().skip(across).collect();
        let mut spans = vec![Span::styled("▏", style::fg(palette, Role::Line))];
        if focused && r == row {
            let at = col - across;
            let before: String = shown.iter().take(at).collect();
            let under = shown.get(at).copied();
            let after: String = shown.iter().skip(at + 1).collect();
            spans.push(Span::styled(
                clip(&before, inner_width),
                style::fg(palette, Role::Text),
            ));
            spans.push(Span::styled(under.map_or(" ".into(), String::from), caret));
            spans.push(Span::styled(
                clip(&after, inner_width),
                style::fg(palette, Role::Text),
            ));
        } else {
            let all: String = shown.iter().collect();
            spans.push(Span::styled(
                clip(&all, inner_width.saturating_sub(2)),
                style::fg(palette, Role::Text),
            ));
        }
        lines.push(Line::from(spans));
    }

    if pending > 0 {
        lines.push(Line::raw(""));
        let limit = if body.height < 26 { 2 } else { 4 };
        for c in data.draft.comments.iter().take(limit) {
            lines.push(text(format!(
                "• {}",
                truncate(&comment_line(c), inner_width.saturating_sub(2))
            )));
        }
        if pending > limit {
            lines.push(muted(format!("  and {} more", pending - limit)));
        }
    }
    lines.push(Line::raw(""));
    let button_row = lines.len();
    let can_submit = modal.problem(pending).is_none();
    lines.push(buttons(app, state, modal, can_submit));
    lines.push(muted(if state.submitting {
        "sending…".to_string()
    } else {
        key_hint(modal.focus, app.kitty_keys).to_string()
    }));
    if let Some(error) = &modal.error {
        for (i, part) in wrap(error, inner_width.saturating_sub(2))
            .into_iter()
            .enumerate()
        {
            let glyph = if i == 0 { "✗ " } else { "  " };
            lines.push(Line::styled(
                format!("{glyph}{part}"),
                style::fg(palette, Role::Danger),
            ));
        }
    }

    let height = (lines.len() as u16 + 2).min(body.height);
    let rect = Rect::new(
        body.x + body.width.saturating_sub(width) / 2,
        body.y + body.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style::fg(palette, Role::Accent))
        .title(Line::styled(
            " Submit your review ",
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1))
        .style(style::bg(palette, Role::Raised));
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);

    let bottom = rect.bottom().saturating_sub(1);
    let inside = |row: u16| row < bottom;
    let x0 = rect.x + 2;
    let y0 = rect.y + 1;
    if inside(y0 + verdict_row as u16) {
        for (verdict, start, len) in verdict_cells {
            hits.push(
                Rect::new(x0 + start, y0 + verdict_row as u16, len, 1),
                Action::ReviewVerdict(verdict),
            );
        }
    }
    let summary_y = y0 + summary_row as u16;
    if summary_y + SUMMARY_ROWS as u16 <= bottom {
        hits.push(
            Rect::new(
                x0 + 1,
                summary_y,
                width.saturating_sub(5),
                SUMMARY_ROWS as u16,
            ),
            Action::SummaryCursor {
                x: x0 + 1,
                y: summary_y,
                first,
                across,
            },
        );
    }
    let row = y0 + button_row as u16;
    if inside(row) {
        let safe_w = (UnicodeWidthStr::width("Cancel") + 4) as u16;
        let go_w = (UnicodeWidthStr::width(submit_label(modal.verdict)) + 4) as u16;
        hits.push(Rect::new(x0, row, safe_w, 1), Action::ReviewButton(false));
        hits.push(
            Rect::new(x0 + safe_w + 2, row, go_w, 1),
            Action::ReviewButton(true),
        );
    }
}

/// `(•) 1 Comment  ( ) 2 Approve  ( ) 3 Request changes`, with each option's offset and width
/// recorded for the mouse. The chosen one is marked with `•` as well as colour.
fn verdict_line(
    app: &App,
    modal: &ReviewModal,
    cells: &mut Vec<(Verdict, u16, u16)>,
) -> Line<'static> {
    let palette = &app.palette;
    let mut spans = Vec::new();
    let mut at = 0u16;
    for (i, verdict) in modal.verdicts.iter().enumerate() {
        let chosen = *verdict == modal.verdict;
        let mark = if chosen { "•" } else { " " };
        let label = format!("({mark}) {} {}", i + 1, verdict_label(*verdict));
        let len = UnicodeWidthStr::width(label.as_str()) as u16;
        let mut st = if chosen {
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD)
        } else {
            style::fg(palette, Role::Muted)
        };
        if chosen && modal.focus == ReviewFocus::Verdict {
            st = st.add_modifier(Modifier::REVERSED);
        }
        cells.push((*verdict, at, len));
        spans.push(Span::styled(label, st));
        spans.push(Span::raw("  "));
        at += len + 2;
    }
    Line::from(spans)
}

/// `[ Cancel ]  [ Approve ]`, with `›` and bold marking the focused one so colour isn't the
/// only signal. Submit reads as unavailable while something is missing.
fn buttons(app: &App, state: &DiffState, modal: &ReviewModal, can_submit: bool) -> Line<'static> {
    let palette = &app.palette;
    let button = |label: &str, focused: bool, enabled: bool| {
        if focused {
            Span::styled(
                format!("› {label} ‹"),
                style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
        } else if enabled {
            Span::styled(format!("  {label}  "), style::fg(palette, Role::Text))
        } else {
            Span::styled(format!("  {label}  "), style::fg(palette, Role::Muted))
        }
    };
    let go = if state.submitting {
        "sending…"
    } else {
        submit_label(modal.verdict)
    };
    Line::from(vec![
        button("Cancel", modal.focus == ReviewFocus::Cancel, true),
        Span::raw("  "),
        button(go, modal.focus == ReviewFocus::Submit, can_submit),
    ])
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match out.last_mut() {
            Some(line)
                if UnicodeWidthStr::width(line.as_str()) + 1 + UnicodeWidthStr::width(word)
                    <= width =>
            {
                line.push(' ');
                line.push_str(word);
            }
            _ => out.push(word.to_string()),
        }
    }
    out
}
