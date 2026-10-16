//! Flagging changes to come back to: `f` toggles, `F` jumps, the queue row shows `⚑`, and a
//! persisting list writes. Rendered headlessly from the frozen demo data.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use rb_theme::ColourDepth;
use review_buddy::app::flags::Flags;
use review_buddy::app::{update, App, AppConfig, Cmd, Msg};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::{self, HitMap};

fn new_app() -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let snapshot = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(world.snapshot())
        .unwrap();
    let mut app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (160, 40),
    });
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    app
}

fn press(app: &mut App, c: char) -> Vec<Cmd> {
    update(
        app,
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
    )
}

fn screen(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|frame| {
            let _: HitMap = ui::draw(frame, app);
        })
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
fn f_flags_the_selected_change_and_f_again_clears_it() {
    let mut app = new_app();
    let id = app.selected_change().unwrap().id.clone();
    assert!(!screen(&app).contains('⚑'));
    press(&mut app, 'f');
    assert!(app.flags.contains(&id));
    assert!(screen(&app).contains('⚑'));
    press(&mut app, 'f');
    assert!(!app.flags.contains(&id));
    assert!(!screen(&app).contains('⚑'));
}

#[test]
fn a_persisting_list_asks_for_a_write_and_memory_only_does_not() {
    let mut app = new_app();
    let cmds = press(&mut app, 'f');
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::SaveFlags { .. })));
    press(&mut app, 'f');
    app.flags = Flags::new(Some("/state/flagged.json".into()), Vec::new());
    let cmds = press(&mut app, 'f');
    assert!(cmds
        .iter()
        .any(|c| matches!(c, Cmd::SaveFlags { ids, .. } if ids.len() == 1)));
}

#[test]
fn capital_f_jumps_to_the_next_flagged_change_and_wraps() {
    let mut app = new_app();
    let first = app.selected_change().unwrap().id.clone();
    press(&mut app, 'j');
    press(&mut app, 'j');
    let later = app.selected_change().unwrap().id.clone();
    assert_ne!(first, later);
    press(&mut app, 'f');
    press(&mut app, 'g');
    assert_eq!(app.selected_change().unwrap().id, first);
    press(&mut app, 'F');
    assert_eq!(app.selected_change().unwrap().id, later);
    // With only one flagged change, jumping again lands on it again.
    press(&mut app, 'F');
    assert_eq!(app.selected_change().unwrap().id, later);
}

#[test]
fn f_with_nothing_flagged_says_what_to_do() {
    let mut app = new_app();
    press(&mut app, 'F');
    let text = &app.status.as_ref().unwrap().notice.text;
    assert!(text.contains("Nothing flagged"), "{text}");
}
