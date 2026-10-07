//! Review drafts: the queue marker, the Pending reviews list, the leave confirm, a restored
//! draft, editing and deleting a pending comment. Rendered headlessly from the frozen demo data
//! (pinned with `insta`) at 160x40 and 100x30 in the default theme and Dusk.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_core::{ChangeId, DraftComment, ReviewDraft, Side};
use rb_theme::ColourDepth;
use review_buddy::app::drafts::Drafts;
use review_buddy::app::{update, App, AppConfig, Cmd, Msg, Screen, Snapshot};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::drafts::StoredDraft;
use review_buddy::ui::{self, HitMap};

fn world() -> DemoWorld {
    DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap()
}

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(f)
}

fn snapshot(world: &DemoWorld) -> Snapshot {
    block_on(world.snapshot()).unwrap()
}

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
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

fn new_app(world: &DemoWorld, theme: &str, width: u16, height: u16) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (width, height),
    });
    update(&mut app, Msg::Loaded(Box::new(snapshot(world))));
    app
}

fn load_diff(app: &mut App, world: &DemoWorld, id: &ChangeId) {
    let data = block_on(world.diff_data(id)).unwrap();
    update(
        app,
        Msg::DiffLoaded {
            id: id.clone(),
            result: Ok(Box::new(data)),
        },
    );
}

fn open_diff(app: &mut App, world: &DemoWorld) -> ChangeId {
    let id = app.selected_change().unwrap().id.clone();
    assert_eq!(id.number, 214);
    let cmds = press(app, KeyCode::Enter);
    assert!(matches!(cmds.as_slice(), [Cmd::LoadDiff(_)]));
    load_diff(app, world, &id);
    assert_eq!(app.screen, Screen::Diff);
    id
}

fn comment(app: &mut App, text: &str) {
    press(app, KeyCode::Char('c'));
    type_text(app, text);
    press(app, KeyCode::Enter);
}

fn stored(app: &App, index: usize, comments: usize, sha: Option<&str>) -> StoredDraft {
    let change = &app.state.changes[index];
    let draft = ReviewDraft {
        body: String::new(),
        comments: (0..comments)
            .map(|n| DraftComment {
                path: "src/a.rs".into(),
                side: Side::New,
                start_line: None,
                line: n as u32 + 1,
                body: format!("Note {}", n + 1),
            })
            .collect(),
    };
    StoredDraft::new(
        change.id.clone(),
        change.title.clone(),
        sha.map_or_else(|| change.head_sha.clone(), str::to_string),
        change.updated_at.0 - 7_200,
        &draft,
        None,
    )
}

fn with_saved_drafts(theme: &str, width: u16, height: u16) -> App {
    let w = world();
    let mut app = new_app(&w, theme, width, height);
    let saved = vec![
        stored(&app, 0, 3, None),
        stored(&app, 1, 1, Some("an-older-head")),
        stored(&app, 2, 2, None),
    ];
    app.drafts = Drafts::new(None, saved);
    app
}

fn queue_markers(theme: &str, width: u16, height: u16) -> App {
    with_saved_drafts(theme, width, height)
}

fn pending_list(theme: &str, width: u16, height: u16) -> App {
    let mut app = with_saved_drafts(theme, width, height);
    press(&mut app, KeyCode::Char('D'));
    press(&mut app, KeyCode::Char('j'));
    app
}

fn pending_discard(theme: &str, width: u16, height: u16) -> App {
    let mut app = pending_list(theme, width, height);
    press(&mut app, KeyCode::Char('x'));
    app
}

fn leaving(theme: &str, width: u16, height: u16) -> App {
    let w = world();
    let mut app = new_app(&w, theme, width, height);
    open_diff(&mut app, &w);
    comment(&mut app, "This could return early.");
    press(&mut app, KeyCode::Esc);
    app
}

fn restored(theme: &str, width: u16, height: u16) -> App {
    let w = world();
    let mut app = new_app(&w, theme, width, height);
    let id = open_diff(&mut app, &w);
    comment(&mut app, "This could return early.");
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::Dashboard);
    press(&mut app, KeyCode::Enter);
    load_diff(&mut app, &w, &id);
    app
}

fn on_a_comment(theme: &str, width: u16, height: u16) -> App {
    let w = world();
    let mut app = new_app(&w, theme, width, height);
    open_diff(&mut app, &w);
    comment(
        &mut app,
        "This could return early.\nWorth a guard?"
            .replace('\n', " ")
            .as_str(),
    );
    app
}

fn editing(theme: &str, width: u16, height: u16) -> App {
    let mut app = on_a_comment(theme, width, height);
    press(&mut app, KeyCode::Char('e'));
    type_text(&mut app, " Edited.");
    app
}

