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
use crate::app::{refresh, Action, App, Chip, Entry, NoticeKind, Screen};

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

/// One row of the key registry. The footer shows the `footer` entries; the help overlay
/// shows them all, so the two can't drift apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub hint: Hint,
    pub group: &'static str,
    pub footer: bool,
}

fn bind(
    group: &'static str,
    key: &'static str,
    label: &'static str,
    action: Option<Action>,
    footer: bool,
) -> Binding {
    Binding {
        hint: Hint { key, label, action },
        group,
        footer,
    }
}

/// Every key that works on `screen`, in the order the help overlay lists them.
pub fn registry(screen: Screen) -> Vec<Binding> {
    let mut bindings = screen_bindings(screen);
    #[cfg(unix)]
    bindings.push(bind("General", "⌃Z", "suspend", None, false));
    bindings
}

fn screen_bindings(screen: Screen) -> Vec<Binding> {
    let open = Some(Action::Open);
    let copy = Some(Action::Copy);
    let help = Some(Action::ToggleHelp);
    let theme = Some(Action::CycleTheme);
    let background = Some(Action::CycleBackground);
    match screen {
        Screen::Dashboard => vec![
            bind(
                "Move",
                "j/k",
                "move in the focused pane (PgUp/PgDn: detail)",
                None,
                false,
            ),
            bind("Move", "g/G", "first / last row", None, false),
            bind("Move", "tab", "next pane, in screen order", None, false),
            bind("Move", "h/l", "previous / next pane", None, false),
            bind("Move", "1-9", "switch source", None, false),
            bind("Move", "[/]", "previous / next detail tab", None, false),
            bind("Change", "⏎", "diff", Some(Action::Chip(Chip::Diff)), true),
            bind("Change", "o", "open", open, true),
            bind("Change", "y", "copy", copy, true),
            bind("Change", "s", "show filters", Some(Action::OpenShow), true),
            bind(
                "View",
                "p",
                "close / reopen the detail pane",
                Some(Action::ToggleDetail),
                false,
            ),
            bind(
                "View",
                "S",
                "sources: auto / left / top",
                Some(Action::CycleSources),
                false,
            ),
            bind(
                "View",
                "P",
                "rotate panes: list left → top → right → bottom",
                Some(Action::CyclePosition),
                false,
            ),
            bind(
                "View",
                "< >",
                "shrink / grow the focused pane (or drag a seam)",
                None,
                false,
            ),
            bind("View", "=", "reset that split to automatic", None, false),
            bind("View", "W", "save pane sizes to config", None, false),
            bind(
                "Change",
                "s /",
                "pick projects in show filters",
                None,
                false,
            ),
            bind("General", "?", "help", help, true),
            bind(
                "General",
                ",",
                "settings",
                Some(Action::OpenSettings),
                false,
            ),
            bind("General", "T", "theme", theme, true),
            bind(
                "General",
                "B",
                "background: theme / yes / no",
                background,
                false,
            ),
            bind("General", "q", "quit", Some(Action::Quit), true),
        ],
        Screen::Diff => vec![
            bind("Move", "j/k", "move", None, true),
            bind("Move", "g/G", "first / last line", None, false),
            bind("Move", "pgup/pgdn", "page up / down", None, false),
            bind("Move", "n/p", "hunk", None, true),
            bind("Move", "← →", "file", None, true),
            bind("Move", "]/[", "file", None, false),
            bind("Move", "tab", "pane", None, true),
            bind("Select", "⇧↑/⇧↓", "select lines", None, false),
            bind("Select", "⇧pgup/⇧pgdn", "select a page", None, false),
            bind(
                "Select",
                "V",
                "select lines with j/k, again to end",
                None,
                false,
            ),
            bind("Select", "esc", "clear the selection", None, false),
            bind("Review", "c", "comment", None, true),
            bind("Review", "r", "reply to the thread", None, false),
            bind("Review", "a", "approve", None, true),
            bind("Review", "x", "request changes", None, false),
            bind("Change", "o", "open", open, true),
            bind("Change", "y", "copy", copy, true),
            bind("General", "esc", "back", Some(Action::CloseDiff), true),
            bind(
                "General",
                "q",
                "back to the queue",
                Some(Action::CloseDiff),
                false,
            ),
            bind("General", "?", "help", help, true),
            bind("General", "T", "theme", theme, true),
            bind(
                "General",
                "B",
                "background: theme / yes / no",
                background,
                false,
            ),
        ],
        Screen::Settings => {
            use crate::settings::Click;
            let click = |c| Some(Action::Settings(c));
            vec![
                bind("Move", "j/k", "move between sources", None, false),
                bind("Move", "g/G", "first / last source", None, false),
                bind("Source", "t", "test token", click(Click::Test), true),
                bind("Source", "e", "edit", click(Click::Edit), true),
                bind("Source", "a", "add", click(Click::Add), true),
                bind("Source", "space", "on/off", click(Click::Toggle), true),
                bind("Source", "x", "remove", click(Click::Remove), true),
                bind("General", "esc", "back", click(Click::Back), true),
                bind("General", "?", "help", help, true),
                bind("General", "T", "theme", theme, false),
                bind(
                    "General",
                    "B",
                    "background: theme / yes / no",
                    background,
                    false,
                ),
            ]
        }
        Screen::FirstRun => vec![
            bind("First run", "⏎", "continue", None, true),
            bind("First run", "← →", "buttons", None, true),
            bind("First run", "⌫", "back", None, true),
            bind("First run", "space", "pick", None, true),
            bind("First run", "esc", "skip for now", None, true),
            bind("First run", "tab", "list / buttons", None, false),
            bind("First run", "← →", "theme (look step)", None, false),
        ],
    }
}

