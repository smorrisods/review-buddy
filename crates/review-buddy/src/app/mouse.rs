//! Mouse input as pure transitions. Clicks are resolved against the [`HitMap`](crate::ui::HitMap)
//! the last draw registered; drags and wheel turns use the same geometry the screens draw with.
//!
//! The polite rule: with mouse capture on, a terminal only forwards plain mouse events and keeps
//! Shift-drag for its own text selection. If one does deliver a Shift event anyway, the only one
//! used is a Shift-click in the diff, which extends the selected range. Every other Shift event
//! is ignored, never consumed.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{composer, dashboard, diff, resize, settings, show, update, Action, App, Cmd, Screen};
use crate::ui::help;

/// Two clicks on the same row this many ticks apart (a tick is 250 ms) make a double-click.
const DOUBLE_CLICK_TICKS: u64 = 2;

pub(super) fn on_mouse(app: &mut App, mouse: MouseEvent) -> Vec<Cmd> {
    if let Some(cmds) = super::selection::on_mouse(app, mouse) {
        return cmds;
    }
    if let Some(cmds) = super::terminal::on_mouse(app, mouse) {
        return cmds;
    }
    let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
    if app.help {
        return on_help(app, mouse, shift);
    }
    if app.pending.is_some() && app.screen == Screen::Dashboard {
        return on_pending(app, mouse, shift);
    }
    if app.show.open {
        return on_show(app, mouse, shift);
    }
    if app.screen == Screen::Settings {
        return settings::on_mouse(app, mouse, shift);
    }
    if app.diff.as_ref().is_some_and(|s| s.has_overlay()) {
        return on_overlay(app, mouse, shift);
    }
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => press(app, mouse, shift),
        MouseEventKind::Drag(MouseButton::Left) if !shift && app.drag.is_some() => {
            resize::drag(app, mouse.column, mouse.row);
            Vec::new()
        }
        MouseEventKind::Drag(MouseButton::Left) if !shift && app.screen == Screen::Diff => {
            diff::on_drag(app, mouse.row);
            Vec::new()
        }
        MouseEventKind::Up(MouseButton::Left) if app.drag.is_some() => resize::release(app),
        MouseEventKind::Up(MouseButton::Left) => {
            diff::on_release(app);
            Vec::new()
        }
        MouseEventKind::ScrollDown if !shift => update::scroll(app, mouse.column, mouse.row, true),
        MouseEventKind::ScrollUp if !shift => update::scroll(app, mouse.column, mouse.row, false),
        _ => Vec::new(),
    }
}

fn press(app: &mut App, mouse: MouseEvent, shift: bool) -> Vec<Cmd> {
    let hit = app.hits.at(mouse.column, mouse.row).cloned();
    if shift {
        return match hit {
            Some(Action::DiffRow(row)) if app.screen == Screen::Diff => {
                diff::on_press(app, row, true)
            }
            _ => Vec::new(),
        };
    }
    if hit.is_none() {
        let double = resize::is_double(app, mouse.column, mouse.row, DOUBLE_CLICK_TICKS);
        if let Some(cmds) = resize::press(app, mouse.column, mouse.row, double) {
            return cmds;
        }
    }
    if let Some((key, at)) =
        super::selection::start_target(app, mouse.column, mouse.row, hit.as_ref())
    {
        return press_text(app, mouse, hit, (key, at));
    }
    super::selection::clear(app);
    match hit {
        Some(Action::DiffRow(row)) if app.screen == Screen::Diff => {
            app.last_click = None;
            diff::on_press(app, row, false)
        }
        Some(action) => {
            let double = matches!(action, Action::SelectItem(_))
                && app.last_click.as_ref().is_some_and(|(last, tick)| {
                    *last == action && app.ticks.saturating_sub(*tick) <= DOUBLE_CLICK_TICKS
                });
            app.last_click = Some((action.clone(), app.ticks));
            let mut cmds = update::run(app, action);
            if double {
                app.last_click = None;
                cmds.extend(dashboard::activate(app));
            }
            cmds
        }
        None => {
            app.last_click = None;
            match app.hits.pane_at(mouse.column, mouse.row) {
                Some(pane) => update::run(app, Action::FocusPane(pane)),
                None => Vec::new(),
            }
        }
    }
}