fn deleting(theme: &str, width: u16, height: u16) -> App {
    let mut app = on_a_comment(theme, width, height);
    press(&mut app, KeyCode::Char('d'));
    app
}

macro_rules! snapshots {
    ($($name:ident: $scene:ident($theme:literal, $w:literal, $h:literal);)*) => {
        $(
            #[test]
            fn $name() {
                let mut a = $scene($theme, $w, $h);
                insta::assert_snapshot!(text(&render(&mut a)));
            }
        )*
    };
}

snapshots! {
    queue_markers_160x40_default_theme: queue_markers("liminal-hq", 160, 40);
    queue_markers_160x40_dusk: queue_markers("dusk", 160, 40);
    queue_markers_100x30_default_theme: queue_markers("liminal-hq", 100, 30);
    queue_markers_100x30_dusk: queue_markers("dusk", 100, 30);
    pending_list_160x40_default_theme: pending_list("liminal-hq", 160, 40);
    pending_list_160x40_dusk: pending_list("dusk", 160, 40);
    pending_list_100x30_default_theme: pending_list("liminal-hq", 100, 30);
    pending_list_100x30_dusk: pending_list("dusk", 100, 30);
    pending_discard_160x40_default_theme: pending_discard("liminal-hq", 160, 40);
    pending_discard_160x40_dusk: pending_discard("dusk", 160, 40);
    pending_discard_100x30_default_theme: pending_discard("liminal-hq", 100, 30);
    pending_discard_100x30_dusk: pending_discard("dusk", 100, 30);
    leave_confirm_160x40_default_theme: leaving("liminal-hq", 160, 40);
    leave_confirm_160x40_dusk: leaving("dusk", 160, 40);
    leave_confirm_100x30_default_theme: leaving("liminal-hq", 100, 30);
    leave_confirm_100x30_dusk: leaving("dusk", 100, 30);
    restored_status_160x40_default_theme: restored("liminal-hq", 160, 40);
    restored_status_160x40_dusk: restored("dusk", 160, 40);
    restored_status_100x30_default_theme: restored("liminal-hq", 100, 30);
    restored_status_100x30_dusk: restored("dusk", 100, 30);
    edit_composer_160x40_default_theme: editing("liminal-hq", 160, 40);
    edit_composer_160x40_dusk: editing("dusk", 160, 40);
    edit_composer_100x30_default_theme: editing("liminal-hq", 100, 30);
    edit_composer_100x30_dusk: editing("dusk", 100, 30);
    delete_confirm_160x40_default_theme: deleting("liminal-hq", 160, 40);
    delete_confirm_160x40_dusk: deleting("dusk", 160, 40);
    delete_confirm_100x30_default_theme: deleting("liminal-hq", 100, 30);
    delete_confirm_100x30_dusk: deleting("dusk", 100, 30);
}

#[test]
fn the_marker_sits_beside_the_age_and_survives_a_narrow_queue() {
    for (w, h) in [(160, 40), (100, 30)] {
        let mut app = queue_markers("liminal-hq", w, h);
        let shown = text(&render(&mut app));
        assert!(shown.contains("✎ 3"), "{w}x{h}\n{shown}");
        assert!(shown.contains("✎ 2") && shown.contains("✎ 1"), "{w}x{h}");
    }
}

#[test]
fn the_pending_list_names_the_outdated_one_and_the_footer_offers_it() {
    let mut app = pending_list("liminal-hq", 160, 40);
    let shown = text(&render(&mut app));
    assert!(shown.contains("Pending reviews · 3"), "{shown}");
    assert!(shown.contains("outdated"), "{shown}");
    press(&mut app, KeyCode::Esc);
    let shown = text(&render(&mut app));
    assert!(shown.contains("D pending"), "{shown}");
}

#[test]
fn leaving_keeps_the_draft_and_the_queue_shows_it() {
    let mut app = leaving("liminal-hq", 160, 40);
    let shown = text(&render(&mut app));
    assert!(
        shown.contains("Keep these 2 comments as a draft?"),
        "{shown}"
    );
    press(&mut app, KeyCode::Enter);
    let shown = text(&render(&mut app));
    assert!(shown.contains("✎ 2"), "{shown}");
}

#[test]
fn a_restored_draft_says_so_in_the_footer() {
    let mut app = restored("liminal-hq", 160, 40);
    let shown = text(&render(&mut app));
    assert!(shown.contains("Draft restored · 2 comments"), "{shown}");
    assert!(shown.contains("This could return early."), "{shown}");
}
