//! The help overlay: a centred modal listing the keys for the current screen.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use rb_theme::Role;
use unicode_width::UnicodeWidthStr;

use super::{
    chrome::{registry, Binding},
    layout, style,
};
use crate::app::{App, Screen};

const MAX_WIDTH: u16 = 56;

fn screen_name(screen: Screen) -> &'static str {
    match screen {
        Screen::Dashboard => "Queue",
        Screen::Diff => "Diff",
        Screen::FirstRun => "First run",
    }
}

/// The overlay's rows as plain `(group, key, label)` triples, in display order.
pub fn rows(screen: Screen) -> Vec<Binding> {
    registry(screen)
}

/// How far the overlay can scroll when the body is too short to show every row.
pub fn max_scroll(app: &App) -> u16 {
    let area = layout::body(app.size);
    let total = content(app).len() as u16;
    let shown = (total + 2).min(area.height).saturating_sub(2);
    total.saturating_sub(shown)
}

fn content(app: &App) -> Vec<Line<'static>> {
    let palette = &app.palette;
    let bindings = rows(app.screen);
    let key_w = bindings
        .iter()
        .map(|b| UnicodeWidthStr::width(b.hint.key))
        .max()
        .unwrap_or(1);

    let mut lines: Vec<Line> = Vec::new();
    let mut group = "";
    for binding in &bindings {
        if binding.group != group {
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            group = binding.group;
            lines.push(Line::styled(
                group,
                style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
            ));
        }
        let pad = key_w - UnicodeWidthStr::width(binding.hint.key);
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                binding.hint.key,
                style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" ".repeat(pad + 2)),
            Span::styled(binding.hint.label, style::fg(palette, Role::Muted)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(
            "esc",
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" or ", style::fg(palette, Role::Muted)),
        Span::styled(
            "?",
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to close", style::fg(palette, Role::Muted)),
    ]));

    lines
}

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let palette = &app.palette;
    let lines = content(app);
    let width = MAX_WIDTH.min(area.width.saturating_sub(2));
    let height = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style::fg(palette, Role::Accent))
        .title(Line::styled(
            format!(" Keys · {} ", screen_name(app.screen)),
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .style(style::bg(palette, Role::Raised));
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.help_scroll.min(max_scroll(app)), 0))
            .style(style::fg(palette, Role::Text))
            .block(block),
        rect,
    );
}