/// The footer for the first-run step on screen: the keys differ by step, because `←`/`→` change
/// the theme on the look step, belong to the text cursor in the token field, and there is
/// nothing to go back to on the welcome step.
pub fn first_run_hints(flow: &crate::setup::Flow) -> Vec<Hint> {
    use crate::setup::Step;
    let hint = |key, label| Hint {
        key,
        label,
        action: None,
    };
    if flow.token_open {
        return vec![
            hint("⏎", "continue"),
            hint("tab", "buttons"),
            hint("esc", "skip for now"),
        ];
    }
    match flow.step {
        Step::Welcome => vec![hint("⏎", "continue"), hint("esc", "skip for now")],
        Step::Look => vec![
            hint("⏎", "continue"),
            hint("← →", "theme"),
            hint("tab", "buttons"),
            hint("⌫", "back"),
            hint("esc", "skip for now"),
        ],
        Step::Connect | Step::Scope | Step::Jax => vec![
            hint("⏎", "continue"),
            hint("← →", "buttons"),
            hint("⌫", "back"),
            hint("space", "pick"),
            hint("esc", "skip for now"),
        ],
        Step::Summary | Step::Done => vec![
            hint("⏎", "continue"),
            hint("← →", "buttons"),
            hint("⌫", "back"),
            hint("esc", "skip for now"),
        ],
    }
}

/// The footer while a composer is open.
pub fn composer_hints() -> Vec<Hint> {
    let hint = |key, label| Hint {
        key,
        label,
        action: None,
    };
    vec![
        hint("⏎", "add to review"),
        hint("⌃⏎", "post now"),
        hint("⇧⏎", "newline"),
        hint("esc", "discard"),
    ]
}

pub fn hints_for(screen: Screen) -> Vec<Hint> {
    registry(screen)
        .into_iter()
        .filter(|b| b.footer)
        .map(|b| b.hint)
        .collect()
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
    let info = if app.screen == Screen::FirstRun {
        "  ·  first run".to_string()
    } else if app.screen == Screen::Settings {
        "  ·  settings".to_string()
    } else {
        format!("  ·  {} · {} changes", app.source_label, app.change_count)
    };
    spans.push(Span::styled(info, style::fg(palette, Role::Muted)));
    if let Some(cached) = app.state.offline_since_cache() {
        spans.push(Span::styled(
            format!("  ·  offline · cached {}", refresh::hhmm(cached)),
            style::fg(palette, Role::TextSecondary).add_modifier(Modifier::BOLD),
        ));
    } else if app.state.pending_sources > 0 {
        let glyph = refresh::spinner(app.ticks(), app.reduced_motion);
        spans.push(Span::styled(
            format!("  ·  {glyph} refreshing"),
            style::fg(palette, Role::Muted),
        ));
    }
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
    // Blend at full precision and quantise each letter, so 256 colours still show a gradient.
    let stops = palette.wordmark_blend_stops();
    let last = WORDMARK.chars().count().saturating_sub(1).max(1) as f32;
    WORDMARK
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let st = match style::gradient_at(&stops, i as f32 / last).map(|c| palette.at_depth(c))
            {
                Some(c) => ratatui::style::Style::default().fg(style::colour(c)),
                None => style::fg(palette, Role::TextBright),
            };
            Span::styled(ch.to_string(), st.add_modifier(Modifier::BOLD))
        })
        .collect()
}

