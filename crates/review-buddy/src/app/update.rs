use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    composer, dashboard, diff, links, live, mouse, Action, App, AppState, Cmd, Entry, Msg, Notice,
    NoticeKind, Screen, Snapshot, MAX_TOASTS, NOTICE_TTL,
};

/// Applies one message and returns the effects to run. Does no I/O.
pub fn update(app: &mut App, msg: Msg) -> Vec<Cmd> {
    let mut cmds = apply(app, msg);
    cmds.extend(live::ensure_info(app));
    cmds
}

fn apply(app: &mut App, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Key(key) => on_key(app, key),
        Msg::Mouse(mouse) => mouse::on_mouse(app, mouse),
        Msg::Resize(w, h) => {
            app.size = (w, h);
            dashboard::on_resize(app);
            diff::on_resize(app);
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
            if app.refresh_on_focus && app.state.loaded {
                return live::request_refresh(app, false);
            }
            Vec::new()
        }
        Msg::FocusLost => {
            app.focused = false;
            Vec::new()
        }
        Msg::Paste(text) => composer::on_paste(app, &text),
        Msg::Notify(notice) => push_toast(app, notice),
        Msg::Status(notice) => set_status(app, notice),
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
            take_snapshot(app, *snapshot);
            Vec::new()
        }
        Msg::Cached(snapshot) => {
            take_snapshot(app, *snapshot);
            live::request_refresh(app, false)
        }
        Msg::SourceLoaded {
            source,
            result,
            now,
        } => live::on_source_loaded(app, source, result, now),
        Msg::InfoLoaded { id, result } => live::on_info_loaded(app, id, result),
        Msg::DiffLoaded { id, result } => {
            diff::on_loaded(app, &id, result);
            Vec::new()
        }
        Msg::ReviewSubmitted {
            id,
            verdict,
            result,
            demo,
        } => composer::on_submitted(app, &id, verdict, result, demo),
        Msg::ReplyPosted {
            id,
            thread,
            result,
            demo,
        } => composer::on_replied(app, &id, &thread, result, demo),
    }
}

pub(super) fn close_help(app: &mut App) {
    app.help = false;
    app.help_scroll = 0;
    app.mark_dirty();
}

fn take_snapshot(app: &mut App, snapshot: Snapshot) {
    app.source_label = format!("{} · {} sources", snapshot.label, snapshot.sources.len());
    app.change_count = snapshot.changes.len();
    app.state = AppState {
        loaded: true,
        sources: snapshot.sources,
        changes: snapshot.changes,
        details: snapshot.details,
        now: Some(snapshot.now),
        ..AppState::default()
    };
    dashboard::reconcile(app);
    app.mark_dirty();
}

pub(super) fn scroll(app: &mut App, column: u16, row: u16, down: bool) -> Vec<Cmd> {
    match app.screen {
        Screen::Dashboard => dashboard::on_scroll(app, column, row, down),
        Screen::Diff => diff::on_scroll(app, column, row, down),
    }
    Vec::new()
}

fn on_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    if key.kind == KeyEventKind::Release {
        return Vec::new();
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let quits = match key.code {
        KeyCode::Char('c') => ctrl,
        KeyCode::Char('q') => !ctrl && app.screen == Screen::Dashboard && !app.help,
        _ => false,
    };
    if !quits {
        app.quit_armed = false;
    }
    if app.help {
        return match key.code {
            KeyCode::Esc | KeyCode::Char('?') => {
                close_help(app);
                Vec::new()
            }
            KeyCode::Char('c') if ctrl => run(app, Action::Quit),
            _ => Vec::new(),
        };
    }
    let overlay = app.screen == Screen::Diff && app.diff.as_ref().is_some_and(|s| s.has_overlay());
    if overlay && !quits {
        return composer::on_key(app, key);
    }
    match key.code {
        _ if quits => run(app, Action::Quit),
        KeyCode::Char('?') if !ctrl && !alt => run(app, Action::ToggleHelp),
        KeyCode::Char('o') if !ctrl && !alt => run(app, Action::Open),
        KeyCode::Char('y') if !ctrl && !alt => run(app, Action::Copy),
        KeyCode::Char('T') if !ctrl => run(app, Action::CycleTheme),
        KeyCode::Char('r') if !ctrl && app.screen == Screen::Dashboard => {
            live::request_refresh(app, true)
        }
        KeyCode::Esc if !app.toasts.is_empty() => {
            app.toasts.clear();
            app.mark_dirty();
            Vec::new()
        }
        _ => match app.screen {
            Screen::Dashboard => dashboard::on_key(app, key).unwrap_or_default(),
            Screen::Diff => diff::on_key(app, key),
        },
    }
}

