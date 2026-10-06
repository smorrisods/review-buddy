//! Source aggregation: the Sources pane, the collapsed tab strip, tag colours and `in_all`.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, Terminal};
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

fn rows(buffer: &Buffer, y0: u16, y1: u16, width: u16) -> String {
    text(buffer)
        .lines()
        .skip(usize::from(y0))
        .take(usize::from(y1 - y0))
        .map(|l| {
            l.chars()
                .take(usize::from(width))
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn sources_pane_160x40_default_theme() {
    let mut a = app("liminal-hq", 160, 40, true);
    insta::assert_snapshot!(rows(&render(&mut a), 1, 12, 26));
}

#[test]
fn sources_pane_160x40_dusk() {
    let mut a = app("dusk", 160, 40, true);
    insta::assert_snapshot!(rows(&render(&mut a), 1, 12, 26));
}

#[test]
fn strip_129x40_default_theme() {
    let mut a = app("liminal-hq", 129, 40, true);
    insta::assert_snapshot!(rows(&render(&mut a), 1, 2, 129));
}

#[test]
fn strip_129x40_dusk_with_a_later_source_selected() {
    let mut a = app("dusk", 129, 40, true);
    press(&mut a, KeyCode::Char('3'));
    insta::assert_snapshot!(rows(&render(&mut a), 1, 2, 129));
}

#[test]
fn strip_100x30_default_theme() {
    let mut a = app("liminal-hq", 100, 30, true);
    insta::assert_snapshot!(rows(&render(&mut a), 1, 2, 100));
}

#[test]
fn strip_100x30_dusk() {
    let mut a = app("dusk", 100, 30, true);
    press(&mut a, KeyCode::Char('4'));
    insta::assert_snapshot!(rows(&render(&mut a), 1, 2, 100));
}

fn many_sources(count: usize) -> App {
    let mut a = app("liminal-hq", 100, 30, true);
    let template = a.state.sources[0].clone();
    a.state.sources = (0..count)
        .map(|i| {
            let mut s = template.clone();
            s.id = rb_core::SourceId::new(format!("src{i}"));
            s.label = format!("source-number-{i}");
            s
        })
        .collect();
    a
}

#[test]
fn strip_scrolls_to_keep_the_active_tab_visible_and_marks_hidden_ones() {
    let mut a = many_sources(8);
    let first = rows(&render(&mut a), 1, 2, 100);
    assert!(
        first.contains(" 1 ") && first.trim_end().ends_with('›'),
        "{first}"
    );
    assert!(!first.contains('‹'));
    for key in ['5', '9'] {
        press(&mut a, KeyCode::Char(key));
        let line = rows(&render(&mut a), 1, 2, 100);
        assert!(line.contains(&format!(" {key} ")), "{key}: {line}");
        assert!(line.contains('‹'), "{line}");
    }
    let last = rows(&render(&mut a), 1, 2, 100);
    assert!(!last.contains('›'), "{last}");
}

#[test]
fn strip_markers_are_click_targets() {
    let mut a = many_sources(9);
    press(&mut a, KeyCode::Char('5'));
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "‹").unwrap();
    click(&mut a, x, y);
    assert_eq!(a.dashboard.source, 3);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "›").unwrap();
    click(&mut a, x, y);
    assert_eq!(a.dashboard.source, 4);
}

#[test]
fn in_all_false_sources_are_left_out_of_all_but_stay_selectable() {
    let mut a = app("liminal-hq", 160, 40, true);
    let all = review_buddy::app::queue::count_in(&a.state, None);
    let id = a.state.sources[2].id.clone();
    let own = review_buddy::app::queue::count_in(&a.state, Some(&id));
    assert!(own > 0);
    a.state.sources[2].in_all = false;
    assert_eq!(
        review_buddy::app::queue::count_in(&a.state, None),
        all - own
    );
    press(&mut a, KeyCode::Char('4'));
    assert_eq!(review_buddy::app::queue::count_in(&a.state, Some(&id)), own);
    assert!(!a.queue().is_empty());
    let t = text(&render(&mut a));
    assert!(t.contains("not in All"), "{t}");
}

#[test]
fn tag_colour_applies_to_the_dot_and_the_row_tag() {
    let mut a = app("liminal-hq", 160, 40, true);
    let before = render(&mut a);
    a.state.sources[0].tag_colour = Some("#112233".into());
    a.state.sources[3].tag_colour = Some("not-a-colour".into());
    let after = render(&mut a);
    let dot = |b: &Buffer, name: &str| {
        let y = (1..40)
            .find(|y| {
                (0..26)
                    .map(|x| b[(x, *y)].symbol())
                    .collect::<String>()
                    .contains(name)
            })
            .unwrap();
        (0..26)
            .map(|x| &b[(x, y)])
            .find(|c| c.symbol() == "●")
            .unwrap()
            .fg
    };
    assert_eq!(dot(&after, "liminal-hq"), Color::Rgb(0x11, 0x22, 0x33));
    // An invalid colour never reaches the screen: the forge's own colour stays.
    assert_eq!(dot(&after, "gitlab.com"), dot(&before, "gitlab.com"));
    let tags = after
        .content()
        .iter()
        .filter(|c| c.fg == Color::Rgb(0x11, 0x22, 0x33) && c.symbol() == "G")
        .count();
    assert!(tags > 0, "GH tags on the first source take its colour");
}

#[test]
fn tag_colour_survives_no_colour_as_text() {
    let mut a = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: true,
        size: (160, 40),
    });
    update(&mut a, Msg::Loaded(Box::new(snapshot())));
    a.state.sources[0].tag_colour = Some("#112233".into());
    let buffer = render(&mut a);
    assert!(buffer.content().iter().all(|c| c.fg == Color::Reset));
    assert!(text(&buffer).contains("GH"));
}
