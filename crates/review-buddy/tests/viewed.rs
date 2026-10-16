//! Viewed marks on the diff screen, on the frozen demo data: `m` ticks a file and sends the mark
//! to a forge that keeps it (the demo's GitHub), a refusal puts it back, and a forge with
//! nowhere to keep it (the demo's GitLab) keeps the mark on this machine for that commit.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use rb_core::ChangeId;
use rb_theme::ColourDepth;
use review_buddy::app::viewed::Local;
use review_buddy::app::{update, App, AppConfig, Cmd, Msg};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::{self, HitMap};

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

fn press(app: &mut App, c: char) -> Vec<Cmd> {
    update(
        app,
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
    )
}

fn screen(app: &App) -> String {
    let (w, h) = app.size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
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

fn app_with(local: Local) -> (App, DemoWorld) {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let mut app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (160, 40),
    });
    app.viewed = local;
    let snapshot = block_on(world.snapshot()).unwrap();
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    (app, world)
}

/// Opens the diff of change `number`. Returns its id.
fn open_diff(app: &mut App, world: &DemoWorld, number: u64) -> ChangeId {
    for _ in 0..40 {
        if app.selected_change().is_some_and(|c| c.id.number == number) {
            break;
        }
        press(app, 'j');
    }
    let id = app.selected_change().unwrap().id.clone();
    assert_eq!(id.number, number);
    update(app, Msg::Key(KeyEvent::from(KeyCode::Enter)));
    let data = block_on(world.diff_data(&id)).unwrap();
    update(
        app,
        Msg::DiffLoaded {
            id: id.clone(),
            result: Ok(Box::new(data)),
        },
    );
    id
}

fn viewed_count(app: &App) -> usize {
    app.diff
        .as_ref()
        .unwrap()
        .data
        .as_ref()
        .unwrap()
        .viewed
        .len()
}

#[test]
fn m_ticks_a_file_on_a_forge_that_keeps_marks_and_sends_it() {
    let (mut app, world) = app_with(Local::default());
    let id = open_diff(&mut app, &world, 209);
    assert!(screen(&app).contains("0 of 2 files viewed"));
    let cmds = press(&mut app, 'm');
    let [Cmd::SetFileViewed {
        id: sent,
        path,
        viewed,
    }, ..] = cmds.as_slice()
    else {
        panic!("{cmds:?}");
    };
    assert_eq!(
        (sent, path.as_str(), *viewed),
        (&id, "crates/rb-theme/src/palette.rs", true)
    );
    let text = screen(&app);
    assert!(
        text.contains("1 of 2 files viewed") && text.contains('✓'),
        "{text}"
    );
    let cmds = press(&mut app, 'm');
    assert!(matches!(&cmds[0], Cmd::SetFileViewed { viewed: false, .. }));
    assert_eq!(viewed_count(&app), 0);
}

#[test]
fn a_refused_mark_puts_the_tick_back_and_says_so() {
    let (mut app, world) = app_with(Local::default());
    let id = open_diff(&mut app, &world, 209);
    press(&mut app, 'm');
    update(
        &mut app,
        Msg::FileViewedSet {
            id,
            path: "crates/rb-theme/src/palette.rs".into(),
            viewed: true,
            result: Err("GitHub said no".into()),
        },
    );
    assert_eq!(viewed_count(&app), 0);
    let note = &app.status.as_ref().unwrap().notice.text;
    assert!(
        note.contains("back as it was") && note.contains("GitHub said no"),
        "{note}"
    );
}

#[test]
fn the_demo_forge_stores_the_mark_and_a_reopened_diff_shows_it() {
    let (mut app, world) = app_with(Local::default());
    let id = open_diff(&mut app, &world, 209);
    press(&mut app, 'm');
    let data = block_on(world.diff_data(&id)).unwrap();
    assert!(
        data.viewed.is_empty(),
        "nothing is stored until the command runs"
    );
    block_on(async {
        use rb_core::Provider as _;
        world
            .provider(id.kind)
            .set_file_viewed(&id, "crates/rb-theme/src/palette.rs", true)
            .await
            .unwrap();
    });
    let data = block_on(world.diff_data(&id)).unwrap();
    assert_eq!(data.viewed.len(), 1);
}

#[test]
fn a_forge_without_marks_keeps_them_here_for_that_commit_only() {
    let dir = std::env::temp_dir().join("rb-viewed-test-never-written.json");
    let (mut app, world) = app_with(Local::new(Some(dir.clone()), Vec::new()));
    let id = open_diff(&mut app, &world, 12);
    let cmds = press(&mut app, 'm');
    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::SetFileViewed { .. })),
        "nothing is sent to a forge that can't keep it: {cmds:?}"
    );
    let saved = cmds
        .iter()
        .find_map(|c| match c {
            Cmd::SaveViewed { path, entries } => Some((path.clone(), entries.clone())),
            _ => None,
        })
        .expect("a write was asked for");
    assert_eq!(saved.0, dir);
    assert_eq!(saved.1.len(), 1);
    let head = app.selected_change_head(&id);
    assert_eq!(saved.1[0].head_sha, head);
    let note = &app.status.as_ref().unwrap().notice.text;
    assert!(note.contains("on this machine"), "{note}");

    // Back to the dashboard and into the diff again: the mark is still there.
    press(&mut app, 'q');
    let data = block_on(world.diff_data(&id)).unwrap();
    update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Enter)));
    update(
        &mut app,
        Msg::DiffLoaded {
            id: id.clone(),
            result: Ok(Box::new(data)),
        },
    );
    assert_eq!(viewed_count(&app), 1);
}

#[test]
fn a_mark_made_on_an_older_commit_is_ignored() {
    let (mut probe, world) = app_with(Local::default());
    let id = open_diff(&mut probe, &world, 12);
    let stale = review_buddy::viewed::Stored {
        id: id.clone(),
        head_sha: "not-the-current-head".into(),
        paths: ["whatever.rs".to_string()].into(),
    };
    let (mut app, world) = app_with(Local::new(None, vec![stale]));
    open_diff(&mut app, &world, 12);
    assert_eq!(viewed_count(&app), 0);
}
