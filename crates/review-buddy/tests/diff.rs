//! Headless renders of the diff screen from the frozen demo data, pinned with `insta`.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, Terminal};
use rb_core::{FilePatch, FileStatus, ReviewDraft};
use rb_theme::ColourDepth;
use review_buddy::app::{
    update, Cmd, DiffData, DiffFocus, Msg, Phase, Screen, {App, AppConfig, Snapshot},
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

fn base(theme: &str, width: u16, height: u16, no_color: bool) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color,
        size: (width, height),
    });
    update(&mut app, Msg::Loaded(Box::new(snapshot())));
    app
}

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

/// Opens the selected change's diff and answers the load the way the runtime would.
fn open_with(app: &mut App, data: DiffData) {
    let cmds = press(app, KeyCode::Enter);
    let Some(Cmd::LoadDiff(id)) = cmds.into_iter().next() else {
        panic!("opening the diff asks for its patches");
    };
    update(
        app,
        Msg::DiffLoaded {
            id,
            result: Ok(Box::new(data)),
        },
    );
}

fn open_demo(theme: &str, width: u16, height: u16) -> App {
    let mut app = base(theme, width, height, false);
    assert_eq!(app.selected_change().unwrap().id.number, 214);
    let id = app.selected_change().unwrap().id.clone();
    let data = block_on(world().diff_data(&id)).unwrap();
    open_with(&mut app, data);
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

fn find(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let width = buffer.area.width;
    (0..buffer.area.height).find_map(|y| {
        let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        line.find(needle)
            .map(|byte| (line[..byte].chars().count() as u16, y))
    })
}

/// Where the diff pane's cursor marker is, ignoring the Files pane's current-file marker.
fn cursor_pos(buffer: &Buffer) -> Option<(u16, u16)> {
    (0..buffer.area.height)
        .flat_map(|y| (34..buffer.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| buffer[(x, y)].symbol() == "›")
}

fn bg_at(buffer: &Buffer, needle: &str) -> Color {
    let (x, y) = find(buffer, needle).unwrap_or_else(|| panic!("{needle} on screen"));
    buffer[(x, y)].bg
}

fn cursor_new_no(app: &App) -> Option<u32> {
    let state = app.diff.as_ref()?;
    let id = state.view.rows.line_id(state.view.cursor)?;
    state.current()?.diff.parsed()?.line(id)?.new_no
}

fn file(path: &str, patch: Option<String>) -> FilePatch {
    FilePatch {
        path: path.into(),
        old_path: None,
        status: FileStatus::Modified,
        adds: 1,
        dels: 0,
        patch,
    }
}

fn large_patch(lines: usize) -> String {
    let mut patch = format!("@@ -1,{lines} +1,{lines} @@ fn big()\n");
    for i in 1..=lines {
        if i % 100 == 0 {
            patch.push_str(&format!("-let old_{i} = {i};\n+let new_{i} = {i};\n"));
        } else {
            patch.push_str(&format!(" let value_{i} = {i};\n"));
        }
    }
    patch
}

#[test]
fn diff_160x40_default_theme() {
    let mut a = open_demo("liminal-hq", 160, 40);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn diff_160x40_dusk() {
    let mut a = open_demo("dusk", 160, 40);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn diff_100x30_default_theme() {
    let mut a = open_demo("liminal-hq", 100, 30);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn diff_100x30_dusk() {
    let mut a = open_demo("dusk", 100, 30);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn thread_and_pending_suggestion_blocks_100x30() {
    let mut a = open_demo("liminal-hq", 100, 30);
    press(&mut a, KeyCode::Char('n'));
    while cursor_new_no(&a) != Some(51) {
        press(&mut a, KeyCode::Char('j'));
    }
    let buffer = render(&mut a);
    let shown = text(&buffer);
    assert!(shown.contains("thread · line 44"));
    assert!(shown.contains("pending suggestion · line 51"));
    assert!(shown.contains("jo · 1d"));
    insta::assert_snapshot!(shown);
}

#[test]
fn tints_come_with_glyphs_and_vanish_without_colour() {
    let tiny = || {
        DiffData::new(
            vec![file(
                "src/a.rs",
                Some("@@ -1,2 +1,2 @@\n keep\n-old line\n+new line".into()),
            )],
            Vec::new(),
            ReviewDraft::default(),
        )
    };
    let mut a = base("liminal-hq", 160, 40, false);
    open_with(&mut a, tiny());
    let buffer = render(&mut a);
    let shown = text(&buffer);
    assert!(shown.contains("-  old line") || shown.contains("- old line"));
    assert!(shown.contains("+ new line"));
    let (removed, added) = (bg_at(&buffer, "old line"), bg_at(&buffer, "new line"));
    assert_ne!(added, removed);
    assert_ne!(added, Color::Reset);
    assert_ne!(removed, Color::Reset);

    let mut plain = base("liminal-hq", 160, 40, true);
    open_with(&mut plain, tiny());
    let buffer = render(&mut plain);
    assert_eq!(bg_at(&buffer, "new line"), Color::Reset);
    assert_eq!(bg_at(&buffer, "old line"), Color::Reset);
    assert!(text(&buffer).contains("+ new line"));
}

#[test]
fn cursor_marks_one_line_and_follows_j() {
    let mut a = open_demo("liminal-hq", 160, 40);
    let first = cursor_pos(&render(&mut a)).expect("a cursor");
    press(&mut a, KeyCode::Char('j'));
    let buffer = render(&mut a);
    let second = cursor_pos(&buffer).expect("a cursor");
    assert_eq!(second.1, first.1 + 1);
    assert_eq!(
        text(&buffer).matches('›').count(),
        2,
        "one in files, one in diff"
    );
}

#[test]
fn clicking_a_file_switches_the_diff() {
    let mut a = open_demo("liminal-hq", 160, 40);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "menubar.rs").expect("a second file");
    update(
        &mut a,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    let state = a.diff.as_ref().unwrap();
    assert_eq!(state.file, 1);
    assert_eq!(state.focus, DiffFocus::Files);
    let shown = text(&render(&mut a));
    assert!(shown.contains("src/ui/menubar.rs"));
    assert!(shown.contains("pub fn render(menus"));
}

#[test]
fn clicking_a_diff_line_moves_the_cursor_there() {
    let mut a = open_demo("liminal-hq", 160, 40);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "select_prev").expect("a line");
    update(
        &mut a,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    let buffer = render(&mut a);
    let (_, cy) = cursor_pos(&buffer).expect("cursor");
    assert_eq!(cy, y);
}

#[test]
fn large_file_renders_only_a_window() {
    let mut a = base("liminal-hq", 160, 40, false);
    let data = DiffData::new(
        vec![file("src/big.rs", Some(large_patch(20_000)))],
        Vec::new(),
        ReviewDraft::default(),
    );
    open_with(&mut a, data);
    for _ in 0..40 {
        press(&mut a, KeyCode::PageDown);
    }
    assert!(
        a.diff.as_ref().unwrap().view.chunks() < 20,
        "only the chunks that scrolled past are highlighted, not all 134"
    );
    let buffer = render(&mut a);
    let shown = text(&buffer);
    assert!(shown.contains("let value_"));
    assert!(
        !shown.contains("let value_1 ="),
        "the top of the file is out of view"
    );
    assert!(
        a.hits.len() < 60,
        "only visible rows register click targets, got {}",
        a.hits.len()
    );
    insta::assert_snapshot!(shown);
}

#[test]
fn large_file_jump_to_the_end_is_instant_and_shows_the_last_line() {
    let mut a = base("liminal-hq", 160, 40, false);
    let data = DiffData::new(
        vec![file("src/big.rs", Some(large_patch(5_000)))],
        Vec::new(),
        ReviewDraft::default(),
    );
    open_with(&mut a, data);
    press(&mut a, KeyCode::Char('G'));
    let shown = text(&render(&mut a));
    assert!(shown.contains("let new_5000"));
}

#[test]
fn binary_file_shows_the_fallback() {
    let mut a = base("liminal-hq", 100, 30, false);
    let data = DiffData::new(
        vec![
            file("assets/logo.png", None),
            file("src/a.rs", Some("@@ -1 +1 @@\n-a\n+b".into())),
        ],
        Vec::new(),
        ReviewDraft::default(),
    );
    open_with(&mut a, data);
    let shown = text(&render(&mut a));
    assert!(shown.contains("binary or very large"));
    insta::assert_snapshot!(shown);
}

#[test]
fn loading_and_error_states() {
    let mut a = base("liminal-hq", 100, 30, false);
    press(&mut a, KeyCode::Enter);
    assert_eq!(a.diff.as_ref().unwrap().phase, Phase::Loading);
    let shown = text(&render(&mut a));
    assert!(shown.contains("Loading the diff"));
    insta::assert_snapshot!("diff_loading_100x30", shown);

    let id = a.diff.as_ref().unwrap().id.clone();
    update(
        &mut a,
        Msg::DiffLoaded {
            id,
            result: Err("GitHub didn't answer".into()),
        },
    );
    let shown = text(&render(&mut a));
    assert!(shown.contains("The diff didn't load: GitHub didn't answer"));
    assert!(shown.contains("esc to go back"));
    press(&mut a, KeyCode::Esc);
    assert_eq!(a.screen, Screen::Dashboard);
}

#[test]
fn esc_returns_to_the_same_dashboard_selection() {
    let mut a = base("liminal-hq", 160, 40, false);
    press(&mut a, KeyCode::Char('j'));
    let picked = a.selected_change().unwrap().id.clone();
    open_with(
        &mut a,
        DiffData::new(Vec::new(), Vec::new(), ReviewDraft::default()),
    );
    let shown = text(&render(&mut a));
    assert!(shown.contains("This change has no files."));
    press(&mut a, KeyCode::Esc);
    assert_eq!(a.selected_change().unwrap().id, picked);
    assert!(text(&render(&mut a)).contains("queue"));
}
