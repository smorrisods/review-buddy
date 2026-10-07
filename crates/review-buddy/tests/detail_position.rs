//! `ui.detail_position` rendered headlessly at the three reference sizes for each position,
//! with Dusk, Sources on top and a closed Detail for a few, plus the `P` key, the markers and
//! hit-testing in the stacked layouts.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Msg, Pane};
use review_buddy::config::{DetailMode, DetailPosition, SourcesLayout};
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

fn key(app: &mut App, code: KeyCode) {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    update(
        app,
        Msg::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    );
}

fn with(position: DetailPosition, sources: SourcesLayout, detail: DetailMode) -> Options {
    Options {
        sources,
        detail,
        position,
        ..Options::default()
    }
}

macro_rules! frames {
    ($($name:ident: $theme:literal, $pos:ident, $sources:ident, $detail:ident, $w:literal, $h:literal;)*) => {
        $(
            #[test]
            fn $name() {
                let layout = with(DetailPosition::$pos, SourcesLayout::$sources, DetailMode::$detail);
                let mut app = app($theme, ($w, $h), layout);
                insta::assert_snapshot!(text(&render(&mut app)));
            }
        )*
    };
}

frames! {
    position_auto_160x40: "liminal-hq", Auto, Auto, Auto, 160, 40;
    position_auto_129x40: "liminal-hq", Auto, Auto, Auto, 129, 40;
    position_auto_100x30: "liminal-hq", Auto, Auto, Auto, 100, 30;
    position_right_160x40: "liminal-hq", Right, Auto, Auto, 160, 40;
    position_right_129x40: "liminal-hq", Right, Auto, Auto, 129, 40;
    position_right_100x30: "liminal-hq", Right, Auto, Auto, 100, 30;
    position_left_160x40: "liminal-hq", Left, Auto, Auto, 160, 40;
    position_left_129x40: "liminal-hq", Left, Auto, Auto, 129, 40;
    position_left_100x30: "liminal-hq", Left, Auto, Auto, 100, 30;
    position_top_160x40: "liminal-hq", Top, Auto, Auto, 160, 40;
    position_top_129x40: "liminal-hq", Top, Auto, Auto, 129, 40;
    position_top_100x30: "liminal-hq", Top, Auto, Auto, 100, 30;
    position_bottom_160x40: "liminal-hq", Bottom, Auto, Auto, 160, 40;
    position_bottom_129x40: "liminal-hq", Bottom, Auto, Auto, 129, 40;
    position_bottom_100x30: "liminal-hq", Bottom, Auto, Auto, 100, 30;
    position_dusk_left_129x40: "dusk", Left, Auto, Auto, 129, 40;
    position_dusk_top_100x30: "dusk", Top, Auto, Auto, 100, 30;
    position_top_sources_top_100x30: "liminal-hq", Bottom, Top, Auto, 100, 30;
    position_top_sources_left_160x40: "liminal-hq", Top, Left, Auto, 160, 40;
    position_left_closed_129x40: "liminal-hq", Left, Auto, Closed, 129, 40;
    position_bottom_closed_100x30: "liminal-hq", Bottom, Auto, Closed, 100, 30;
}

fn line_of(screen: &str, needle: &str) -> usize {
    screen
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} in\n{screen}"))
}

fn column_of(screen: &str, needle: &str) -> usize {
    let line = screen.lines().find(|l| l.contains(needle)).unwrap();
    line.chars()
        .position(|c| needle.starts_with(c))
        .map(|_| {
            line.find(needle)
                .map(|b| line[..b].chars().count())
                .unwrap()
        })
        .unwrap()
}

#[test]
fn the_panes_land_where_the_position_says() {
    let screen = |p| {
        let mut a = app(
            "liminal-hq",
            (129, 40),
            with(p, SourcesLayout::Auto, DetailMode::Auto),
        );
        text(&render(&mut a))
    };
    let right = screen(DetailPosition::Right);
    assert!(column_of(&right, "queue") < column_of(&right, "Overview"));
    let left = screen(DetailPosition::Left);
    assert!(column_of(&left, "Overview") < column_of(&left, "queue"));
    let top = screen(DetailPosition::Top);
    assert!(line_of(&top, "Overview") < line_of(&top, "queue"));
    let bottom = screen(DetailPosition::Bottom);
    assert!(line_of(&bottom, "queue") < line_of(&bottom, "Overview"));
}

#[test]
fn p_cycles_with_a_toast_and_composes_with_s_and_the_closed_detail() {
    let mut a = app("liminal-hq", (129, 40), Options::default());
    let seen: Vec<&str> = (0..5)
        .map(|_| {
            press(&mut a, 'P');
            a.layout.position.as_str()
        })
        .collect();
    assert_eq!(seen, ["right", "left", "top", "bottom", "auto"]);
    press(&mut a, 'P');
    assert!(text(&render(&mut a)).contains("Detail position: right"));
    press(&mut a, 'S');
    press(&mut a, 'p');
    press(&mut a, 'P');
    assert_eq!(a.layout.sources, SourcesLayout::Left);
    assert_eq!(a.layout.detail, DetailMode::Closed);
    assert_eq!(a.layout.position, DetailPosition::Left);
}

