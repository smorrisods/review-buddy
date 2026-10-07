//! The docked composer and the confirmation modal that can sit over it.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
    Frame,
};
use rb_theme::Role;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::text::wrap;
use super::{chrome::truncate, layout, style, HitMap};
use crate::app::composer::{Composer, Confirm, ConfirmKind, Target};
use crate::app::{Action, App, DiffState};

pub fn draw(
    frame: &mut Frame,
    app: &App,
    state: &DiffState,
    l: &layout::DiffLayout,
    body: Rect,
    hits: &mut HitMap,
) {
    if let Some(composer) = &state.composer {
        draw_composer(frame, app, composer, l, hits);
    }
    if let Some(confirm) = &state.confirm {
        draw_confirm(frame, app, confirm, body, hits);
    }
    if let Some(modal) = &state.review {
        super::review::draw(frame, app, state, modal, body, hits);
    }
}

fn draw_composer(
    frame: &mut Frame,
    app: &App,
    composer: &Composer,
    l: &layout::DiffLayout,
    hits: &mut HitMap,
) {
    let palette = &app.palette;
    let rect = layout::composer_dock(l.code, composer.editor.line_count());
    let hint = if composer.sending {
        " sending… "
    } else {
        match composer.target {
            Target::Edit { .. } => " ⏎ save · ⇧⏎ newline · esc cancel ",
            _ => " ⏎ add to review · ⌃⏎ post now · ⇧⏎ newline · esc discard ",
        }
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style::fg(palette, Role::Accent))
        .title(Line::styled(
            format!(
                " {} ",
                truncate(&composer.title(), usize::from(rect.width.saturating_sub(4)))
            ),
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::styled(hint, style::fg(palette, Role::Muted)))
        .style(style::bg(palette, Role::Raised));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);

    let width = usize::from(inner.width.saturating_sub(2));
    let height = usize::from(inner.height);
    let (row, col) = composer.editor.cursor();
    let first = (row + 1).saturating_sub(height);
    let across = col.saturating_sub(width.saturating_sub(1));
    let text_style = style::fg(palette, Role::Text);
    let caret = Style::default().add_modifier(Modifier::REVERSED);

    let lines: Vec<Line> = composer
        .editor
        .lines()
        .iter()
        .enumerate()
        .skip(first)
        .take(height)
        .map(|(r, text)| {
            let shown: Vec<char> = text.chars().skip(across).collect();
            let mut spans = Vec::new();
            if r == row {
                let at = col - across;
                let before: String = shown.iter().take(at).collect();
                let under = shown.get(at).copied();
                let after: String = shown.iter().skip(at + 1).collect();
                spans.push(Span::styled(clip(&before, width), text_style));
                spans.push(Span::styled(under.map_or(" ".into(), String::from), caret));
                spans.push(Span::styled(clip(&after, width), text_style));
            } else {
                let all: String = shown.iter().collect();
                spans.push(Span::styled(clip(&all, width), text_style));
            }
            Line::from(spans)
        })
        .collect();
    let text_area = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    frame.render_widget(Paragraph::new(lines), text_area);
    hits.push(
        text_area,
        Action::ComposerCursor {
            x: text_area.x,
            y: text_area.y,
            first,
            across,
        },
    );
}

