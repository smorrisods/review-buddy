//! Headless renders of the dashboard from the frozen demo data, pinned with `insta`.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, Terminal};
use rb_theme::ColourDepth;
use review_buddy::app::{update, Action, App, AppConfig, Msg, Pane, Snapshot, Tab};
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

#[test]
fn dashboard_160x40_default_theme() {
    let mut a = app("liminal-hq", 160, 40, true);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn dashboard_160x40_dusk() {
    let mut a = app("dusk", 160, 40, true);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn dashboard_100x30_default_theme() {
    let mut a = app("liminal-hq", 100, 30, true);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn dashboard_100x30_dusk() {
    let mut a = app("dusk", 100, 30, true);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn dashboard_129x40_collapses_sources_into_tabs() {
    let mut a = app("liminal-hq", 129, 40, true);
    let buffer = render(&mut a);
    assert!(!text(&buffer).contains("╭ sources "));
    insta::assert_snapshot!(text(&buffer));
}

#[test]
fn dashboard_130x40_keeps_the_sources_pane() {
    let mut a = app("liminal-hq", 130, 40, true);
    let buffer = render(&mut a);
    assert!(text(&buffer).contains("╭ sources "));
    insta::assert_snapshot!(text(&buffer));
}

#[test]
fn checks_tab_with_noise_expanded() {
    let mut a = app("liminal-hq", 160, 40, true);
    press(&mut a, KeyCode::Char('G'));
    press(&mut a, KeyCode::Enter);
    press(&mut a, KeyCode::Char(']'));
    press(&mut a, KeyCode::Char(']'));
    press(&mut a, KeyCode::Char('g'));
    let buffer = render(&mut a);
    assert!(text(&buffer).contains("⏎ to collapse"));
    insta::assert_snapshot!(text(&buffer));
}

#[test]
fn single_source_view_and_files_tab() {
    let mut a = app("liminal-hq", 160, 40, true);
    press(&mut a, KeyCode::Char('3'));
    press(&mut a, KeyCode::Char('['));
    press(&mut a, KeyCode::Char(']'));
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn loading_shows_placeholders_in_each_pane() {
    let mut a = app("liminal-hq", 160, 40, false);
    a.state.loading = true;
    let buffer = render(&mut a);
    assert_eq!(text(&buffer).matches("·  ·  ·").count(), 3);
    insta::assert_snapshot!(text(&buffer));
}

#[test]
fn nothing_connected_keeps_the_calm_prompt() {
    let mut a = app("liminal-hq", 160, 40, false);
    let buffer = render(&mut a);
    assert!(text(&buffer).contains("Nothing connected yet."));
    assert!(text(&buffer).contains("No sources yet."));
}

#[test]
fn caught_up_when_nothing_needs_you() {
    let mut a = app("liminal-hq", 160, 40, true);
    let mut empty = snapshot();
    empty.changes.clear();
    update(&mut a, Msg::Loaded(Box::new(empty)));
    let buffer = render(&mut a);
    let t = text(&buffer);
    assert!(t.contains("That's everything.") && t.contains("Nothing is waiting on you."));
    assert!(t.contains("Pick a change to read it here."));
}

#[test]
fn queue_matches_the_row_spec() {
    let mut a = app("liminal-hq", 160, 40, true);
    let buffer = render(&mut a);
    let t = text(&buffer);
    for needle in [
        "Waiting on you",
        "Worth a look",
        "Can wait",
        "Add a menu bar and keyboard-driven menus",
        "GH review-buddy#214 · ada · review requested",
        "GL flow!1182",
        "Noise · 2 bot updates · ⏎ to expand",
        "── That's everything.",
    ] {
        assert!(t.contains(needle), "missing {needle:?}\n{t}");
    }
    let (x, y) = find(&buffer, "◐ Add a menu bar").unwrap();
    assert_eq!(buffer[(x - 2, y)].symbol(), "▌");
    assert_eq!(
        buffer[(x - 2, y + 1)].symbol(),
        "▌",
        "rule spans both lines"
    );
    assert_ne!(buffer[(40, y)].bg, Color::Reset, "selection background");
}

#[test]
fn focused_pane_border_uses_the_accent_and_others_the_line_colour() {
    let mut a = app("liminal-hq", 160, 40, true);
    let queue = render(&mut a);
    let accent = queue[(26, 1)].fg;
    let line = queue[(74, 1)].fg;
    assert_ne!(accent, line);
    assert_eq!(queue[(0, 1)].fg, line);
    press(&mut a, KeyCode::Tab);
    let detail = render(&mut a);
    assert_eq!(detail[(74, 1)].fg, accent);
    assert_eq!(detail[(26, 1)].fg, line);
}

#[test]
fn clicking_a_row_selects_it_and_clicking_a_pane_focuses_it() {
    let mut a = app("liminal-hq", 160, 40, true);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "Cache pipeline status lookups").unwrap();
    press(&mut a, KeyCode::Char('h'));
    click(&mut a, x, y);
    assert_eq!(a.dashboard.focus, Pane::Queue);
    assert_eq!(a.selected_change().unwrap().id.number, 1182);

    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "Checks").unwrap();
    click(&mut a, x + 1, y);
    assert_eq!(a.dashboard.tab, Tab::Checks);

    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "smorris").unwrap();
    assert!(x < 26);
    click(&mut a, x, y);
    assert_eq!(a.dashboard.focus, Pane::Sources);
    assert_eq!(a.dashboard.source, 2);

    click(&mut a, 100, 36);
    assert_eq!(a.dashboard.focus, Pane::Detail);
}

#[test]
fn chips_and_tabs_are_click_targets() {
    let mut a = app("liminal-hq", 160, 40, true);
    let buffer = render(&mut a);
    for chip in ["Approve", "Request changes", "Comment", "Diff", "Merge"] {
        let (x, y) = find(&buffer, chip).unwrap();
        assert!(matches!(a.hits.at(x, y), Some(Action::Chip(_))), "{chip}");
    }
    for tab in ["Overview", "Files", "Checks", "Conversation"] {
        let (x, y) = find(&buffer, tab).unwrap();
        assert!(
            matches!(a.hits.at(x, y), Some(Action::SelectTab(_))),
            "{tab}"
        );
    }
}

#[test]
fn collapsed_source_tabs_are_clickable() {
    let mut a = app("liminal-hq", 110, 30, true);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "platform").unwrap();
    assert_eq!(y, 1);
    click(&mut a, x, y);
    assert_eq!(a.dashboard.source, 3);
}

#[test]
fn no_colour_keeps_every_signal_as_text() {
    let mut a = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: true,
        size: (160, 40),
    });
    update(&mut a, Msg::Loaded(Box::new(snapshot())));
    let buffer = render(&mut a);
    assert!(buffer
        .content()
        .iter()
        .all(|c| c.fg == Color::Reset && c.bg == Color::Reset));
    let t = text(&buffer);
    assert!(t.contains('▌') && t.contains('●') && t.contains('◐') && t.contains('✕'));
}
