//! Headless-render helpers shared by `frames.rs` and `frames_v02.rs`: render an `App` into a
//! buffer, dump it as text, dump its bold/dim/reverse runs for `NO_COLOR` frames, and look up
//! cells and theme colours. Include it with `#[path = "support/render.rs"] mod render;`.
#![allow(dead_code)]

use ratatui::{
    backend::TestBackend,
    buffer::Buffer,
    style::{Color, Modifier},
    Terminal,
};
use rb_theme::Role;
use review_buddy::app::App;
use review_buddy::ui::{self, style, HitMap};

pub fn render(app: &mut App) -> Buffer {
    let (w, h) = app.size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|frame| hits = ui::draw(frame, app)).unwrap();
    app.hits = hits;
    terminal.backend().buffer().clone()
}

pub fn rows(buffer: &Buffer) -> Vec<String> {
    let width = usize::from(buffer.area.width);
    buffer
        .content()
        .chunks(width)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect()
}

pub fn text(buffer: &Buffer) -> String {
    let rows: Vec<String> = rows(buffer)
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect();
    rows.join("\n")
}

/// Every run of cells that share a bold/dim/reverse set, as `row:col B|D|R "text"`, in reading
/// order, so a `NO_COLOR` snapshot shows exactly where the modifiers carry the meaning.
pub fn modifier_dump(buffer: &Buffer) -> String {
    let width = usize::from(buffer.area.width);
    let mut out = Vec::new();
    for (y, row) in buffer.content().chunks(width).enumerate() {
        let mut x = 0;
        while x < row.len() {
            let tag = modifier_tag(&row[x]);
            let start = x;
            while x < row.len() && modifier_tag(&row[x]) == tag {
                x += 1;
            }
            if !tag.is_empty() {
                let run: String = row[start..x].iter().map(|c| c.symbol()).collect();
                out.push(format!("{y}:{start} {tag} {:?}", run.trim_end()));
            }
        }
    }
    out.join("\n")
}

fn modifier_tag(cell: &ratatui::buffer::Cell) -> String {
    [
        (Modifier::BOLD, "B"),
        (Modifier::DIM, "D"),
        (Modifier::REVERSED, "R"),
    ]
    .iter()
    .filter(|(m, _)| cell.modifier.contains(*m))
    .map(|(_, t)| *t)
    .collect()
}

pub fn plain_dump(app: &mut App) -> String {
    let buffer = render(app);
    assert!(
        buffer
            .content()
            .iter()
            .all(|c| c.fg == Color::Reset && c.bg == Color::Reset),
        "NO_COLOR paints no colour"
    );
    format!(
        "{}\n\n-- modifiers --\n{}",
        text(&buffer),
        modifier_dump(&buffer)
    )
}

pub fn find(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    rows(buffer).iter().enumerate().find_map(|(y, row)| {
        row.find(needle)
            .map(|byte| (row[..byte].chars().count() as u16, y as u16))
    })
}

pub fn role(app: &App, role: Role) -> Option<Color> {
    app.palette.colour(role).map(style::colour)
}
