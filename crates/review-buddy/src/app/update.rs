use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

use super::{Action, App, AppState, Cmd, Entry, Msg, Notice, NoticeKind, MAX_TOASTS, NOTICE_TTL};

/// Applies one message and returns the effects to run. Does no I/O.
pub fn update(app: &mut App, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Key(key) => on_key(app, key),
        Msg::Mouse(mouse) => match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => match app.hits.at(mouse.column, mouse.row) {
                Some(action) => {
                    let action = action.clone();
                    run(app, action)
                }
                None => Vec::new(),
            },
            _ => Vec::new(),
        },
        Msg::Resize(w, h) => {
            app.size = (w, h);
            app.mark_dirty();
            Vec::new()
        }
        Msg::Tick => {
            app.ticks = app.ticks.wrapping_add(1);
            Vec::new()
        }
        Msg::FocusGained => {
            app.focused = true;
            app.mark_dirty();
            Vec::new()
        }
        Msg::FocusLost => {
            app.focused = false;
            Vec::new()
        }
        Msg::Paste(_) => Vec::new(),
        Msg::Notify(notice) => push_toast(app, notice),
        Msg::StatusExpired(id) => {
            if app.status.as_ref().is_some_and(|e| e.id == id) {
                app.status = None;
                app.mark_dirty();
            }
            Vec::new()
        }
        Msg::ToastExpired(id) => {
            dismiss_toast(app, id);
            Vec::new()
        }
        Msg::Loaded(snapshot) => {
            let snapshot = *snapshot;
            app.source_label = format!("{} · {} sources", snapshot.label, snapshot.sources.len());
            app.change_count = snapshot.changes.len();
            app.state = AppState {
                loaded: true,
                sources: snapshot.sources,
                changes: snapshot.changes,
                now: Some(snapshot.now),
            };
            app.mark_dirty();
            Vec::new()
        }
    }
}

