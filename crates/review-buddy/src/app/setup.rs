//! First-run input as pure transitions over [`crate::setup::Flow`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::update::set_status;
use super::{App, Cmd, Notice, NoticeKind, Screen};
use crate::setup::{Flow, Input, Outcome};

/// Opens first run on `flow` and returns the effects that start it.
pub fn start(app: &mut App, flow: Flow) -> Vec<Cmd> {
    app.set_theme(flow.theme_id());
    let effects = flow.start();
    app.setup = Some(flow);
    app.screen = Screen::FirstRun;
    app.source_label = "first run".to_string();
    app.mark_dirty();
    effects.into_iter().map(Cmd::Setup).collect()
}

pub(super) fn on_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
        return Vec::new();
    }
    let token_open = app.setup.as_ref().is_some_and(|f| f.token_open);
    let input = match key.code {
        KeyCode::Esc => Input::Skip,
        KeyCode::Enter => Input::Next,
        KeyCode::Backspace if token_open => Input::Backspace,
        KeyCode::Backspace => Input::Back,
        KeyCode::Up => Input::Up,
        KeyCode::Down => Input::Down,
        KeyCode::Left => Input::Left,
        KeyCode::Right => Input::Right,
        KeyCode::Char(c) => Input::Char(c),
        _ => return Vec::new(),
    };
    drive(app, input)
}

pub(super) fn on_paste(app: &mut App, text: String) -> Vec<Cmd> {
    drive(app, Input::Paste(text))
}

/// Applies one input to the flow, follows the theme preview, and handles the ending.
pub(super) fn drive(app: &mut App, input: Input) -> Vec<Cmd> {
    let Some(flow) = app.setup.as_mut() else {
        return Vec::new();
    };
    let effects = flow.apply(input);
    let theme = flow.theme_id();
    let outcome = flow.outcome.clone();
    let mut cmds: Vec<Cmd> = effects.into_iter().map(Cmd::Setup).collect();
    app.set_theme(theme);
    app.mark_dirty();
    match outcome {
        Some(Outcome::Skipped) => {
            app.setup = None;
            app.screen = Screen::Dashboard;
            app.source_label = "no sources".to_string();
            let text = "Skipped for now. Run review-buddy --setup when you're ready.";
            cmds.extend(set_status(app, Notice::new(NoticeKind::Info, text)));
        }
        Some(Outcome::Written { .. }) => {
            app.setup = None;
            app.screen = Screen::Dashboard;
            app.state.loading = true;
            cmds.push(Cmd::FinishSetup);
        }
        None => {}
    }
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{update, AppConfig, Msg};
    use crate::setup::{Conn, Detection, Effect, Step};
    use rb_theme::ColourDepth;

    fn app() -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        let flow = Flow::new("/c/config.toml".into(), false, "liminal-hq", true);
        let cmds = start(&mut app, flow);
        assert!(matches!(cmds[..], [Cmd::Setup(Effect::Detect)]));
        app
    }

    fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
        update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    #[test]
    fn it_opens_on_the_first_run_screen_and_q_does_not_quit() {
        let mut a = app();
        assert_eq!(a.screen, Screen::FirstRun);
        press(&mut a, KeyCode::Char('q'));
        assert!(!a.should_quit());
        update(
            &mut a,
            Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        );
        assert!(a.should_quit());
    }

    #[test]
    fn enter_moves_on_and_detection_results_arrive_as_messages() {
        let mut a = app();
        update(&mut a, Msg::Setup(Input::Detected(Detection::default())));
        press(&mut a, KeyCode::Enter);
        assert_eq!(a.setup.as_ref().unwrap().step, Step::Connect);
        assert!(a
            .setup
            .as_ref()
            .unwrap()
            .hosts
            .iter()
            .all(|h| h.conn == Conn::NeedsToken));
    }

    #[test]
    fn the_look_step_previews_the_theme_live() {
        let mut a = app();
        a.setup.as_mut().unwrap().step = Step::Look;
        press(&mut a, KeyCode::Right);
        assert_eq!(a.theme_name(), "Dusk");
    }

    #[test]
    fn escape_skips_to_the_dashboard_with_a_hint() {
        let mut a = app();
        press(&mut a, KeyCode::Esc);
        assert_eq!(a.screen, Screen::Dashboard);
        assert!(a.setup.is_none());
        assert!(a.status.as_ref().unwrap().notice.text.contains("--setup"));
    }

    #[test]
    fn a_written_config_asks_the_runtime_to_load_the_queue() {
        let mut a = app();
        let cmds = update(
            &mut a,
            Msg::Setup(Input::Written(Ok(crate::setup::flow::Written {
                path: "/c/config.toml".into(),
                backup: None,
            }))),
        );
        assert!(cmds.iter().any(|c| matches!(c, Cmd::FinishSetup)));
        assert_eq!(a.screen, Screen::Dashboard);
    }
}
