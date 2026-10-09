//! Open, copy, help overlay, theme and quit behaviour against the frozen demo data.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Cmd, Msg, NoticeKind, Screen, Snapshot};
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

fn diff(theme: &str, width: u16, height: u16) -> App {
    let mut app = dashboard(theme, width, height);
    let id = app.selected_change().unwrap().id.clone();
    let data = block_on(world().diff_data(&id)).unwrap();
    press(&mut app, KeyCode::Enter);
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

/// The snapshots describe the Unix overlay, which lists `⌃Z suspend`; Windows doesn't. Key
/// handling clamps the scroll by the same row count, so the override applies there too.
fn unix_overlay() {
    review_buddy::ui::chrome::set_suspend_listed(true);
}

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    unix_overlay();
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn render(app: &mut App) -> Buffer {
    unix_overlay();
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

fn status(app: &App) -> String {
    app.status.as_ref().expect("a status").notice.text.clone()
}

#[test]
fn o_asks_the_runtime_to_open_the_selected_change() {
    let mut a = dashboard("liminal-hq", 160, 40);
    let cmds = press(&mut a, KeyCode::Char('o'));
    let [Cmd::OpenUrl(url)] = cmds.as_slice() else {
        panic!("expected one OpenUrl, got {cmds:?}");
    };
    assert!(url.starts_with("https://"), "{url}");
    assert!(url.ends_with("/214"), "{url}");
    assert!(url.contains("/pull/"), "{url}");
}

#[test]
fn y_asks_the_runtime_to_copy_the_same_url() {
    let mut a = dashboard("liminal-hq", 160, 40);
    let open = press(&mut a, KeyCode::Char('o'));
    let copy = press(&mut a, KeyCode::Char('y'));
    let (Cmd::OpenUrl(o), Cmd::Copy(c)) = (&open[0], &copy[0]) else {
        panic!("unexpected commands");
    };
    assert_eq!(o, c);
}

#[test]
fn in_the_diff_o_and_y_use_the_files_page() {
    let mut a = diff("liminal-hq", 160, 40);
    let Some(Cmd::OpenUrl(url)) = press(&mut a, KeyCode::Char('o')).into_iter().next() else {
        panic!("expected OpenUrl");
    };
    assert!(url.ends_with("/pull/214/files"), "{url}");
    let Some(Cmd::Copy(url)) = press(&mut a, KeyCode::Char('y')).into_iter().next() else {
        panic!("expected Copy");
    };
    assert!(url.ends_with("/pull/214/files"), "{url}");
}

#[test]
fn status_messages_arrive_in_the_footer_and_expire() {
    let mut a = dashboard("liminal-hq", 160, 40);
    let cmds = update(
        &mut a,
        Msg::Status(review_buddy::app::Notice::new(
            NoticeKind::Success,
            "Copied https://x.test/1",
        )),
    );
    assert!(matches!(
        cmds.as_slice(),
        [Cmd::After {
            msg: Msg::StatusExpired(_),
            ..
        }]
    ));
    assert!(text(&render(&mut a)).contains("✓ Copied https://x.test/1"));
}

#[test]
fn question_mark_opens_and_closes_the_overlay() {
    let mut a = dashboard("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('?'));
    assert!(a.help);
    assert!(text(&render(&mut a)).contains("Keys · Queue"));
    press(&mut a, KeyCode::Esc);
    assert!(!a.help);
    press(&mut a, KeyCode::Char('?'));
    press(&mut a, KeyCode::Char('?'));
    assert!(!a.help);
    assert!(!text(&render(&mut a)).contains("Keys ·"));
}

#[test]
fn the_overlay_swallows_other_keys_but_ctrl_c_still_quits() {
    let mut a = dashboard("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('?'));
    assert!(press(&mut a, KeyCode::Char('o')).is_empty());
    assert!(press(&mut a, KeyCode::Char('q')).is_empty());
    assert!(!a.should_quit());
    update(
        &mut a,
        Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
    );
    assert!(a.should_quit());
}

#[test]
fn the_overlay_lists_every_footer_hint() {
    for mut a in [
        dashboard("liminal-hq", 160, 40),
        diff("liminal-hq", 160, 40),
    ] {
        press(&mut a, KeyCode::Char('?'));
        let shown = text(&render(&mut a));
        for hint in ui::chrome::hints_for(a.screen) {
            assert!(
                shown.contains(hint.key),
                "{} missing on {:?}",
                hint.key,
                a.screen
            );
        }
    }
}

#[test]
fn q_goes_back_in_the_diff_and_quits_on_the_dashboard() {
    let mut a = diff("liminal-hq", 160, 40);
    a.diff.as_mut().unwrap().data.as_mut().unwrap().draft = rb_core::ReviewDraft::default();
    press(&mut a, KeyCode::Char('q'));
    assert_eq!(a.screen, Screen::Dashboard);
    assert!(!a.should_quit());
    press(&mut a, KeyCode::Char('q'));
    assert!(a.should_quit());
}

#[test]
fn quitting_with_unsent_drafts_asks_first() {
    use rb_core::{DraftComment, ReviewDraft};
    let mut a = diff("liminal-hq", 160, 40);
    let comment = DraftComment {
        path: "src/a.rs".into(),
        side: rb_core::Side::New,
        start_line: None,
        line: 1,
        body: "wip".into(),
    };
    a.diff.as_mut().unwrap().data.as_mut().unwrap().draft = ReviewDraft {
        body: "wip".into(),
        comments: vec![comment],
    };
    assert!(a.has_unsent_drafts());
    let ctrl_c = || Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    update(&mut a, ctrl_c());
    assert!(!a.should_quit());
    assert!(status(&a).contains("aren't saved"));
    update(&mut a, ctrl_c());
    assert!(a.should_quit());
}

#[test]
fn theme_cycle_names_the_theme() {
    let mut a = dashboard("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('T'));
    assert_eq!(status(&a), format!("Theme: {}", a.theme_name()));
}

#[test]
fn help_dashboard_160x40_default_theme() {
    let mut a = dashboard("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('?'));
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn help_dashboard_160x40_dusk() {
    let mut a = dashboard("dusk", 160, 40);
    press(&mut a, KeyCode::Char('?'));
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn help_dashboard_100x30_default_theme() {
    let mut a = dashboard("liminal-hq", 100, 30);
    press(&mut a, KeyCode::Char('?'));
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn help_dashboard_100x30_dusk() {
    let mut a = dashboard("dusk", 100, 30);
    press(&mut a, KeyCode::Char('?'));
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn help_diff_160x40_default_theme() {
    let mut a = diff("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('?'));
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn help_diff_160x40_dusk() {
    let mut a = diff("dusk", 160, 40);
    press(&mut a, KeyCode::Char('?'));
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn help_diff_100x30_default_theme() {
    let mut a = diff("liminal-hq", 100, 30);
    press(&mut a, KeyCode::Char('?'));
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn help_diff_100x30_dusk() {
    let mut a = diff("dusk", 100, 30);
    press(&mut a, KeyCode::Char('?'));
    insta::assert_snapshot!(text(&render(&mut a)));
}

fn scrolled_to_end(theme: &str, width: u16, height: u16) -> App {
    let mut a = dashboard(theme, width, height);
    press(&mut a, KeyCode::Char('?'));
    // Scroll until the overlay stops moving, however many rows the platform lists.
    loop {
        let before = a.help_scroll;
        press(&mut a, KeyCode::Char('j'));
        if a.help_scroll == before {
            break;
        }
    }
    a
}

#[test]
fn the_legend_explains_every_status_mark_and_scrolls_into_view() {
    for (width, height) in [(100, 30), (160, 40)] {
        let mut a = scrolled_to_end("liminal-hq", width, height);
        let shown = text(&render(&mut a));
        assert!(shown.contains("Queue marks"), "{width}x{height}");
        for mark in ui::chrome::legend(a.screen) {
            assert!(
                shown.contains(mark.hint.key),
                "{} at {width}x{height}",
                mark.hint.key
            );
            assert!(
                shown.contains(mark.hint.label),
                "{} at {width}x{height}",
                mark.hint.label
            );
        }
        for glyph in [
            "✓N",
            "✕N",
            "○N",
            "¶N",
            "N open",
            "CI running",
            "CI failing",
            "+N −M",
        ] {
            assert!(shown.contains(glyph), "{glyph} at {width}x{height}");
        }
    }
}

#[test]
fn the_suspend_row_and_the_general_keys_still_show_above_the_legend() {
    let mut a = dashboard("liminal-hq", 100, 30);
    press(&mut a, KeyCode::Char('?'));
    // Scroll down until the suspend row appears (the legend follows it).
    let mut shown = text(&render(&mut a));
    for _ in 0..100 {
        if shown.contains("⌃Z") {
            break;
        }
        press(&mut a, KeyCode::Char('j'));
        shown = text(&render(&mut a));
    }
    assert!(shown.contains("⌃Z"), "suspend row\n{shown}");
    assert!(shown.contains("quit"));
}

#[test]
fn help_dashboard_100x30_scrolled_to_the_legend() {
    let mut a = scrolled_to_end("liminal-hq", 100, 30);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn help_dashboard_160x40_scrolled_to_the_legend() {
    let mut a = scrolled_to_end("liminal-hq", 160, 40);
    insta::assert_snapshot!(text(&render(&mut a)));
}
