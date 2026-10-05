//! Rendering. `draw` paints one frame from `&App` and returns the click targets it laid out.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::Modifier,
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use rb_theme::Role;

use crate::app::{App, Screen};

pub mod chrome;
mod hitmap;
pub mod size;
pub mod style;

pub use hitmap::HitMap;

/// Draws the whole interface. The returned [`HitMap`] belongs to this frame.
pub fn draw(frame: &mut Frame, app: &App) -> HitMap {
    let mut hits = HitMap::default();
    let area = frame.area();

    if size::is_too_small(area.width, area.height) {
        draw_too_small(frame, app, area);
        return hits;
    }

    let [top, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    chrome::draw_top_bar(frame, app, top, &mut hits);
    match app.screen {
        Screen::Dashboard => draw_dashboard(frame, app, body),
    }
    chrome::draw_footer(frame, app, footer, &mut hits);
    chrome::draw_toasts(frame, app, body.inner(Margin::new(2, 1)), &mut hits);
    hits
}

fn draw_dashboard(frame: &mut Frame, app: &App, area: Rect) {
    let palette = &app.palette;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style::fg(palette, Role::Accent))
        .title(Line::styled(
            " queue ",
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::styled(
            "Nothing connected yet.",
            style::fg(palette, Role::TextBright),
        ),
        Line::styled(
            "Press , to add GitHub or GitLab.",
            style::fg(palette, Role::Muted),
        ),
    ];
    let height = lines.len() as u16;
    let rect = Rect::new(
        inner.x,
        inner.y + inner.height.saturating_sub(height) / 2,
        inner.width,
        height.min(inner.height),
    );
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), rect);
}

fn draw_too_small(frame: &mut Frame, app: &App, area: Rect) {
    let palette = &app.palette;
    let lines = vec![
        Line::styled(
            size::headline(area.width, area.height),
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            size::detail(area.width, area.height),
            style::fg(palette, Role::Muted),
        ),
    ];
    let height = lines.len() as u16;
    let rect = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(height) / 2,
        area.width,
        height.min(area.height),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(ratatui::widgets::Wrap { trim: true }),
        rect,
    );
}
