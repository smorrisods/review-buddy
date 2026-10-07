//! The Pending reviews overlay: every change with a saved draft, newest first.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use rb_theme::Role;

use super::{chrome::truncate, layout, style, text::cells, HitMap};
use crate::app::pending::{self, PendingList, Row};
use crate::app::{Action, App};

const MAX_WIDTH: u16 = 100;

pub fn draw(frame: &mut Frame, app: &App, list: &PendingList, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let rows = pending::rows(app);
    let width = MAX_WIDTH.min(area.width.saturating_sub(2));
    let room = pending::visible_rows(area.height).min(rows.len().max(1));
    let height = (room as u16 + 6).min(area.height);
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let persists = app.drafts.persists();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style::fg(palette, Role::Accent))
        .title(Line::styled(
            format!(" Pending reviews · {} ", rows.len()),
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::styled(
            " ⏎ open · x discard · esc close ",
            style::fg(palette, Role::Muted),
        ))
        .style(style::bg(palette, Role::Raised));
    let inner = layout::inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);

    let w = usize::from(inner.width);
    let mut lines = vec![Line::styled(
        truncate(
            if persists {
                "Drafts saved on this computer. Nothing here has been sent."
            } else {
                "Drafts kept for this session only. Nothing here has been sent."
            },
            w,
        ),
        style::fg(palette, Role::Muted),
    )];
    lines.push(Line::raw(""));
    let selected = list.selected.min(rows.len().saturating_sub(1));
    let start = selected
        .saturating_sub(room.saturating_sub(1))
        .min(rows.len().saturating_sub(room));
    for (n, row) in rows.iter().enumerate().skip(start).take(room) {
        let y = inner.y + lines.len() as u16;
        if y < inner.bottom() {
            hits.push(Rect::new(inner.x, y, inner.width, 1), Action::PendingRow(n));
        }
        lines.push(row_line(app, row, n == selected, w));
    }
    frame.render_widget(Paragraph::new(lines), inner);

    if let Some(confirm) = &list.confirm {
        draw_confirm(frame, app, confirm.yes, &rows, selected, area, hits);
    }
}

fn row_line(app: &App, row: &Row, at: bool, width: usize) -> Line<'static> {
    let palette = &app.palette;
    let mut tail = format!("✎ {}  {}", row.comments, row.age);
    if row.outdated {
        tail.push_str("  outdated");
    }
    if row.missing {
        tail.push_str("  not in queue");
    }
    let head = format!("{}  {}", row.source, short(&row.id));
    let room = width.saturating_sub(2 + cells(&head) + 2 + cells(&tail) + 2);
    let title = truncate(&row.title, room);
    let used = 2 + cells(&head) + 2 + cells(&title) + cells(&tail);
    let pad = width.saturating_sub(used);
    let line = Line::from(vec![
        Span::styled(
            if at { "▌ " } else { "  " },
            style::fg(palette, Role::Accent),
        ),
        Span::styled(head, style::fg(palette, Role::Cyan)),
        Span::raw("  "),
        Span::styled(
            title,
            if at {
                style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD)
            } else {
                style::fg(palette, Role::Text)
            },
        ),
        Span::raw(" ".repeat(pad)),
        Span::styled(tail, style::fg(palette, Role::Accent)),
    ]);
    if at {
        line.style(style::bg(palette, Role::Selection))
    } else {
        line
    }
}

fn short(id: &rb_core::ChangeId) -> String {
    id.short_ref()
}

fn draw_confirm(
    frame: &mut Frame,
    app: &App,
    yes: bool,
    rows: &[Row],
    selected: usize,
    area: Rect,
    hits: &mut HitMap,
) {
    let palette = &app.palette;
    let width = 60.min(area.width.saturating_sub(4));
    let text_width = usize::from(width.saturating_sub(4));
    let name = rows.get(selected).map(|r| short(&r.id)).unwrap_or_default();
    let button = |label: &str, focused: bool| {
        if focused {
            Span::styled(
                format!("› {label} ‹"),
                style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
        } else {
            Span::styled(format!("  {label}  "), style::fg(palette, Role::Muted))
        }
    };
    let lines = vec![
        Line::styled(
            truncate(&format!("Discard your draft on {name}?"), text_width),
            style::fg(palette, Role::Text),
        ),
        Line::styled(
            "Your unsent comments and summary are lost.",
            style::fg(palette, Role::Muted),
        ),
        Line::raw(""),
        Line::from(vec![
            button("No, keep it", !yes),
            Span::raw("  "),
            button("Discard", yes),
        ]),
    ];
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
            " Discard this draft? ",
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .padding(ratatui::widgets::Padding::horizontal(1))
        .style(style::bg(palette, Role::Raised));
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
    let row = rect.y + 4;
    let keep_w = (cells("No, keep it") + 4) as u16;
    let go_w = (cells("Discard") + 4) as u16;
    if row < rect.bottom().saturating_sub(1) {
        hits.push(Rect::new(rect.x + 2, row, keep_w, 1), Action::Answer(false));
        hits.push(
            Rect::new(rect.x + 2 + keep_w + 2, row, go_w, 1),
            Action::Answer(true),
        );
    }
}
