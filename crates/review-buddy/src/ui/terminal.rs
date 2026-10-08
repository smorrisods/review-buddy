//! Drawing the terminal pane and its start prompt, and where the pane sits.
//!
//! The geometry is pure and shared with `update` (through `app::terminal`), so the mouse targets
//! and the size of the child's grid can't drift from what is drawn.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
    Frame,
};
use rb_term::widget::TerminalWidget;
use rb_theme::Role;
use unicode_width::UnicodeWidthStr;

use super::chrome::Hint;
use super::layout::Size;
use super::text::wrap;
use super::{style, HitMap};
use crate::app::terminal::{Choice, Phase, TermAction};
use crate::app::{Action, App};
use crate::config::DetailPosition;

/// The pane never takes the app below this much room.
pub const APP_MIN_COLS: u16 = 70;
pub const APP_MIN_ROWS: u16 = 14;
/// The smallest box the pane can have, border included.
pub const PANE_MIN_COLS: u16 = 24;
pub const PANE_MIN_ROWS: u16 = 6;
/// `auto` puts the pane beside the app from this width.
pub const AUTO_SIDE_MIN: u16 = 150;
/// The pane's starting share of the body.
pub const DEFAULT_SHARE: u8 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
    Top,
    Bottom,
}

impl Side {
    /// The seam between the pane and the app is a vertical line (columns resize).
    pub fn vertical_seam(self) -> bool {
        matches!(self, Self::Right | Self::Left)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dock {
    pub side: Side,
    /// The pane with its border.
    pub outer: Rect,
    /// The grid the child draws in.
    pub inner: Rect,
    /// What is left for the app.
    pub app: Rect,
    /// The column or row where the second of the two areas starts.
    pub boundary: u16,
    /// The two border columns or rows either side of the boundary.
    pub hit: Rect,
}

fn side_for(body: Rect, position: DetailPosition) -> Option<Side> {
    let beside_fits = body.width >= APP_MIN_COLS + PANE_MIN_COLS;
    let stacked_fits = body.height >= APP_MIN_ROWS + PANE_MIN_ROWS;
    let side = match position {
        DetailPosition::Right => Side::Right,
        DetailPosition::Left => Side::Left,
        DetailPosition::Top => Side::Top,
        DetailPosition::Bottom => Side::Bottom,
        DetailPosition::Auto => {
            if body.width >= AUTO_SIDE_MIN {
                Side::Right
            } else {
                Side::Bottom
            }
        }
    };
    match side {
        Side::Right | Side::Left if beside_fits => Some(side),
        _ if stacked_fits => Some(if side.vertical_seam() {
            Side::Bottom
        } else {
            side
        }),
        _ => None,
    }
}

/// Where the pane goes in `body`, or `None` when there isn't room for it and the app.
pub fn dock_in(body: Rect, position: DetailPosition, size: Option<Size>) -> Option<Dock> {
    let side = side_for(body, position)?;
    let share = size.unwrap_or(Size::Percent(DEFAULT_SHARE));
    let (outer, app, boundary, hit) = if side.vertical_seam() {
        let hi = body.width - APP_MIN_COLS;
        let w = share.resolve(body.width).clamp(PANE_MIN_COLS, hi);
        let strip = |x: u16| {
            Rect::new(
                x.saturating_sub(1),
                body.y.saturating_add(1),
                2,
                body.height.saturating_sub(2),
            )
        };
        if side == Side::Right {
            let x = body.right() - w;
            (
                Rect::new(x, body.y, w, body.height),
                Rect::new(body.x, body.y, body.width - w, body.height),
                x,
                strip(x),
            )
        } else {
            let edge = body.x + w;
            (
                Rect::new(body.x, body.y, w, body.height),
                Rect::new(edge, body.y, body.width - w, body.height),
                edge,
                strip(edge),
            )
        }
    } else {
        let hi = body.height - APP_MIN_ROWS;
        let h = share.resolve(body.height).clamp(PANE_MIN_ROWS, hi);
        let strip = |y: u16| {
            Rect::new(
                body.x.saturating_add(1),
                y.saturating_sub(1),
                body.width.saturating_sub(2),
                2,
            )
        };
        if side == Side::Bottom {
            let y = body.bottom() - h;
            (
                Rect::new(body.x, y, body.width, h),
                Rect::new(body.x, body.y, body.width, body.height - h),
                y,
                strip(y),
            )
        } else {
            let edge = body.y + h;
            (
                Rect::new(body.x, body.y, body.width, h),
                Rect::new(body.x, edge, body.width, body.height - h),
                edge,
                strip(edge),
            )
        }
    };
    let inner = Rect::new(
        outer.x.saturating_add(1),
        outer.y.saturating_add(1),
        outer.width.saturating_sub(2),
        outer.height.saturating_sub(2),
    );
    Some(Dock {
        side,
        outer,
        inner,
        app,
        boundary,
        hit,
    })
}

pub use dock_in as dock;

/// The pane's size in cells for a seam dragged to `boundary`.
pub fn size_for(side: Side, body: Rect, boundary: u16) -> u16 {
    let b = i32::from(boundary);
    let cells = match side {
        Side::Right => i32::from(body.right()) - b,
        Side::Left => b - i32::from(body.x),
        Side::Top => b - i32::from(body.y),
        Side::Bottom => i32::from(body.bottom()) - b,
    };
    cells.clamp(1, i32::from(u16::MAX)) as u16
}

/// Splits the body into the app's area and the pane's, when the pane is on screen.
pub fn split(app: &App, body: Rect) -> (Rect, Option<Dock>) {
    match crate::app::terminal::dock(app) {
        Some(d) => (d.app, Some(d)),
        None => (body, None),
    }
}

fn pane_title(app: &App) -> String {
    let term = &app.term;
    let name = term
        .pane
        .as_ref()
        .and_then(|p| p.title().map(str::to_string))
        .or_else(|| term.launch.as_ref().map(|l| l.id.short_ref()))
        .unwrap_or_default();
    let focus = if term.focused { " (focused)" } else { "" };
    let place = if term.started_in.is_empty() {
        String::new()
    } else {
        format!(" · {}", term.started_in)
    };
    super::chrome::truncate(&format!(" terminal{focus} · {name}{place} "), 60)
}

/// The line on the pane's bottom border that says how to get out (or back in).
fn pane_hint(app: &App) -> String {
    let term = &app.term;
    let chord = term.settings.chord.label();
    if term.pane.as_ref().is_some_and(|p| p.exited().is_some()) {
        return " finished · any key closes it ".into();
    }
    if term.focused && term.escape.is_armed() {
        return format!(" {chord} …  esc app · t hide · x close · p move · < > size ");
    }
    if term.focused {
        format!(" {chord} then esc: back to the app ")
    } else {
        " t: type here · click to focus ".into()
    }
}

pub fn draw(frame: &mut Frame, app: &App, d: Dock) {
    let palette = &app.palette;
    let term = &app.term;
    let Some(pane) = term.pane.as_ref() else {
        return;
    };
    let (border_style, border_type) = if term.focused {
        (style::fg(palette, Role::Accent), BorderType::Thick)
    } else {
        (style::fg(palette, Role::Line), BorderType::Rounded)
    };
    let title_style = if term.focused {
        style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD)
    } else {
        style::fg(palette, Role::TextSecondary)
    };
    let screen = pane.screen();
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style)
        .title(Line::styled(pane_title(app), title_style))
        .title_bottom(Line::styled(pane_hint(app), title_style));
    if screen.scrolled > 0 {
        block = block.title(
            Line::styled(
                format!(" ↑ {} lines back ", screen.scrolled),
                style::fg(palette, Role::Warning),
            )
            .right_aligned(),
        );
    }
    frame.render_widget(block, d.outer);
    let mut base = style::fg(palette, Role::Text);
    if let Some(bg) = palette.background() {
        base = base.bg(style::colour(bg));
    }
    frame.render_widget(
        TerminalWidget::new(&screen)
            .base_style(base)
            .show_cursor(term.focused),
        d.inner,
    );
}