pub fn draw_footer(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let hints = if app.diff_state().is_some_and(|s| s.composer.is_some()) {
        composer_hints()
    } else if let Some(hints) = app
        .settings
        .as_ref()
        .filter(|_| app.screen == Screen::Settings)
        .and_then(super::settings::modal_hints)
    {
        hints
    } else if let Some(flow) = app
        .setup
        .as_ref()
        .filter(|_| app.screen == Screen::FirstRun)
    {
        first_run_hints(flow)
    } else {
        let mut hints = hints_for(app.screen);
        if app.screen == Screen::Dashboard && !app.layout.detail_open() {
            let at = hints
                .iter()
                .position(|h| h.key == "?")
                .unwrap_or(hints.len());
            hints.insert(
                at,
                Hint {
                    key: "p",
                    label: "show detail",
                    action: Some(Action::ToggleDetail),
                },
            );
        }
        hints
    };

    let fit = |text: String, role| {
        let room = usize::from(area.width.saturating_sub(4));
        (truncate(&text, room), role)
    };
    let mut status = app.status.as_ref().map(status_text).map(|(t, r)| fit(t, r));
    if status.is_none() {
        status = app
            .diff_state()
            .filter(|_| app.screen == Screen::Diff)
            .and_then(|s| s.range.map(|r| (s, r)))
            .map(|(s, r)| {
                let (top, bottom) = r.bounds();
                let n = s.view.rows.lines_between(top, bottom);
                fit(format!("{n} lines selected"), Role::Muted)
            });
    }
    if status.is_none() {
        // The passive timestamp gives way to the hints: it only shows when none of them is cut.
        status = app
            .state
            .last_refreshed
            .map(|at| fit(format!("refreshed {}", refresh::hhmm(at)), Role::Muted))
            .filter(|(t, _)| {
                let room = area.width.saturating_sub(2 + width(t));
                place_hints(&hints, room).len() == hints.len()
            });
    }
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
    fn first_run_hints_follow_the_step() {
        use crate::setup::{Flow, Step};
        let mut flow = Flow::new("/c/config.toml".into(), false, "liminal-hq", true);
        let keys = |f: &Flow| {
            first_run_hints(f)
                .iter()
                .map(|h| format!("{} {}", h.key, h.label))
                .collect::<Vec<_>>()
        };
        flow.step = Step::Welcome;
        assert!(!keys(&flow).iter().any(|k| k.contains("back")));
        flow.step = Step::Look;
        assert!(keys(&flow).contains(&"← → theme".to_string()));
        assert!(keys(&flow).contains(&"tab buttons".to_string()));
        assert!(!keys(&flow).iter().any(|k| k == "← → buttons"));
        flow.step = Step::Scope;
        assert!(keys(&flow).contains(&"← → buttons".to_string()));
        flow.token_open = true;
        assert!(!keys(&flow).iter().any(|k| k.starts_with('⌫')));
    }

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

    fn footer_text(width: u16) -> String {
        use crate::app::AppConfig;
        use ratatui::{backend::TestBackend, Terminal};
        let mut app = App::new(AppConfig::from_env((width, 30)));
        app.state.last_refreshed = Some(rb_core::Timestamp(1_000_000));
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|f| draw_footer(f, &app, f.area(), &mut HitMap::default()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn the_refreshed_time_gives_way_to_the_hints() {
        let wide = footer_text(120);
        assert!(wide.contains("s show filters") && wide.contains("refreshed"));
        let narrow = footer_text(70);
        assert!(narrow.contains("s show filters") && !narrow.contains("refreshed"));
    }
}
