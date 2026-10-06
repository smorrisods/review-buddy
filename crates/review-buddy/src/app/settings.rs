//! Settings → Sources as pure transitions over [`crate::settings::State`].

use crossterm::event::{KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};

use super::update::{push_toast, run};
use super::{Action, App, Cmd, Screen};
use crate::settings::{Effect, Input, Out, State};

/// Opens Settings. Demo mode shows the demo sources and reads nothing; otherwise the config is
/// read by an effect, so the screen opens on "reading your config".
pub fn open(app: &mut App) -> Vec<Cmd> {
    app.screen = Screen::Settings;
    app.mark_dirty();
    if app.demo {
        app.settings = Some(State::demo(&app.state.sources));
        return Vec::new();
    }
    app.settings = Some(State::loading());
    vec![Cmd::Settings(Effect::Load)]
}

pub(super) fn close(app: &mut App) {
    app.settings = None;
    app.screen = Screen::Dashboard;
    app.mark_dirty();
}

/// Applies one input and carries out what it asks of the app around the state.
pub(super) fn drive(app: &mut App, input: Input) -> Vec<Cmd> {
    let Some(state) = app.settings.as_mut() else {
        return Vec::new();
    };
    let out = state.apply(input);
    app.mark_dirty();
    finish(app, out)
}

fn finish(app: &mut App, out: Out) -> Vec<Cmd> {
    let mut cmds: Vec<Cmd> = out.effects.into_iter().map(Cmd::Settings).collect();
    if out.back {
        close(app);
    }
    if out.reload {
        app.state.loading = true;
        cmds.push(Cmd::FinishSetup);
    }
    if let Some(notice) = out.notice {
        cmds.extend(push_toast(app, notice));
    }
    cmds
}

/// `None` means the key isn't Settings': help and the theme key use the app's own handling.
pub(super) fn on_key(app: &mut App, key: KeyEvent) -> Option<Vec<Cmd>> {
    if key.kind == KeyEventKind::Release {
        return Some(Vec::new());
    }
    let state = app.settings.as_mut()?;
    let out = state.key(key);
    if out.pass {
        return None;
    }
    app.mark_dirty();
    Some(finish(app, out))
}

pub(super) fn on_paste(app: &mut App, text: String) -> Vec<Cmd> {
    drive(app, Input::Paste(text))
}

/// Clicks resolve against the hit map; the wheel moves the selection. With a form or confirm
/// open, only its own targets answer.
pub(super) fn on_mouse(app: &mut App, mouse: MouseEvent, shift: bool) -> Vec<Cmd> {
    if shift {
        return Vec::new();
    }
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            match app.hits.at(mouse.column, mouse.row).cloned() {
                Some(Action::Settings(click)) => drive(app, Input::Click(click)),
                Some(Action::DismissToast(id)) => run(app, Action::DismissToast(id)),
                _ => Vec::new(),
            }
        }
        MouseEventKind::ScrollDown => drive(app, Input::Scroll { down: true }),
        MouseEventKind::ScrollUp => drive(app, Input::Scroll { down: false }),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};
    use rb_theme::ColourDepth;

    use super::*;
    use crate::app::{update, AppConfig, Msg, NoticeKind};
    use crate::settings::{Saved, Snapshot};

    fn app() -> App {
        App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        })
    }

    fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
        update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            rows: Vec::new(),
            write_target: "/c/config.toml".into(),
            origin: None,
            editable: true,
        }
    }

    #[test]
    fn the_comma_key_opens_settings_and_asks_for_the_config() {
        let mut a = app();
        let cmds = press(&mut a, KeyCode::Char(','));
        assert_eq!(a.screen, Screen::Settings);
        assert!(matches!(cmds[..], [Cmd::Settings(Effect::Load)]));
        assert!(!a.should_quit());
    }

    #[test]
    fn q_does_not_quit_from_settings_and_escape_goes_back() {
        let mut a = app();
        press(&mut a, KeyCode::Char(','));
        press(&mut a, KeyCode::Char('q'));
        assert!(!a.should_quit());
        press(&mut a, KeyCode::Esc);
        assert_eq!(a.screen, Screen::Dashboard);
        assert!(a.settings.is_none());
    }

    #[test]
    fn help_and_theme_keys_still_work_on_the_screen() {
        let mut a = app();
        press(&mut a, KeyCode::Char(','));
        press(&mut a, KeyCode::Char('?'));
        assert!(a.help);
        press(&mut a, KeyCode::Esc);
        assert!(!a.help && a.screen == Screen::Settings);
        press(&mut a, KeyCode::Char('T'));
        assert_eq!(a.theme_name(), "Dusk");
    }

    #[test]
    fn demo_opens_read_only_without_asking_for_anything() {
        let mut a = app();
        a.demo = true;
        let cmds = press(&mut a, KeyCode::Char(','));
        assert!(cmds.is_empty());
        let state = a.settings.as_ref().unwrap();
        assert!(state.demo && state.loaded);
        let cmds = press(&mut a, KeyCode::Char('a'));
        assert!(cmds.iter().all(|c| !matches!(c, Cmd::Settings(_))));
        assert!(a.toasts[0].notice.text.contains("(demo)"));
    }

    #[test]
    fn a_saved_change_asks_the_runtime_to_reload_the_queue() {
        let mut a = app();
        press(&mut a, KeyCode::Char(','));
        update(&mut a, Msg::Settings(Input::Loaded(Ok(snapshot()))));
        let cmds = update(
            &mut a,
            Msg::Settings(Input::Saved(Ok(Saved {
                message: "Added x.".into(),
                select: None,
            }))),
        );
        assert!(cmds.iter().any(|c| matches!(c, Cmd::FinishSetup)));
        assert!(cmds
            .iter()
            .any(|c| matches!(c, Cmd::Settings(Effect::Load))));
        assert_eq!(a.toasts[0].notice.kind, NoticeKind::Success);
        assert!(a.state.loading);
    }

    #[test]
    fn an_empty_config_offers_to_add_and_a_failed_read_is_a_calm_toast() {
        let mut a = app();
        press(&mut a, KeyCode::Char(','));
        update(
            &mut a,
            Msg::Settings(Input::Loaded(Err("config.toml:3: bad".into()))),
        );
        assert!(a.toasts[0].notice.text.contains("config.toml:3"));
        let cmds = press(&mut a, KeyCode::Char('a'));
        assert!(matches!(cmds[..], [Cmd::Settings(Effect::Detect)]));
    }

    #[test]
    fn typing_in_the_form_is_not_taken_for_global_keys() {
        let mut a = app();
        press(&mut a, KeyCode::Char(','));
        update(&mut a, Msg::Settings(Input::Loaded(Ok(snapshot()))));
        press(&mut a, KeyCode::Char('a'));
        press(&mut a, KeyCode::Enter);
        for c in "T?q".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        assert!(!a.help && !a.should_quit());
        assert_eq!(a.theme_name(), "Liminal HQ");
        update(&mut a, Msg::Paste("gitlab.work.ca".into()));
        assert!(a.settings.as_ref().unwrap().is_modal());
    }
}