/// Footer hints while the pane has the keyboard.
pub fn focus_hints() -> Vec<Hint> {
    vec![
        Hint {
            key: "esc esc",
            label: "back to the app",
            action: None,
        },
        Hint {
            key: "chord esc",
            label: "also leaves (the chord is on the border)",
            action: None,
        },
    ]
}

/// Footer hints while the start prompt is open.
pub fn prompt_hints() -> Vec<Hint> {
    vec![
        Hint {
            key: "←→",
            label: "choose",
            action: None,
        },
        Hint {
            key: "⏎",
            label: "confirm",
            action: None,
        },
        Hint {
            key: "esc",
            label: "cancel",
            action: None,
        },
    ]
}

fn label(choice: Choice) -> &'static str {
    match choice {
        Choice::Cancel => "No, cancel",
        Choice::Worktree => "Create the worktree",
        Choice::Current => "Use the current directory",
    }
}

/// The start prompt: what will be created, with the safe answer first and chosen.
pub fn draw_prompt(frame: &mut Frame, app: &App, body: Rect, hits: &mut HitMap) {
    let Some(prompt) = app.term.prompt.as_ref() else {
        return;
    };
    let palette = &app.palette;
    let width = 78.min(body.width.saturating_sub(4));
    let text_width = usize::from(width.saturating_sub(4));
    let text = |s: String| Line::styled(s, style::fg(palette, Role::Text));
    let muted = |s: String| Line::styled(s, style::fg(palette, Role::Muted));
    let mut lines: Vec<Line> = Vec::new();
    let mut choices = prompt.choices();
    let mut picked = Choice::Cancel;
    match &prompt.phase {
        Phase::Planning => {
            lines.push(muted(
                "Looking for a local clone of this repository…".into(),
            ));
            choices.clear();
        }
        Phase::Creating(plan) => {
            lines.push(muted(format!("Creating {}…", plan.path.display())));
            choices.clear();
        }
        Phase::Choose {
            plan,
            choice,
            failed,
        } => {
            picked = *choice;
            match plan {
                Ok(plan) => {
                    for (i, l) in plan.preview().iter().enumerate() {
                        for w in wrap(l, text_width) {
                            lines.push(if i == 0 { text(w) } else { muted(w) });
                        }
                    }
                }
                Err(why) => {
                    for w in wrap(why, text_width) {
                        lines.push(text(w));
                    }
                }
            }
            if prompt.current_allowed {
                lines.push(Line::raw(""));
                for w in wrap(
                    "The current directory starts the terminal where Review Buddy was started, with the same environment variables, and creates nothing.",
                    text_width,
                ) {
                    lines.push(muted(w));
                }
            }
            if let Some(failed) = failed {
                lines.push(Line::raw(""));
                for w in wrap(failed, text_width) {
                    lines.push(Line::styled(w, style::fg(palette, Role::Warning)));
                }
            }
        }
    }
    lines.push(Line::raw(""));
    let button_row = lines.len();
    let mut spans: Vec<Span> = Vec::new();
    let mut spots: Vec<(u16, u16, Choice)> = Vec::new();
    let mut x = 0u16;
    for c in &choices {
        let focused = *c == picked;
        let name = label(*c);
        let shown = if focused {
            format!("› {name} ‹")
        } else {
            format!("  {name}  ")
        };
        let w = UnicodeWidthStr::width(shown.as_str()) as u16;
        spots.push((x, w, *c));
        spans.push(Span::styled(
            shown,
            if focused {
                style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                style::fg(palette, Role::Muted)
            },
        ));
        spans.push(Span::raw("  "));
        x += w + 2;
    }
    if !choices.is_empty() {
        lines.push(Line::from(spans));
        lines.push(muted("⏎ choose · ←→ switch · esc cancel".into()));
    } else {
        lines.push(muted("esc cancel".into()));
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
            format!(" Open a terminal for {} ", prompt.launch.id.short_ref()),
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1))
        .style(style::bg(palette, Role::Raised));
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
    let row = rect.y + 1 + button_row as u16;
    if row < rect.bottom().saturating_sub(1) {
        for (x, w, c) in spots {
            hits.push(
                Rect::new(rect.x + 2 + x, row, w, 1),
                Action::Terminal(TermAction::Answer(c)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(w: u16, h: u16) -> Rect {
        Rect::new(0, 1, w, h)
    }

    #[test]
    fn auto_goes_beside_when_wide_and_below_otherwise() {
        let wide = dock_in(body(160, 38), DetailPosition::Auto, None).unwrap();
        assert_eq!(wide.side, Side::Right);
        assert_eq!(wide.outer.width, 64);
        assert_eq!(wide.app.width + wide.outer.width, 160);
        let narrow = dock_in(body(100, 28), DetailPosition::Auto, None).unwrap();
        assert_eq!(narrow.side, Side::Bottom);
        assert_eq!(narrow.outer.height, 11);
        assert_eq!(narrow.app.height + narrow.outer.height, 28);
    }

    #[test]
    fn every_placement_tiles_the_body_without_overlap() {
        for position in [
            DetailPosition::Right,
            DetailPosition::Left,
            DetailPosition::Top,
            DetailPosition::Bottom,
        ] {
            let b = body(160, 38);
            let d = dock_in(b, position, None).unwrap();
            assert!(d.app.intersection(d.outer).is_empty(), "{position:?}");
            assert_eq!(d.app.area() + d.outer.area(), b.area(), "{position:?}");
            assert_eq!(d.inner.width + 2, d.outer.width);
        }
    }

    #[test]
    fn a_side_placement_falls_back_to_stacked_when_too_narrow() {
        let d = dock_in(body(90, 30), DetailPosition::Right, None).unwrap();
        assert_eq!(d.side, Side::Bottom);
    }

    #[test]
    fn no_room_means_no_dock() {
        assert!(dock_in(body(60, 15), DetailPosition::Auto, None).is_none());
    }

    #[test]
    fn sizes_are_clamped_to_leave_the_app_room() {
        let huge = dock_in(body(160, 38), DetailPosition::Right, Some(Size::Cells(500))).unwrap();
        assert_eq!(huge.app.width, APP_MIN_COLS);
        let tiny = dock_in(body(160, 38), DetailPosition::Right, Some(Size::Cells(2))).unwrap();
        assert_eq!(tiny.outer.width, PANE_MIN_COLS);
        let pct = dock_in(
            body(160, 38),
            DetailPosition::Bottom,
            Some(Size::Percent(50)),
        )
        .unwrap();
        assert_eq!(pct.outer.height, 19);
    }

    #[test]
    fn the_seam_hit_straddles_the_boundary() {
        let d = dock_in(body(160, 38), DetailPosition::Right, None).unwrap();
        assert_eq!(d.boundary, d.outer.x);
        assert!(d.hit.x < d.boundary && d.hit.right() > d.boundary);
        let b = dock_in(body(100, 30), DetailPosition::Bottom, None).unwrap();
        assert!(b.hit.y < b.boundary && b.hit.bottom() > b.boundary);
    }

    #[test]
    fn dragging_the_seam_maps_back_to_a_size() {
        let b = body(160, 38);
        for position in [
            DetailPosition::Right,
            DetailPosition::Left,
            DetailPosition::Top,
            DetailPosition::Bottom,
        ] {
            let d = dock_in(b, position, Some(Size::Cells(40))).unwrap();
            let want = if d.side.vertical_seam() {
                d.outer.width
            } else {
                d.outer.height
            };
            assert_eq!(size_for(d.side, b, d.boundary), want, "{position:?}");
        }
    }
}
