//! The top bar, footer, and toast stack shared by every screen.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use rb_theme::Role;
use unicode_width::UnicodeWidthStr;

use super::{style, HitMap};
use crate::app::{Action, App, Entry, NoticeKind, Screen};

fn width(s: &str) -> u16 {
    UnicodeWidthStr::width(s).min(usize::from(u16::MAX)) as u16
}

pub const WORDMARK: &str = "review buddy";

/// A key hint: accent key letter, muted label, optionally clickable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub key: &'static str,
    pub label: &'static str,
    pub action: Option<Action>,
}

pub fn hints_for(screen: Screen) -> Vec<Hint> {
    match screen {
        Screen::Dashboard => vec![
            Hint {
                key: "T",
                label: "theme",
                action: Some(Action::CycleTheme),
            },
            Hint {
                key: "q",
                label: "quit",
                action: Some(Action::Quit),
            },
        ],
    }
}

const HINT_GAP: u16 = 2;

/// Where each hint that fits within `width` cells starts. Later hints drop first.
pub fn place_hints(hints: &[Hint], width_cells: u16) -> Vec<(u16, &Hint)> {
    let mut x = 0u16;
    let mut out = Vec::new();
    for hint in hints {
        let w = hint_width(hint);
        let needed = if out.is_empty() { w } else { HINT_GAP + w };
        if x.saturating_add(needed) > width_cells {
            break;
        }
        let start = if out.is_empty() { x } else { x + HINT_GAP };
        out.push((start, hint));
        x = start + w;
    }
    out
}

fn hint_width(hint: &Hint) -> u16 {
    width(hint.key) + 1 + width(hint.label)
}

pub fn draw_top_bar(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let mut spans = vec![Span::raw(" ")];
    spans.extend(wordmark_spans(app));
    let info = format!("  ·  {} · {} changes", app.source_label, app.change_count);
    spans.push(Span::styled(info, style::fg(palette, Role::Muted)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    let theme = format!(" {} ", app.theme_name());
    let w = width(&theme);
    if w < area.width {
        let rect = Rect::new(area.right() - w, area.y, w, 1);
        let line = Line::styled(theme, style::fg(palette, Role::TextSecondary));
        frame.render_widget(Paragraph::new(line), rect);
        hits.push(rect, Action::CycleTheme);
    }
}

fn wordmark_spans(app: &App) -> Vec<Span<'static>> {
    let palette = &app.palette;
    let stops = palette.wordmark();
    let last = WORDMARK.chars().count().saturating_sub(1).max(1) as f32;
    WORDMARK
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let st = match style::gradient_at(&stops, i as f32 / last) {
                Some(c) => ratatui::style::Style::default().fg(style::colour(c)),
                None => style::fg(palette, Role::TextBright),
            };
            Span::styled(ch.to_string(), st.add_modifier(Modifier::BOLD))
        })
        .collect()
}

pub fn draw_footer(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let hints = hints_for(app.screen);

    let status = app.status.as_ref().map(status_text);
    let status_w = status.as_ref().map_or(0, |(t, _)| width(t) + 1);
    let hint_room = area.width.saturating_sub(1 + status_w);

    let mut spans = vec![Span::raw(" ")];
    let mut cursor = 0u16;
    for (x, hint) in place_hints(&hints, hint_room) {
        let pad = x - cursor;
        spans.push(Span::raw(" ".repeat(usize::from(pad))));
        spans.push(Span::styled(
            hint.key,
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {}", hint.label),
            style::fg(palette, Role::Muted),
        ));
        cursor = x + hint_width(hint);
        if let Some(action) = &hint.action {
            hits.push(
                Rect::new(area.x + 1 + x, area.y, hint_width(hint), 1),
                action.clone(),
            );
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    if let Some((text, role)) = status {
        let w = width(&text);
        let rect = Rect::new(area.right() - w - 1, area.y, w, 1);
        frame.render_widget(
            Paragraph::new(Line::styled(text, style::fg(palette, role))),
            rect,
        );
    }
}

fn status_text(entry: &Entry) -> (String, Role) {
    let (glyph, role) = notice_look(entry.notice.kind);
    (format!("{glyph} {}", entry.notice.text), role)
}

fn notice_look(kind: NoticeKind) -> (&'static str, Role) {
    match kind {
        NoticeKind::Info => ("·", Role::TextSecondary),
        NoticeKind::Success => ("✓", Role::Success),
        NoticeKind::Warning => ("!", Role::Warning),
    }
}

const TOAST_MAX_WIDTH: u16 = 60;

/// Draws toasts stacked upward from the bottom-right of `area`, newest at the bottom.
pub fn draw_toasts(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let mut bottom = area.bottom();
    for entry in app.toasts.iter().rev() {
        let (glyph, role) = notice_look(entry.notice.kind);
        let w = (width(&entry.notice.text) + 6)
            .min(TOAST_MAX_WIDTH)
            .min(area.width);
        if bottom < area.y + 3 || w < 7 {
            break;
        }
        let text = format!(
            "{glyph} {}",
            truncate(&entry.notice.text, usize::from(w - 6))
        );
        let rect = Rect::new(area.right() - w, bottom - 3, w, 3);
        bottom -= 3;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(style::fg(palette, role))
            .style(style::bg(palette, Role::Raised));
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(Line::styled(text, style::fg(palette, Role::Text))).block(block),
            rect,
        );
        hits.push(rect, Action::DismissToast(entry.id));
    }
}

/// Shortens `text` to at most `max` cells, ending in an ellipsis when cut.
pub fn truncate(text: &str, max: usize) -> String {
    if UnicodeWidthStr::width(text) <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_cuts_on_cell_width() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefgh", 5), "abcd…");
        assert_eq!(truncate("日本語です", 5), "日本…");
        assert_eq!(truncate("abc", 0), "…");
    }

    fn hint(key: &'static str, label: &'static str) -> Hint {
        Hint {
            key,
            label,
            action: None,
        }
    }

    #[test]
    fn hints_are_placed_with_gaps() {
        let hints = [hint("T", "theme"), hint("q", "quit")];
        let placed = place_hints(&hints, 80);
        assert_eq!(placed.iter().map(|(x, _)| *x).collect::<Vec<_>>(), [0, 9]);
    }

    #[test]
    fn hints_that_do_not_fit_drop_from_the_end() {
        let hints = [hint("T", "theme"), hint("q", "quit")];
        assert_eq!(place_hints(&hints, 14).len(), 1);
        assert_eq!(place_hints(&hints, 15).len(), 2);
        assert!(place_hints(&hints, 5).is_empty());
        assert!(place_hints(&hints, 0).is_empty());
    }

    #[test]
    fn dashboard_hints_are_clickable_and_lowercase_labels() {
        for h in hints_for(Screen::Dashboard) {
            assert!(h.action.is_some());
            assert_eq!(h.label, h.label.to_lowercase());
        }
    }
}