#[test]
fn markers_follow_the_arrangement() {
    for (p, close, reopen) in [
        (DetailPosition::Right, " ⟩ ", "⟨ detail"),
        (DetailPosition::Left, " ⟨ ", "⟩ detail"),
        (DetailPosition::Top, " ▲ ", "▼ detail"),
        (DetailPosition::Bottom, " ▼ ", "▲ detail"),
    ] {
        let mut a = app(
            "liminal-hq",
            (129, 40),
            with(p, SourcesLayout::Auto, DetailMode::Auto),
        );
        assert!(text(&render(&mut a)).contains(close), "{p:?} close");
        press(&mut a, 'p');
        assert!(text(&render(&mut a)).contains(reopen), "{p:?} reopen");
    }
}

#[test]
fn clicking_the_stacked_markers_toggles_detail() {
    let mut a = app(
        "liminal-hq",
        (100, 30),
        with(
            DetailPosition::Bottom,
            SourcesLayout::Auto,
            DetailMode::Auto,
        ),
    );
    let screen = text(&render(&mut a));
    let (row, col) = (line_of(&screen, " ▼ "), column_of(&screen, "▼"));
    mouse(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        col as u16,
        row as u16,
    );
    assert_eq!(a.layout.detail, DetailMode::Closed);
    let screen = text(&render(&mut a));
    let (row, col) = (line_of(&screen, "▲ detail"), column_of(&screen, "▲ detail"));
    mouse(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        col as u16,
        row as u16,
    );
    assert_eq!(a.layout.detail, DetailMode::Open);
}

#[test]
fn clicks_and_the_wheel_hit_the_pane_under_the_pointer_when_stacked() {
    for p in [DetailPosition::Top, DetailPosition::Bottom] {
        let mut a = app(
            "liminal-hq",
            (100, 30),
            with(p, SourcesLayout::Auto, DetailMode::Auto),
        );
        let screen = text(&render(&mut a));
        let (queue, detail) = (line_of(&screen, "╭ queue"), line_of(&screen, "Overview"));
        mouse(
            &mut a,
            MouseEventKind::Down(MouseButton::Left),
            20,
            detail as u16 + 1,
        );
        assert_eq!(a.dashboard.focus, Pane::Detail, "{p:?}");
        mouse(
            &mut a,
            MouseEventKind::Down(MouseButton::Left),
            20,
            queue as u16 + 2,
        );
        assert_eq!(a.dashboard.focus, Pane::Queue, "{p:?}");
        let before = a.dashboard.queue_scroll;
        mouse(&mut a, MouseEventKind::ScrollDown, 20, queue as u16 + 2);
        assert!(a.dashboard.queue_scroll >= before);
        let before = a.dashboard.detail_scroll;
        mouse(&mut a, MouseEventKind::ScrollDown, 20, detail as u16 + 1);
        assert_eq!(a.dashboard.queue_scroll, a.dashboard.queue_scroll);
        assert!(a.dashboard.detail_scroll >= before);
    }
}

#[test]
fn tab_follows_the_visual_order() {
    let order = |p, sources| {
        let mut a = app("liminal-hq", (160, 40), with(p, sources, DetailMode::Auto));
        let mut seen = vec![a.dashboard.focus];
        for _ in 0..3 {
            key(&mut a, KeyCode::Tab);
            seen.push(a.dashboard.focus);
        }
        seen
    };
    use Pane::*;
    assert_eq!(
        order(DetailPosition::Right, SourcesLayout::Top),
        [Queue, Detail, Queue, Detail]
    );
    assert_eq!(
        order(DetailPosition::Left, SourcesLayout::Top),
        [Queue, Detail, Queue, Detail]
    );
    assert_eq!(
        order(DetailPosition::Top, SourcesLayout::Left),
        [Queue, Sources, Detail, Queue]
    );
    assert_eq!(
        order(DetailPosition::Bottom, SourcesLayout::Left),
        [Queue, Detail, Sources, Queue]
    );
}

#[test]
fn a_short_detail_keeps_its_chips_and_tabs_and_scrolls() {
    let mut a = app(
        "liminal-hq",
        (100, 30),
        with(
            DetailPosition::Bottom,
            SourcesLayout::Auto,
            DetailMode::Auto,
        ),
    );
    let screen = text(&render(&mut a));
    assert!(
        screen.contains("Approve") && screen.contains("Overview"),
        "{screen}"
    );
    a.dashboard.focus = Pane::Detail;
    key(&mut a, KeyCode::PageDown);
    assert!(a.dashboard.detail_scroll > 0);
    assert!(text(&render(&mut a)).contains("Approve"));
}
