//! The remembered layout as the app drives it: which keys mark it dirty, the debounce
//! through `update`, the write on quit, and the toast and help row.
#![cfg(feature = "demo")]

use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Cmd, Msg};
use review_buddy::config::{DetailMode, DetailPosition, SourcesLayout};
use review_buddy::session::{Session, Tracker};

#[path = "support/render.rs"]
mod render_support;
use render_support::{render, text};

fn app() -> App {
    let mut app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (160, 40),
    });
    app.session = Some(Tracker::new(
        PathBuf::from("session.toml"),
        Session::default(),
        app.session_snapshot(),
    ));
    app
}

fn press(app: &mut App, c: char) -> Vec<Cmd> {
    update(
        app,
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
    )
}

fn schedules(cmds: &[Cmd]) -> usize {
    cmds.iter()
        .filter(|c| {
            matches!(
                c,
                Cmd::After {
                    msg: Msg::SaveSessionDue,
                    delay
                } if *delay == Duration::from_millis(500)
            )
        })
        .count()
}

fn saves(cmds: &[Cmd]) -> Vec<Session> {
    cmds.iter()
        .filter_map(|c| match c {
            Cmd::SaveSession { session, .. } => Some(*session),
            _ => None,
        })
        .collect()
}

#[test]
fn the_layout_keys_schedule_one_debounced_write_with_the_latest_values() {
    let mut a = app();
    assert_eq!(schedules(&press(&mut a, 'P')), 1);
    assert_eq!(schedules(&press(&mut a, 'P')), 0, "already scheduled");
    assert_eq!(schedules(&press(&mut a, 'S')), 0);
    assert_eq!(schedules(&press(&mut a, 'p')), 0);
    let written = saves(&update(&mut a, Msg::SaveSessionDue));
    assert_eq!(written.len(), 1);
    let l = written[0].layout;
    assert_eq!(l.detail_position, Some(DetailPosition::Left));
    assert_eq!(l.sources, Some(SourcesLayout::Left));
    assert_eq!(l.detail, Some(DetailMode::Closed));
    assert!(saves(&update(&mut a, Msg::SaveSessionDue)).is_empty());
    assert_eq!(
        schedules(&press(&mut a, 'p')),
        1,
        "the next change schedules again"
    );
}

#[test]
fn b_is_remembered_too() {
    let mut a = app();
    assert_eq!(schedules(&press(&mut a, 'B')), 1);
    let written = saves(&update(&mut a, Msg::SaveSessionDue));
    assert!(written[0].layout.background.is_some());
}

#[test]
fn keys_that_change_no_layout_write_nothing() {
    let mut a = app();
    for c in ['j', 'k', 'J', 's', '?', '?'] {
        let cmds = press(&mut a, c);
        assert_eq!(schedules(&cmds), 0, "{c}");
        assert!(saves(&cmds).is_empty(), "{c}");
    }
}

#[test]
fn quitting_writes_at_once_when_a_change_is_waiting_and_not_otherwise() {
    let mut a = app();
    press(&mut a, 'P');
    let cmds = press(&mut a, 'q');
    assert_eq!(saves(&cmds).len(), 1);
    assert!(saves(&update(&mut a, Msg::SaveSessionDue)).is_empty());

    let mut quiet = app();
    assert!(saves(&press(&mut quiet, 'q')).is_empty());
}

#[test]
fn without_a_tracker_nothing_is_scheduled_or_written() {
    let mut a = app();
    a.session = None;
    for c in ['P', 'S', 'p', 'B'] {
        let cmds = press(&mut a, c);
        assert_eq!(schedules(&cmds), 0);
    }
    assert!(update(&mut a, Msg::SaveSessionDue).is_empty());
    assert!(saves(&press(&mut a, 'q')).is_empty());
}

#[test]
fn the_toast_describes_the_arrangement() {
    let mut a = app();
    press(&mut a, 'P');
    let toast: Vec<String> = text(&render(&mut a))
        .lines()
        .filter(|l| l.contains("Layout: "))
        .map(|l| l.trim().to_string())
        .collect();
    insta::assert_snapshot!(toast.join("\n"));
}

#[test]
fn the_help_row_names_the_cycle() {
    let mut a = app();
    press(&mut a, '?');
    let row: Vec<String> = text(&render(&mut a))
        .lines()
        .filter(|l| l.contains("rotate panes"))
        .map(|l| l.trim().to_string())
        .collect();
    insta::assert_snapshot!(row.join("\n"));
}