/// A press on text: the click's own action runs (the cursor moves, a file opens, a pane takes
/// focus), then a selection begins where the pointer is.
fn press_text(
    app: &mut App,
    mouse: MouseEvent,
    hit: Option<Action>,
    (key, at): (crate::ui::textmap::RegionKey, super::selection::Pos),
) -> Vec<Cmd> {
    app.last_click = None;
    let mut cmds = match hit {
        Some(action) => update::run(app, action),
        None => match app.hits.pane_at(mouse.column, mouse.row) {
            Some(pane) => update::run(app, Action::FocusPane(pane)),
            None => Vec::new(),
        },
    };
    super::selection::begin(app, key, at);
    if app.screen == Screen::Diff {
        cmds.extend(super::selection::hint_once(app));
    }
    cmds
}

fn on_help(app: &mut App, mouse: MouseEvent, shift: bool) -> Vec<Cmd> {
    if shift {
        return Vec::new();
    }
    match mouse.kind {
        MouseEventKind::Down(_) => update::close_help(app),
        MouseEventKind::ScrollDown => scroll_help(app, true),
        MouseEventKind::ScrollUp => scroll_help(app, false),
        _ => {}
    }
    Vec::new()
}

/// The Show control answers its own rows; a click anywhere else closes it.
fn on_show(app: &mut App, mouse: MouseEvent, shift: bool) -> Vec<Cmd> {
    if shift {
        return Vec::new();
    }
    match mouse.kind {
        MouseEventKind::ScrollDown => {
            show::scroll(app, true);
            return Vec::new();
        }
        MouseEventKind::ScrollUp => {
            show::scroll(app, false);
            return Vec::new();
        }
        MouseEventKind::Down(MouseButton::Left) => {}
        _ => return Vec::new(),
    }
    match app.hits.at(mouse.column, mouse.row).cloned() {
        Some(
            action @ (Action::ToggleShow(_)
            | Action::ToggleProject(..)
            | Action::FocusProjectSearch
            | Action::DismissToast(_)),
        ) => update::run(app, action),
        _ => update::run(app, Action::CloseShow),
    }
}

/// The Pending reviews list answers its own rows and buttons; a click elsewhere closes it.
fn on_pending(app: &mut App, mouse: MouseEvent, shift: bool) -> Vec<Cmd> {
    if shift || !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Vec::new();
    }
    let confirming = app.pending.as_ref().is_some_and(|p| p.confirm.is_some());
    match app.hits.at(mouse.column, mouse.row).cloned() {
        Some(Action::Answer(yes)) if confirming => super::pending::answer(app, yes),
        Some(Action::PendingRow(n)) if !confirming => super::pending::click(app, n),
        Some(Action::DismissToast(id)) => update::run(app, Action::DismissToast(id)),
        _ if confirming => Vec::new(),
        _ => {
            super::pending::close(app);
            Vec::new()
        }
    }
}

fn scroll_help(app: &mut App, down: bool) {
    let max = help::max_scroll(app);
    app.help_scroll = if down {
        app.help_scroll.saturating_add(1).min(max)
    } else {
        app.help_scroll.saturating_sub(1)
    };
    app.mark_dirty();
}

