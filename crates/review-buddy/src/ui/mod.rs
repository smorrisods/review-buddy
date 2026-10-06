//! Rendering. `draw` paints one frame from `&App` and returns the click targets it laid out.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::Modifier,
    text::Line,
    widgets::Paragraph,
    Frame,
};
use rb_theme::Role;

use crate::app::{App, Screen};

pub mod chrome;
pub mod composer;
pub mod dashboard;
pub mod detail;
pub mod diff;
pub mod first_run;
pub mod help;
mod hitmap;
pub mod layout;
pub mod settings;
pub mod show;
pub mod size;
pub mod style;
pub mod text;

pub use hitmap::HitMap;

/// Draws the whole interface. The returned [`HitMap`] belongs to this frame.
pub fn draw(frame: &mut Frame, app: &App) -> HitMap {
    let mut hits = HitMap::default();
    let area = frame.area();
    paint_background(frame, app, area);

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
        Screen::Dashboard => dashboard::draw(frame, app, body, &mut hits),
        Screen::Diff => diff::draw(frame, app, body, &mut hits),
        Screen::FirstRun => first_run::draw(frame, app, body, &mut hits),
        Screen::Settings => settings::draw(frame, app, body, &mut hits),
    }
    chrome::draw_footer(frame, app, footer, &mut hits);
    chrome::draw_toasts(frame, app, body.inner(Margin::new(2, 1)), &mut hits);
    if app.show.open && app.screen == Screen::Dashboard {
        show::draw(frame, app, body, &mut hits);
    }
    if app.help {
        help::draw(frame, app, body);
    }
    hits
}

/// Fills the frame with the resolved background, when there is one, so every cell that a
/// widget leaves alone still carries it.
fn paint_background(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(colour) = app.palette.background() {
        let fill = ratatui::style::Style::default().bg(style::colour(colour));
        frame.render_widget(ratatui::widgets::Block::default().style(fill), area);
    }
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
