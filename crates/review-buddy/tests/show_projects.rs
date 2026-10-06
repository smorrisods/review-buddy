//! The Projects section of the Show control: a long list that scrolls and searches, select
//! all/none, clicks and the wheel, and how the queue follows. Rendered headlessly.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_core::{ChangeSummary, SourceId};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Msg, Snapshot};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::{self, HitMap};

/// The demo queue plus a spread of projects on its first source, like a busy organisation.
fn snapshot() -> Snapshot {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let mut snap = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(world.snapshot())
        .unwrap();
    let template: ChangeSummary = snap.changes[0].clone();
    let source: SourceId = template.id.source_id.clone();
    let repos = [
        "ontario-design-system",
        "ontario-design-system-legacy-docs",
        "legacy-portal",
        "legacy-forms",
        "legacy-search",
        "eslint-config",
        "tokens",
        "icons",
        "web-components",
        "storybook",
        "release-tools",
        "status-page",
    ];
    for (i, repo) in repos.iter().enumerate() {
        let mut change = template.clone();
        change.id.source_id = source.clone();
        change.id.repo = format!("ongov/{repo}");
        change.id.number = 900 + i as u64;
        change.title = format!("Update {repo}");
        snap.changes.push(change);
    }
    snap
}

fn app(theme: &str, width: u16, height: u16) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (width, height),
    });
    update(&mut app, Msg::Loaded(Box::new(snapshot())));
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

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn mouse(app: &mut App, kind: MouseEventKind, x: u16, y: u16) {
    update(
        app,
        Msg::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
    );
}

fn searching(a: &mut App) {
    press(a, KeyCode::Char('s'));
    press(a, KeyCode::Char('/'));
    type_text(a, "design");
}

fn narrowed(a: &mut App) {
    press(a, KeyCode::Char('s'));
    press(a, KeyCode::Char('/'));
    type_text(a, "legacy");
    press(a, KeyCode::Enter);
    press(a, KeyCode::Char('n'));
    press(a, KeyCode::Esc);
}

#[test]
fn projects_search_160x40_default_theme() {
    let mut a = app("liminal-hq", 160, 40);
    searching(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn projects_search_160x40_dusk() {
    let mut a = app("dusk", 160, 40);
    searching(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn projects_search_100x30_default_theme() {
    let mut a = app("liminal-hq", 100, 30);
    searching(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn projects_search_100x30_dusk() {
    let mut a = app("dusk", 100, 30);
    searching(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn projects_narrowed_160x40_default_theme() {
    let mut a = app("liminal-hq", 160, 40);
    narrowed(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn projects_narrowed_160x40_dusk() {
    let mut a = app("dusk", 160, 40);
    narrowed(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn projects_narrowed_100x30_default_theme() {
    let mut a = app("liminal-hq", 100, 30);
    narrowed(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn projects_narrowed_100x30_dusk() {
    let mut a = app("dusk", 100, 30);
    narrowed(&mut a);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn the_overlay_fits_a_long_list_at_100x30_and_says_how_many_are_shown() {
    let mut a = app("liminal-hq", 100, 30);
    press(&mut a, KeyCode::Char('s'));
    let t = text(&render(&mut a));
    assert!(t.contains("Projects  16 of 16 projects shown"), "{t}");
    assert!(t.contains("esc close"), "the hints stay on screen:\n{t}");
    press(&mut a, KeyCode::Char('G'));
    let t = text(&render(&mut a));
    assert!(
        t.contains("esc close") && t.contains("[x] smorris/dotfiles"),
        "{t}"
    );
}

#[test]
fn clicking_a_project_hides_its_changes_and_the_counts_follow() {
    let mut a = app("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('s'));
    let buffer = render(&mut a);
    assert!(
        text(&buffer).contains("Projects  16 of 16 projects shown"),
        "{}",
        text(&buffer)
    );
    let (x, y) = find(&buffer, "[x] ongov/tokens").unwrap();
    mouse(&mut a, MouseEventKind::Down(MouseButton::Left), x + 6, y);
    let t = text(&render(&mut a));
    assert!(t.contains("[ ] ongov/tokens"), "{t}");
    assert!(t.contains("Projects  15 of 16 projects shown"), "{t}");
    assert!(
        t.contains("1 hidden by your Show filters: 1 by project."),
        "{t}"
    );
    press(&mut a, KeyCode::Esc);
    let t = text(&render(&mut a));
    assert!(!t.contains("Update tokens"), "{t}");
    assert!(t.contains("● All              18"), "{t}");
}

#[test]
fn the_wheel_scrolls_the_project_list() {
    let mut a = app("liminal-hq", 100, 30);
    press(&mut a, KeyCode::Char('s'));
    let before = text(&render(&mut a));
    assert!(!before.contains("smorris/dotfiles"));
    for _ in 0..4 {
        mouse(&mut a, MouseEventKind::ScrollDown, 50, 15);
    }
    let after = text(&render(&mut a));
    assert!(after.contains("smorris/dotfiles"), "{after}");
    assert!(a.show.open);
}

#[test]
fn a_none_under_a_search_leaves_the_other_projects_ticked() {
    let mut a = app("liminal-hq", 160, 40);
    narrowed(&mut a);
    let t = text(&render(&mut a));
    assert!(t.contains("Projects  12 of 16 projects shown"), "{t}");
    assert!(t.contains("[ ] ongov/legacy-forms") && t.contains("[x] ongov/tokens"));
}

#[test]
fn clicking_the_search_line_focuses_it() {
    let mut a = app("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('s'));
    let (x, y) = find(&render(&mut a), "type to find a project").unwrap();
    mouse(&mut a, MouseEventKind::Down(MouseButton::Left), x, y);
    type_text(&mut a, "icons");
    let t = text(&render(&mut a));
    assert!(t.contains("1 match"), "{t}");
}

#[test]
fn new_projects_after_a_refresh_follow_the_show_all_default() {
    let mut a = app("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('s'));
    press(&mut a, KeyCode::Char('n'));
    let t = text(&render(&mut a));
    assert!(t.contains("Projects  0 of 16 projects shown"), "{t}");
    let mut snap = snapshot();
    let mut extra = snap.changes[0].clone();
    extra.id.repo = "ongov/brand-new".into();
    extra.id.number = 5000;
    snap.changes.push(extra);
    update(&mut a, Msg::Loaded(Box::new(snap)));
    let t = text(&render(&mut a));
    assert!(t.contains("Projects  1 of 17 projects shown"), "{t}");
    assert!(t.contains("[x] ongov/brand-new"), "{t}");
}