/// A composer or confirmation holds input: only its own targets and toasts answer. A click
/// anywhere else is ignored, so nothing is discarded or sent by accident.
fn on_overlay(app: &mut App, mouse: MouseEvent, shift: bool) -> Vec<Cmd> {
    if shift {
        return Vec::new();
    }
    let confirming = app.diff.as_ref().is_some_and(|s| s.confirm.is_some());
    let reviewing = app.diff.as_ref().is_some_and(|s| s.review.is_some());
    let hit = app.hits.at(mouse.column, mouse.row).cloned();
    if reviewing {
        return on_review(app, mouse, hit);
    }
    match (mouse.kind, hit) {
        (MouseEventKind::Down(MouseButton::Left), Some(Action::Answer(yes))) if confirming => {
            composer::answer(app, yes)
        }
        (MouseEventKind::Down(MouseButton::Left), Some(Action::Choose(n))) if confirming => {
            composer::choose(app, n)
        }
        (MouseEventKind::Down(MouseButton::Left), Some(Action::DismissToast(id))) => {
            update::run(app, Action::DismissToast(id))
        }
        (
            MouseEventKind::Down(MouseButton::Left),
            Some(Action::ComposerCursor {
                x,
                y,
                first,
                across,
            }),
        ) if !confirming => {
            composer::place_cursor(app, mouse.column, mouse.row, (x, y, first, across));
            Vec::new()
        }
        (
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp,
            Some(Action::ComposerCursor { .. }),
        ) if !confirming => {
            composer::scroll_text(app, mouse.kind == MouseEventKind::ScrollDown);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// The review modal: its verdicts, buttons and summary answer; everything else is ignored.
fn on_review(app: &mut App, mouse: MouseEvent, hit: Option<Action>) -> Vec<Cmd> {
    match (mouse.kind, hit) {
        (MouseEventKind::Down(MouseButton::Left), Some(Action::ReviewVerdict(v))) => {
            super::review::set_verdict(app, v);
            Vec::new()
        }
        (MouseEventKind::Down(MouseButton::Left), Some(Action::ReviewButton(go))) => {
            super::review::arm(app, go);
            Vec::new()
        }
        (MouseEventKind::Up(MouseButton::Left), hit) => {
            let over = match hit {
                Some(Action::ReviewButton(go)) => Some(go),
                _ => None,
            };
            super::review::release(app, over)
        }
        (MouseEventKind::Down(MouseButton::Left), Some(Action::DismissToast(id))) => {
            update::run(app, Action::DismissToast(id))
        }
        (
            MouseEventKind::Down(MouseButton::Left),
            Some(Action::SummaryCursor {
                x,
                y,
                first,
                across,
            }),
        ) => {
            super::review::place_cursor(app, mouse.column, mouse.row, (x, y, first, across));
            Vec::new()
        }
        (
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp,
            Some(Action::SummaryCursor { .. }),
        ) => {
            super::review::scroll_text(app, mouse.kind == MouseEventKind::ScrollDown);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::layout::Rect;
    use rb_theme::ColourDepth;

    use super::*;
    use crate::app::{AppConfig, Msg};

    fn app() -> App {
        App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        })
    }

    fn event(kind: MouseEventKind, mods: KeyModifiers) -> Msg {
        Msg::Mouse(MouseEvent {
            kind,
            column: 1,
            row: 1,
            modifiers: mods,
        })
    }

    #[test]
    fn shift_events_never_run_actions_or_scroll() {
        let mut a = app();
        a.hits.push(Rect::new(0, 0, 5, 5), Action::Quit);
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::ScrollDown,
        ] {
            update::update(&mut a, event(kind, KeyModifiers::SHIFT));
        }
        assert!(!a.should_quit());
        assert!(!a.is_dirty() || a.dashboard.queue_scroll == 0);
    }

    #[test]
    fn a_click_on_a_target_remembers_itself_and_empty_space_forgets_it() {
        let mut a = app();
        a.hits.push(Rect::new(0, 0, 5, 5), Action::SelectItem(0));
        update::update(
            &mut a,
            event(MouseEventKind::Down(MouseButton::Left), KeyModifiers::NONE),
        );
        assert!(matches!(a.last_click, Some((Action::SelectItem(0), _))));
        let far = Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 100,
            row: 30,
            modifiers: KeyModifiers::NONE,
        });
        update::update(&mut a, far);
        assert!(a.last_click.is_none());
    }

    #[test]
    fn help_swallows_clicks_and_resets_its_scroll_when_closed() {
        let mut a = app();
        a.hits.push(Rect::new(0, 0, 5, 5), Action::Quit);
        a.help = true;
        a.help_scroll = 3;
        update::update(
            &mut a,
            event(MouseEventKind::Down(MouseButton::Left), KeyModifiers::NONE),
        );
        assert!(!a.help && a.help_scroll == 0 && !a.should_quit());
    }
}
