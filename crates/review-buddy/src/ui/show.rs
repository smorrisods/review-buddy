//! The Show filters overlay: five checkboxes, what each lets in, and what the filters hide.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use rb_theme::Role;

use super::{layout, style, text::cells, HitMap};
use crate::app::{queue, show, Action, App};

const MAX_WIDTH: u16 = 54;

pub fn draw(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let rows = show::FILTERS.len() as u16;
    let width = MAX_WIDTH.min(area.width.saturating_sub(2));
    let height = (rows + 6).min(area.height);
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
            " Show ",
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .style(style::bg(palette, Role::Raised));
    let inner = layout::inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);

    let name_w = show::FILTERS
        .iter()
        .map(|f| cells(f.as_str()))
        .max()
        .unwrap_or(0);
    let mut lines = vec![Line::raw("")];
    for (n, filter) in show::FILTERS.into_iter().enumerate() {
        let on = show::is_on(app, filter);
        let at = app.show.cursor == n;
        let name = format!("{:<name_w$}", filter.as_str());
        let name_style = if at {
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD)
        } else {
            style::fg(palette, if on { Role::Text } else { Role::Muted })
        };
        let line = Line::from(vec![
            Span::styled(
                if at { "▌ " } else { "  " },
                style::fg(palette, Role::Accent),
            ),
            Span::styled(
                format!("[{}] ", if on { "x" } else { " " }),
                style::fg(palette, Role::Accent),
            ),
            Span::styled(name, name_style),
            Span::styled(
                format!("  {}", show::describe(filter)),
                style::fg(palette, Role::Muted),
            ),
        ]);
        let y = inner.y + 1 + n as u16;
        if y < inner.bottom() {
            hits.push(
                Rect::new(inner.x, y, inner.width, 1),
                Action::ToggleShow(filter),
            );
        }
        lines.push(if at {
            line.style(style::bg(palette, Role::Selection))
        } else {
            line
        });
    }
    let hidden = queue::hidden_in(&app.state, app.active_source().map(|s| &s.id));
    let note = match hidden {
        0 => "Nothing is hidden.".to_string(),
        n => format!("{n} hidden by your Show filters."),
    };
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(note, style::fg(palette, Role::Muted)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "space",
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" toggle · ", style::fg(palette, Role::Muted)),
        Span::styled(
            "esc",
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " close · this session only",
            style::fg(palette, Role::Muted),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(lines).style(style::fg(palette, Role::Text)),
        inner,
    );
}
