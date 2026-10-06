//! The Show filters control and a filtered queue, rendered headlessly from the frozen demo data.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Msg, Snapshot};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::{self, HitMap};

fn snapshot() -> Snapshot {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(world.snapshot())
        .unwrap()
}

fn app(theme: &str, width: u16, height: u16, loaded: bool) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (width, height),
    });
    if loaded {
        update(&mut app, Msg::Loaded(Box::new(snapshot())));
    }
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

fn find(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let width = buffer.area.width;
    (0..buffer.area.height).find_map(|y| {
        let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        line.find(needle)
            .map(|byte| (line[..byte].chars().count() as u16, y))
    })
}

fn press(app: &mut App, code: KeyCode) {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn click(app: &mut App, x: u16, y: u16) {
    update(
        app,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
    );
}

fn open_control(a: &mut App) {
    press(a, KeyCode::Char('s'));
}

fn narrowed(a: &mut App) {
    for key in ['3', '4'] {
        open_control(a);
        press(a, KeyCode::Char(key));
        press(a, KeyCode::Esc);
    }
}

#[test]
fn show_control_160x40_default_theme() {
    let mut a = app("liminal-hq", 160, 40, true);
    open_control(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn show_control_160x40_dusk() {
    let mut a = app("dusk", 160, 40, true);
    open_control(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn show_control_100x30_default_theme() {
    let mut a = app("liminal-hq", 100, 30, true);
    open_control(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn show_control_100x30_dusk() {
    let mut a = app("dusk", 100, 30, true);
    open_control(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn filtered_queue_160x40_default_theme() {
    let mut a = app("liminal-hq", 160, 40, true);
    narrowed(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn filtered_queue_160x40_dusk() {
    let mut a = app("dusk", 160, 40, true);
    narrowed(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn filtered_queue_100x30_default_theme() {
    let mut a = app("liminal-hq", 100, 30, true);
    narrowed(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn filtered_queue_100x30_dusk() {
    let mut a = app("dusk", 100, 30, true);
    narrowed(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn toggling_drafts_and_noise_changes_the_counts_at_once() {
    let mut a = app("liminal-hq", 160, 40, true);
    let before = text(&render(&mut a));
    assert!(before.contains("Noise ·") && before.contains("7 in this view."));
    assert!(!before.contains("hidden by your Show filters"));
    open_control(&mut a);
    press(&mut a, KeyCode::Char('4'));
    press(&mut a, KeyCode::Char('5'));
    let after = text(&render(&mut a));
    assert!(after.contains("[x] drafts") && after.contains("[x] noise"));
    assert!(
        !after.contains("Noise ·"),
        "noise is mixed into the buckets"
    );
    press(&mut a, KeyCode::Char('3'));
    let narrower = text(&render(&mut a));
    assert!(narrower.contains("1 hidden by your Show filters."));
    press(&mut a, KeyCode::Esc);
    let closed = text(&render(&mut a));
    assert!(!closed.contains("this session only"));
    assert!(closed.contains("1 hidden by your Show filters"));
}

#[test]
fn the_end_note_opens_the_control_and_checkboxes_click() {
    let mut a = app("liminal-hq", 160, 40, true);
    narrowed(&mut a);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "hidden by your Show filters").unwrap();
    click(&mut a, x, y);
    let buffer = render(&mut a);
    assert!(text(&buffer).contains("this session only"));
    let (x, y) = find(&buffer, "changes you opened").unwrap();
    click(&mut a, x, y);
    let buffer = render(&mut a);
    assert!(text(&buffer).contains("Nothing is hidden."));
    click(&mut a, 0, 0);
    assert!(!text(&render(&mut a)).contains("this session only"));
}

#[test]
fn the_sources_pane_checkboxes_click_and_counts_follow_the_filters() {
    let mut a = app("liminal-hq", 160, 40, true);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "[x] authored").unwrap();
    click(&mut a, x, y);
    let t = text(&render(&mut a));
    assert!(t.contains("[ ] authored"));
    assert!(t.contains("● All               6"), "{t}");
    assert!(t.contains("1 hidden by your Show filters"));
}

#[test]
fn filters_that_hide_everything_say_so() {
    let mut a = app("liminal-hq", 160, 40, true);
    press(&mut a, KeyCode::Char('3'));
    open_control(&mut a);
    press(&mut a, KeyCode::Char('3'));
    press(&mut a, KeyCode::Esc);
    let t = text(&render(&mut a));
    assert!(t.contains("Your Show filters hide all 1."), "{t}");
    assert!(t.contains("Press s to widen them."));
}