fn on_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    if key.kind == KeyEventKind::Release {
        return Vec::new();
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => run(app, Action::Quit),
        KeyCode::Char('q') if !ctrl => run(app, Action::Quit),
        KeyCode::Char('T') if !ctrl => run(app, Action::CycleTheme),
        KeyCode::Esc => {
            if app.toasts.is_empty() {
                Vec::new()
            } else {
                app.toasts.clear();
                app.mark_dirty();
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

fn run(app: &mut App, action: Action) -> Vec<Cmd> {
    match action {
        Action::Quit => {
            app.quit = true;
            Vec::new()
        }
        Action::CycleTheme => {
            app.cycle_theme();
            let text = format!("Theme: {}", app.theme_name());
            set_status(app, Notice::new(NoticeKind::Info, text))
        }
        Action::DismissToast(id) => {
            dismiss_toast(app, id);
            Vec::new()
        }
    }
}

fn set_status(app: &mut App, notice: Notice) -> Vec<Cmd> {
    let id = app.take_id();
    app.status = Some(Entry { id, notice });
    app.mark_dirty();
    vec![expiry(Msg::StatusExpired(id))]
}

fn push_toast(app: &mut App, notice: Notice) -> Vec<Cmd> {
    let id = app.take_id();
    app.toasts.push(Entry { id, notice });
    if app.toasts.len() > MAX_TOASTS {
        app.toasts.remove(0);
    }
    app.mark_dirty();
    vec![expiry(Msg::ToastExpired(id))]
}

fn dismiss_toast(app: &mut App, id: u64) {
    let before = app.toasts.len();
    app.toasts.retain(|e| e.id != id);
    if app.toasts.len() != before {
        app.mark_dirty();
    }
}

fn expiry(msg: Msg) -> Cmd {
    Cmd::After {
        delay: NOTICE_TTL,
        msg,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEventState, MouseEvent};
    use ratatui::layout::Rect;

    use super::*;
    use crate::app::{AppConfig, Snapshot};
    use rb_theme::ColourDepth;

    fn app() -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        app.clear_dirty();
        app
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Msg {
        Msg::Key(KeyEvent::new(code, modifiers))
    }

    fn click(col: u16, row: u16) -> Msg {
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn loaded_snapshot_fills_state_and_top_bar_info() {
        let mut a = app();
        let cmds = update(
            &mut a,
            Msg::Loaded(Box::new(Snapshot {
                label: "demo".into(),
                sources: Vec::new(),
                changes: Vec::new(),
                now: rb_core::Timestamp(5),
            })),
        );
        assert!(cmds.is_empty());
        assert!(a.state.loaded);
        assert_eq!(a.state.now, Some(rb_core::Timestamp(5)));
        assert_eq!(a.source_label, "demo · 0 sources");
        assert!(a.is_dirty());
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let mut a = app();
        update(&mut a, key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(a.should_quit());
        let mut a = app();
        update(&mut a, key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(a.should_quit());
    }

    #[test]
    fn ctrl_q_and_other_keys_do_not_quit() {
        let mut a = app();
        update(&mut a, key(KeyCode::Char('q'), KeyModifiers::CONTROL));
        update(&mut a, key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!a.should_quit());
        assert!(!a.is_dirty());
    }

    #[test]
    fn key_release_is_ignored() {
        let mut a = app();
        let release = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        update(&mut a, Msg::Key(release));
        assert!(!a.should_quit());
    }

    #[test]
    fn t_cycles_themes_and_sets_status_with_expiry() {
        let mut a = app();
        assert_eq!(a.theme_name(), "Liminal HQ");
        let cmds = update(&mut a, key(KeyCode::Char('T'), KeyModifiers::SHIFT));
        assert_ne!(a.theme_name(), "Liminal HQ");
        assert!(a.is_dirty());
        let status = a.status.clone().expect("status set");
        assert!(status.notice.text.starts_with("Theme: "));
        assert!(matches!(
            cmds.as_slice(),
            [Cmd::After { delay, msg: Msg::StatusExpired(id) }] if *delay == NOTICE_TTL && *id == status.id
        ));
    }

    #[test]
    fn theme_cycle_wraps_after_all_builtins() {
        let mut a = app();
        let first = a.theme_name().to_string();
        for _ in 0..rb_theme::BUILTIN_IDS.len() {
            update(&mut a, Msg::Key(KeyEvent::from(KeyCode::Char('T'))));
        }
        assert_eq!(a.theme_name(), first);
    }

    #[test]
    fn status_expiry_only_clears_matching_id() {
        let mut a = app();
        update(&mut a, Msg::Key(KeyEvent::from(KeyCode::Char('T'))));
        let id = a.status.as_ref().unwrap().id;
        a.clear_dirty();
        update(&mut a, Msg::StatusExpired(id + 100));
        assert!(a.status.is_some());
        assert!(!a.is_dirty());
        update(&mut a, Msg::StatusExpired(id));
        assert!(a.status.is_none());
        assert!(a.is_dirty());
    }

    #[test]
    fn toasts_stack_cap_and_expire() {
        let mut a = app();
        let mut ids = Vec::new();
        for i in 0..5 {
            let cmds = update(
                &mut a,
                Msg::Notify(Notice::new(NoticeKind::Success, format!("n{i}"))),
            );
            if let [Cmd::After {
                msg: Msg::ToastExpired(id),
                ..
            }] = cmds.as_slice()
            {
                ids.push(*id);
            }
        }
        assert_eq!(a.toasts.len(), MAX_TOASTS);
        assert_eq!(a.toasts[0].notice.text, "n2");
        update(&mut a, Msg::ToastExpired(ids[4]));
        assert_eq!(a.toasts.len(), MAX_TOASTS - 1);
        update(&mut a, Msg::ToastExpired(ids[0]));
        assert_eq!(a.toasts.len(), MAX_TOASTS - 1);
    }

    #[test]
    fn esc_clears_toasts() {
        let mut a = app();
        update(&mut a, Msg::Notify(Notice::new(NoticeKind::Info, "hi")));
        update(&mut a, Msg::Key(KeyEvent::from(KeyCode::Esc)));
        assert!(a.toasts.is_empty());
    }

    #[test]
    fn resize_updates_size_and_marks_dirty() {
        let mut a = app();
        update(&mut a, Msg::Resize(90, 20));
        assert_eq!(a.size, (90, 20));
        assert!(a.is_dirty());
    }

    #[test]
    fn tick_counts_without_redrawing() {
        let mut a = app();
        update(&mut a, Msg::Tick);
        assert_eq!(a.ticks(), 1);
        assert!(!a.is_dirty());
    }

    #[test]
    fn focus_events_track_focus() {
        let mut a = app();
        update(&mut a, Msg::FocusLost);
        assert!(!a.focused);
        update(&mut a, Msg::FocusGained);
        assert!(a.focused);
    }

    #[test]
    fn clicking_a_registered_target_runs_its_action() {
        let mut a = app();
        a.hits.push(Rect::new(10, 0, 5, 1), Action::CycleTheme);
        update(&mut a, click(12, 0));
        assert_eq!(
            a.status.as_ref().map(|e| e.notice.kind),
            Some(NoticeKind::Info)
        );
        a.hits.push(Rect::new(0, 5, 3, 1), Action::Quit);
        update(&mut a, click(1, 5));
        assert!(a.should_quit());
    }

    #[test]
    fn clicking_empty_space_or_other_buttons_does_nothing() {
        let mut a = app();
        a.hits.push(Rect::new(0, 0, 3, 1), Action::Quit);
        update(&mut a, click(50, 20));
        let right = Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 1,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        update(&mut a, right);
        assert!(!a.should_quit());
    }

    #[test]
    fn clicking_a_toast_dismisses_it() {
        let mut a = app();
        update(
            &mut a,
            Msg::Notify(Notice::new(NoticeKind::Warning, "slow")),
        );
        let id = a.toasts[0].id;
        a.hits
            .push(Rect::new(0, 0, 10, 3), Action::DismissToast(id));
        update(&mut a, click(2, 1));
        assert!(a.toasts.is_empty());
    }
}
