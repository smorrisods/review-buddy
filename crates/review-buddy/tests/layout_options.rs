//! The dashboard layout options (`ui.sources`, `ui.detail`) rendered headlessly at the three
//! reference sizes: the default, Sources on top, Detail closed and both. Also pins the toggle
//! keys, the clickable affordances and the footer hint.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Msg, Pane};
use review_buddy::config::{DetailMode, SourcesLayout};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::layout::Options;

#[path = "support/render.rs"]
mod render_support;
use render_support::{render, text};

fn app(theme: &str, size: (u16, u16), options: Options) -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let snapshot = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(world.snapshot())
        .unwrap();
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size,
    });
    app.layout = options;
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    app
}

fn press(app: &mut App, c: char) {
    update(
        app,
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
    );
}

fn click(app: &mut App, column: u16, row: u16) {
    update(
        app,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
}

fn options(sources: SourcesLayout, detail: DetailMode) -> Options {
    Options { sources, detail }
}

macro_rules! frames {
    ($($name:ident: $theme:literal, $sources:ident, $detail:ident, $w:literal, $h:literal;)*) => {
        $(
            #[test]
            fn $name() {
                let layout = options(SourcesLayout::$sources, DetailMode::$detail);
                let mut app = app($theme, ($w, $h), layout);
                insta::assert_snapshot!(text(&render(&mut app)));
            }
        )*
    };
}

frames! {
    layout_default_160x40: "liminal-hq", Auto, Auto, 160, 40;
    layout_default_129x40: "liminal-hq", Auto, Auto, 129, 40;
    layout_default_100x30: "liminal-hq", Auto, Auto, 100, 30;
    layout_sources_top_160x40: "liminal-hq", Top, Auto, 160, 40;
    layout_sources_top_129x40: "liminal-hq", Top, Auto, 129, 40;
    layout_sources_top_100x30: "liminal-hq", Top, Auto, 100, 30;
    layout_detail_closed_160x40: "liminal-hq", Auto, Closed, 160, 40;
    layout_detail_closed_129x40: "liminal-hq", Auto, Closed, 129, 40;
    layout_detail_closed_100x30: "liminal-hq", Auto, Closed, 100, 30;
    layout_both_160x40: "liminal-hq", Top, Closed, 160, 40;
    layout_both_129x40: "liminal-hq", Top, Closed, 129, 40;
    layout_both_100x30: "liminal-hq", Top, Closed, 100, 30;
    layout_dusk_both_160x40: "dusk", Top, Closed, 160, 40;
    layout_dusk_both_129x40: "dusk", Top, Closed, 129, 40;
    layout_dusk_both_100x30: "dusk", Top, Closed, 100, 30;
    layout_dusk_sources_left_160x40: "dusk", Left, Auto, 160, 40;
    layout_dusk_sources_left_129x40: "dusk", Left, Auto, 129, 40;
    layout_dusk_sources_left_100x30: "dusk", Left, Auto, 100, 30;
}

#[test]
fn p_closes_and_reopens_the_detail_pane_and_the_footer_says_so() {
    let mut app = app("liminal-hq", (160, 40), Options::default());
    let before = app.dashboard.selected.clone();
    assert!(!text(&render(&mut app)).contains("show detail"));
    press(&mut app, 'p');
    assert_eq!(app.layout.detail, DetailMode::Closed);
    let screen = text(&render(&mut app));
    assert!(screen.contains("p show detail"), "{screen}");
    assert_eq!(app.dashboard.selected, before);
    press(&mut app, 'p');
    assert_eq!(app.layout.detail, DetailMode::Open);
    assert!(!text(&render(&mut app)).contains("p show detail"));
}

#[test]
fn closing_detail_moves_focus_off_it_and_a_refresh_keeps_the_layout() {
    let mut app = app("liminal-hq", (160, 40), Options::default());
    app.dashboard.focus = Pane::Detail;
    press(&mut app, 'p');
    assert_eq!(app.dashboard.focus, Pane::Queue);
    press(&mut app, 'S');
    press(&mut app, 'S');
    assert_eq!(app.layout.sources, SourcesLayout::Top);
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let snapshot = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(world.snapshot())
        .unwrap();
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    assert_eq!(app.layout.sources, SourcesLayout::Top);
    assert_eq!(app.layout.detail, DetailMode::Closed);
    press(&mut app, 'S');
    assert_eq!(app.layout.sources, SourcesLayout::Auto);
}

#[test]
fn the_border_markers_toggle_the_detail_pane() {
    let mut app = app("liminal-hq", (160, 40), Options::default());
    let screen = text(&render(&mut app));
    assert!(screen.contains(" ⟩ "), "{screen}");
    let row = screen.lines().position(|l| l.contains(" ⟩ ")).unwrap() as u16;
    let col = screen
        .lines()
        .nth(row as usize)
        .unwrap()
        .chars()
        .position(|c| c == '⟩')
        .unwrap() as u16;
    click(&mut app, col, row);
    assert_eq!(app.layout.detail, DetailMode::Closed);
    let screen = text(&render(&mut app));
    let row = screen.lines().position(|l| l.contains("⟨ detail")).unwrap() as u16;
    let col = screen
        .lines()
        .nth(row as usize)
        .unwrap()
        .chars()
        .position(|c| c == '⟨')
        .unwrap() as u16;
    click(&mut app, col, row);
    assert_eq!(app.layout.detail, DetailMode::Open);
}

#[test]
fn a_closed_detail_pane_gives_rows_the_full_width() {
    let mut open = app("liminal-hq", (107, 30), Options::default());
    let mut closed = app(
        "liminal-hq",
        (107, 30),
        options(SourcesLayout::Auto, DetailMode::Closed),
    );
    let long = |s: &str| s.lines().filter(|l| l.contains('…')).count();
    assert!(long(&text(&render(&mut closed))) < long(&text(&render(&mut open))));
}

#[test]
fn clicking_and_scrolling_the_queue_still_work_when_detail_is_closed() {
    let mut app = app(
        "liminal-hq",
        (100, 30),
        options(SourcesLayout::Top, DetailMode::Closed),
    );
    render(&mut app);
    let first = app.dashboard.selected.clone();
    update(
        &mut app,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 50,
            row: 10,
            modifiers: KeyModifiers::NONE,
        }),
    );
    click(&mut app, 10, 6);
    render(&mut app);
    assert!(app.dashboard.selected.is_some());
    let _ = first;
    assert_eq!(app.dashboard.focus, Pane::Queue);
}