/// Cuts `text` so it fits `width` cells, with tabs shown as spaces.
pub(super) fn clip(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let c = if c == '\t' { ' ' } else { c };
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

fn draw_confirm(frame: &mut Frame, app: &App, confirm: &Confirm, body: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let width = 64.min(body.width.saturating_sub(4));
    let text_width = usize::from(width.saturating_sub(4));
    let text = |s: String| Line::styled(s, style::fg(palette, Role::Text));
    let muted = |s: String| Line::styled(s, style::fg(palette, Role::Muted));

    let mut lines: Vec<Line> = Vec::new();
    match &confirm.kind {
        ConfirmKind::Discard => {
            lines.push(text("Your unsent text will be lost.".into()));
        }
        ConfirmKind::Leave { summary_only, .. } => {
            lines.push(text("Nothing has been sent to the forge.".into()));
            lines.push(muted(if app.drafts.persists() {
                "A kept draft is saved on this computer and waits for you.".into()
            } else {
                "A kept draft lasts this session only; it isn't saved to disk.".into()
            }));
            if *summary_only {
                lines.push(muted("Discard throws your summary away.".into()));
            } else {
                lines.push(muted("Discard throws these comments away.".into()));
            }
        }
        ConfirmKind::DeleteComment { what, remote, .. } => {
            for line in wrap(what, text_width).into_iter().take(3) {
                lines.push(text(line));
            }
            lines.push(Line::raw(""));
            lines.push(muted(if *remote {
                "It is removed from your pending review on the forge.".into()
            } else {
                "It is removed from your pending review.".into()
            }));
        }
        ConfirmKind::SaveRemote { what } => {
            for line in wrap(what, text_width).into_iter().take(3) {
                lines.push(text(line));
            }
            lines.push(Line::raw(""));
            lines.push(muted(
                "It changes your pending comment on the forge. Nothing is published until you submit.".into(),
            ));
        }
        ConfirmKind::Outdated { what, .. } => {
            for line in wrap(what, text_width).into_iter().take(3) {
                lines.push(text(line));
            }
            lines.push(Line::raw(""));
            lines.push(muted(
                "The code changed and this line is gone. Add it to your summary to post it as a general comment.".into(),
            ));
        }
        ConfirmKind::PostNow { summary, body } => {
            lines.push(Line::styled(
                summary.clone(),
                style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
            ));
            lines.push(muted(
                "It goes to the forge straight away, on its own.".into(),
            ));
            lines.push(Line::raw(""));
            for line in wrap(body, text_width).into_iter().take(4) {
                lines.push(text(line));
            }
        }
    }
    lines.push(Line::raw(""));
    let button_row = lines.len();
    lines.push(buttons(app, confirm));

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
            format!(" {} ", confirm.title()),
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1))
        .style(style::bg(palette, Role::Raised));
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);

    let row = rect.y + 1 + (button_row as u16);
    if let Some(labels) = confirm.choices() {
        let mut x = rect.x + 2;
        for (i, label) in labels.iter().enumerate() {
            let w = (UnicodeWidthStr::width(*label) + 4) as u16;
            if row < rect.bottom().saturating_sub(1) {
                hits.push(Rect::new(x, row, w, 1), Action::Choose(i));
            }
            x += w + 2;
        }
        return;
    }
    let (safe, go) = confirm.buttons();
    let safe_w = (UnicodeWidthStr::width(safe) + 4) as u16;
    let go_w = (UnicodeWidthStr::width(go) + 4) as u16;
    let x = rect.x + 2;
    if row < rect.bottom().saturating_sub(1) {
        hits.push(Rect::new(x, row, safe_w, 1), Action::Answer(false));
        hits.push(
            Rect::new(x + safe_w + 2, row, go_w, 1),
            Action::Answer(true),
        );
    }
}

fn choice_button(app: &App, label: &str, focused: bool) -> Span<'static> {
    let palette = &app.palette;
    if focused {
        Span::styled(
            format!("› {label} ‹"),
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )
    } else {
        Span::styled(format!("  {label}  "), style::fg(palette, Role::Muted))
    }
}

/// `[ Cancel ]  [ Approve ]`, with `›` and bold marking the focused one so colour isn't the
/// only signal.
fn buttons(app: &App, confirm: &Confirm) -> Line<'static> {
    let palette = &app.palette;
    if let Some(labels) = confirm.choices() {
        let mut spans = Vec::new();
        for (i, label) in labels.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(choice_button(app, label, confirm.focus == i));
        }
        return Line::from(spans);
    }
    let (safe, go) = confirm.buttons();
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
    let hint = Span::styled("   ⏎ choose · esc cancel", style::fg(palette, Role::Muted));
    Line::from(vec![
        button(safe, !confirm.yes),
        Span::raw("  "),
        button(go, confirm.yes),
        hint,
    ])
}
