//! The diff pane for an image file: the original and the replacement, laid out as
//! `images::diff::Layout` says, each under a caption that names it and its size.

use ratatui::{layout::Rect, text::Line, widgets::Paragraph, Frame};
use rb_core::FileStatus;
use rb_theme::Role;

use super::style;
use crate::app::App;
use crate::images::diff::{self, Cell};
use crate::images::{Slot, View};

/// Draws the pair into `area`. Returns `false` when the file isn't a picture to show, so the
/// caller draws its usual note instead.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) -> bool {
    let Some((_, pair)) = crate::app::images::diff_pair(app) else {
        return false;
    };
    if !app.images.is_on() || area.width < 8 || area.height < 3 {
        return false;
    }
    let status = app
        .diff
        .as_ref()
        .and_then(|d| d.current())
        .map(|f| f.diff.status);
    let panels = diff::panels(app.image_layout, area.width, area.height);
    let sides = [
        (
            "Original",
            &pair.original,
            panels.original,
            FileStatus::Added,
        ),
        (
            "Replacement",
            &pair.replacement,
            panels.replacement,
            FileStatus::Removed,
        ),
    ];
    for (name, url, cell, missing_when) in sides {
        let Some(cell) = cell else { continue };
        let rect = Rect::new(area.x + cell.x, area.y + cell.y, cell.width, cell.height);
        let absent = status == Some(missing_when);
        draw_panel(
            frame,
            app,
            rect,
            name,
            url.as_ref().map(url::Url::as_str),
            absent,
        );
    }
    true
}

fn draw_panel(
    frame: &mut Frame,
    app: &App,
    rect: Rect,
    name: &str,
    url: Option<&str>,
    absent: bool,
) {
    if rect.height == 0 || rect.width == 0 {
        return;
    }
    let palette = &app.palette;
    let caption =
        |text: String| Paragraph::new(Line::styled(text, style::fg(palette, Role::Muted)));
    let (note, picture) = match (url, absent) {
        (_, true) => (Some("none: the file isn't there".to_string()), None),
        (None, false) => (Some("no address for this version".to_string()), None),
        (Some(url), false) => match app.images.view_url(url) {
            View::Ready(img) => (
                Some(format!("{}×{}", img.width(), img.height())),
                Some((url, img.width(), img.height())),
            ),
            View::Loading => (Some("loading…".to_string()), None),
            View::Failed(failure) => (Some(format!("couldn't load: {}", failure.reason())), None),
            View::Off => (Some("images are off".to_string()), None),
        },
    };
    let title = match note {
        Some(note) => format!("{name} · {note}"),
        None => name.to_string(),
    };
    frame.render_widget(
        caption(super::chrome::truncate(&title, usize::from(rect.width))),
        Rect::new(rect.x, rect.y, rect.width, 1),
    );
    let Some((url, w, h)) = picture else { return };
    let room = Cell {
        x: 0,
        y: 1,
        width: rect.width,
        height: rect.height - 1,
    };
    if room.height == 0 {
        return;
    }
    let (cols, rows) = diff::fit_in((w, h), app.images.font(), room.width, room.height);
    let slot = Slot {
        line: 0,
        cols,
        rows,
        url: url.to_string(),
    };
    let target = Rect::new(rect.x, rect.y + 1, cols, rows);
    app.images.render(frame, target, &slot, 0);
}

/// The caption for the pane's bottom edge while a picture is shown.
pub fn status(app: &App) -> Option<String> {
    crate::app::images::diff_pair(app)?;
    app.images
        .is_on()
        .then(|| format!("pictures: {} · i changes", app.image_layout.label()))
}