pub(super) fn run(app: &mut App, action: Action) -> Vec<Cmd> {
    match action {
        Action::Quit => {
            if app.has_unsent_drafts() && !app.quit_armed {
                app.quit_armed = true;
                let text = "You have unsent review comments. Quit again to leave without sending, or press esc to keep reviewing.";
                return set_status(app, Notice::new(NoticeKind::Warning, text));
            }
            app.quit = true;
            Vec::new()
        }
        Action::ToggleHelp => {
            app.help = !app.help;
            app.help_scroll = 0;
            app.mark_dirty();
            Vec::new()
        }
        Action::Open => match links::current_url(app) {
            Some(url) => vec![Cmd::OpenUrl(url)],
            None => nothing_selected(app, "open"),
        },
        Action::Copy => match links::current_url(app) {
            Some(url) => vec![Cmd::Copy(url)],
            None => nothing_selected(app, "copy"),
        },
        Action::CycleTheme => {
            app.cycle_theme();
            diff::refresh_theme(app);
            let text = format!("Theme: {}", app.theme_name());
            set_status(app, Notice::new(NoticeKind::Info, text))
        }
        Action::DismissToast(id) => {
            dismiss_toast(app, id);
            Vec::new()
        }
        Action::CloseDiff | Action::DiffFile(_) | Action::DiffRow(_) | Action::DiffFocus(_) => {
            diff::on_action(app, action)
        }
        Action::ComposerCursor { .. } | Action::Answer(_) => Vec::new(),
        other => dashboard::on_action(app, other),
    }
}

fn nothing_selected(app: &mut App, verb: &str) -> Vec<Cmd> {
    let text = format!("Nothing to {verb} yet. Select a change first.");
    set_status(app, Notice::new(NoticeKind::Info, text))
}

pub(super) fn set_status(app: &mut App, notice: Notice) -> Vec<Cmd> {
    let id = app.take_id();
    app.status = Some(Entry { id, notice });
    app.mark_dirty();
    vec![expiry(Msg::StatusExpired(id))]
}

pub(super) fn push_toast(app: &mut App, notice: Notice) -> Vec<Cmd> {
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
    use crossterm::event::{KeyEventState, MouseButton, MouseEvent, MouseEventKind};
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
                details: Default::default(),
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

    #[test]
    fn o_and_y_without_a_selection_explain_themselves() {
        for code in ['o', 'y'] {
            let mut a = app();
            let cmds = update(&mut a, Msg::Key(KeyEvent::from(KeyCode::Char(code))));
            assert!(matches!(cmds.as_slice(), [Cmd::After { .. }]));
            let text = a.status.as_ref().unwrap().notice.text.clone();
            assert!(text.contains("Select a change first"), "{text}");
        }
    }

    #[test]
    fn help_toggles_and_esc_closes_it_before_anything_else() {
        let mut a = app();
        update(&mut a, Msg::Key(KeyEvent::from(KeyCode::Char('?'))));
        assert!(a.help);
        update(&mut a, Msg::Notify(Notice::new(NoticeKind::Info, "hi")));
        update(&mut a, Msg::Key(KeyEvent::from(KeyCode::Esc)));
        assert!(!a.help);
        assert_eq!(a.toasts.len(), 1, "the first Esc only closed the overlay");
    }

    #[test]
    fn a_click_closes_the_overlay_without_acting_on_what_is_beneath() {
        let mut a = app();
        a.hits.push(Rect::new(0, 5, 3, 1), Action::Quit);
        update(&mut a, Msg::Key(KeyEvent::from(KeyCode::Char('?'))));
        update(&mut a, click(1, 5));
        assert!(!a.help);
        assert!(!a.should_quit());
    }

    #[test]
    fn status_messages_from_effects_expire_like_any_other() {
        let mut a = app();
        let cmds = update(&mut a, Msg::Status(Notice::new(NoticeKind::Info, "Opened")));
        let id = a.status.as_ref().unwrap().id;
        assert!(matches!(
            cmds.as_slice(),
            [Cmd::After { msg: Msg::StatusExpired(i), .. }] if *i == id
        ));
    }
}
