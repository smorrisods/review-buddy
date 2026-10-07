//! Keyboard line ranges and file flipping in the diff, driven through `update` against headless
//! renders of the frozen demo data.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_theme::ColourDepth;
use review_buddy::app::{
    update, Cmd, Msg, Screen, {App, AppConfig, Snapshot},
};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::{self, HitMap};

fn world() -> DemoWorld {
    DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap()
}

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

fn snapshot() -> Snapshot {
    block_on(world().snapshot()).unwrap()
}

fn dashboard(theme: &str, width: u16, height: u16) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (width, height),
    });
    update(&mut app, Msg::Loaded(Box::new(snapshot())));
    app
}

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn diff(theme: &str, width: u16, height: u16) -> App {
    let mut app = dashboard(theme, width, height);
    let id = app.selected_change().unwrap().id.clone();
    let data = block_on(world().diff_data(&id)).unwrap();
    let Some(Cmd::LoadDiff(id)) = press(&mut app, KeyCode::Enter).into_iter().next() else {
        panic!("enter opens the diff");
    };
    update(
        &mut app,
        Msg::DiffLoaded {
            id,
            result: Ok(Box::new(data)),
        },
    );
    assert_eq!(app.screen, Screen::Diff);
    app
}

fn render(app: &mut App) -> Buffer {
    let (w, h) = app.size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|frame| hits = ui::draw(frame, app)).unwrap();
    app.hits = hits;
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    let width = usize::from(buffer.area.width);
    buffer
        .content()
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tap(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    update(app, Msg::Key(KeyEvent::new(code, modifiers)));
}

fn range_app(width: u16, height: u16) -> App {
    let mut a = diff("liminal-hq", width, height);
    render(&mut a);
    tap(&mut a, KeyCode::Down, KeyModifiers::SHIFT);
    tap(&mut a, KeyCode::Down, KeyModifiers::SHIFT);
    tap(&mut a, KeyCode::Down, KeyModifiers::SHIFT);
    assert!(a.diff_state().unwrap().range.is_some());
    a
}

#[test]
fn keyboard_range_160x40_default_theme() {
    let mut a = range_app(160, 40);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn keyboard_range_100x30_default_theme() {
    let mut a = range_app(100, 30);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn the_footer_counts_the_selected_lines() {
    let mut a = range_app(160, 40);
    assert!(text(&render(&mut a)).contains("4 lines selected"));
    tap(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!text(&render(&mut a)).contains("lines selected"));
}

#[test]
fn the_footer_and_help_mention_the_arrows() {
    let mut a = diff("liminal-hq", 160, 40);
    assert!(text(&render(&mut a)).contains("← → file"));
    update(
        &mut a,
        Msg::Key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)),
    );
    let shown = text(&render(&mut a));
    assert!(shown.contains("select lines"), "{shown}");
}
