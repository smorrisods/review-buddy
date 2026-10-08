//! Selecting text in the diff (issue 140) against a dedicated patch built through
//! `DiffData::new`, so wrapped lines, wide characters, threads and suggestions can be aimed at
//! without touching the demo patches and their snapshots.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_core::{
    ChangeId, Comment, CommentId, DraftComment, FilePatch, FileStatus, ForgeKind, ReviewDraft,
    Side, SourceId, Thread, ThreadId, Timestamp,
};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Cmd, DiffData, Msg, Screen};
use review_buddy::ui::{self, HitMap};

const PATCH: &str = "@@ -1,6 +1,6 @@ fn long()
 let short = 1;
-let removed = \"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon\";
+let added = \"w000 w001 w002 w003 w004 w005 w006 w007 w008 w009 w010 w011 w012 w013 w014 w015 w016 w017 w018 w019 w020 w021\";
 \tlet tabbed = 1;
 let cjk = \"日本語のとても長い文字列がここに入ります。日本語のとても長い文字列がここに入ります。\";
 let end = 2;";

const ADDED: &str = "let added = \"w000 w001 w002 w003 w004 w005 w006 w007 w008 w009 w010 w011 w012 w013 w014 w015 w016 w017 w018 w019 w020 w021\";";

fn id() -> ChangeId {
    ChangeId {
        source_id: SourceId("s".into()),
        kind: ForgeKind::GitHub,
        repo: "o/r".into(),
        number: 1,
    }
}

fn thread(line: u32) -> Thread {
    Thread {
        id: ThreadId("t1".into()),
        path: Some("src/wrap.rs".into()),
        line: Some(line),
        side: Side::New,
        start_line: None,
        start_side: None,
        pending: false,
        resolved: false,
        outdated: false,
        comments: vec![
            Comment {
                id: CommentId("c1".into()),
                author: "jo".into(),
                body: "Should this wrap round to the first item? Most menus I use stop at the end."
                    .into(),
                created_at: Timestamp(0),
                pending: false,
            },
            Comment {
                id: CommentId("c2".into()),
                author: "ada".into(),
                body: "Yes.".into(),
                created_at: Timestamp(3_600),
                pending: false,
            },
        ],
    }
}

fn suggestion(line: u32) -> DraftComment {
    DraftComment {
        path: "src/wrap.rs".into(),
        side: Side::New,
        start_line: None,
        line,
        body: "Shorter:\n\n```suggestion\nlet cjk = \"短い\";\n```".into(),
    }
}

fn app(size: (u16, u16), wrap: bool, blocks: bool) -> App {
    let mut app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size,
    });
    app.diff_wrap = wrap;
    app.demo = true;
    let (threads, draft) = if blocks {
        (
            vec![thread(2)],
            ReviewDraft {
                body: String::new(),
                comments: vec![suggestion(4)],
            },
        )
    } else {
        (Vec::new(), ReviewDraft::default())
    };
    let data = DiffData::new(
        vec![FilePatch {
            path: "src/wrap.rs".into(),
            old_path: None,
            status: FileStatus::Modified,
            adds: 1,
            dels: 1,
            patch: Some(PATCH.to_string()),
        }],
        threads,
        draft,
    );
    app.diff = Some(review_buddy::app::DiffState::loading(id()));
    app.screen = Screen::Diff;
    update(
        &mut app,
        Msg::DiffLoaded {
            id: id(),
            result: Ok(Box::new(data)),
        },
    );
    render(&mut app);
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

fn row_text(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

/// The first cell and the screen row of `needle`, counting cells (wide characters take two).
fn find(buffer: &Buffer, needle: &str) -> (u16, u16) {
    (0..buffer.area.height)
        .find_map(|y| {
            let line = row_text(buffer, y);
            line.find(needle)
                .map(|byte| (line[..byte].chars().count() as u16, y))
        })
        .unwrap_or_else(|| panic!("{needle:?} not on screen"))
}

fn send(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) -> Vec<Cmd> {
    update(
        app,
        Msg::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }),
    )
}

fn drag(app: &mut App, from: (u16, u16), to: (u16, u16)) -> Vec<Cmd> {
    send(app, MouseEventKind::Down(MouseButton::Left), from);
    render(app);
    send(app, MouseEventKind::Drag(MouseButton::Left), to);
    render(app);
    send(app, MouseEventKind::Up(MouseButton::Left), to)
}

fn key(app: &mut App, c: char) -> Vec<Cmd> {
    update(
        app,
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
    )
}

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn copied(cmds: &[Cmd]) -> Option<String> {
    cmds.iter().find_map(|c| match c {
        Cmd::CopySelection(text) => Some(text.clone()),
        _ => None,
    })
}

fn go_to(app: &mut App, starts_with: &str) {
    for _ in 0..40 {
        let s = app.diff_state().unwrap();
        let id = s.view.rows.line_id(s.view.cursor).unwrap();
        let text = &s
            .current()
            .unwrap()
            .diff
            .parsed()
            .unwrap()
            .line(id)
            .unwrap()
            .text;
        if text.starts_with(starts_with) {
            return;
        }
        key(app, 'j');
    }
    panic!("no line starting with {starts_with:?}");
}

#[test]
fn wrapped_rows_copy_as_one_line_without_a_newline() {
    let mut a = app((100, 30), true, false);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let added");
    // From the `a` of `added` on the first row to well into the third row.
    let cmds = drag(&mut a, (x + 4, y), (x + 20, y + 2));
    let text = copied(&cmds).expect("copied");
    assert!(!text.contains('\n'), "soft-wrapped rows join: {text:?}");
    assert!(ADDED.contains(&text), "exactly the source text: {text:?}");
    assert!(
        text.starts_with("added") && text.contains("w010"),
        "{text:?}"
    );
    assert!(!text.contains('↪'), "no wrap markers: {text:?}");
}

#[test]
fn the_same_drag_with_wrap_off_stays_on_one_row() {
    let mut a = app((100, 30), false, false);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let added");
    let cmds = drag(&mut a, (x + 4, y), (x + 20, y));
    assert_eq!(copied(&cmds).as_deref(), Some("added = \"w000 w00"));
}

#[test]
fn a_triple_click_takes_the_whole_wrapped_line() {
    let mut a = app((100, 30), true, false);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let added");
    let at = (x + 5, y + 1);
    let mut last = Vec::new();
    for _ in 0..3 {
        send(&mut a, MouseEventKind::Down(MouseButton::Left), at);
        render(&mut a);
        last = send(&mut a, MouseEventKind::Up(MouseButton::Left), at);
    }
    assert_eq!(copied(&last).as_deref(), Some(ADDED));
}

#[test]
fn wide_characters_stay_whole_in_a_wrapped_cjk_line() {
    let mut a = app((100, 30), true, false);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let cjk");
    let cmds = drag(&mut a, (x, y), (x + 12, y + 1));
    let text = copied(&cmds).expect("copied");
    assert!(text.starts_with("let cjk"), "{text:?}");
    assert!(text.contains("日本語"), "{text:?}");
    assert!(
        !text.contains('\u{fffd}') && !text.contains('\n'),
        "{text:?}"
    );
    let whole = "let cjk = \"日本語のとても長い文字列がここに入ります。日本語のとても長い文字列がここに入ります。\";";
    assert!(whole.contains(&text), "{text:?}");
}

#[test]
fn a_drag_in_a_thread_copies_its_text_without_the_border() {
    let mut a = app((160, 40), false, true);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "Should this wrap");
    let cmds = drag(&mut a, (x, y), (x + 200, y));
    let text = copied(&cmds).expect("copied");
    assert_eq!(
        text,
        "Should this wrap round to the first item? Most menus I use stop at the end."
    );
}

#[test]
fn a_drag_in_a_thread_stays_in_that_thread() {
    let mut a = app((160, 40), false, true);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "Should this wrap");
    // Pulling the pointer up into the code and down into the suggestion changes nothing.
    let cmds = drag(&mut a, (x + 7, y), (60, y - 4));
    let text = copied(&cmds).expect("copied");
    assert!(text.starts_with("jo"), "{text:?}");
    assert!(
        text.ends_with("this wrap") || text.contains("jo · "),
        "{text:?}"
    );
    assert!(!text.contains("let "), "never code: {text:?}");
}

#[test]
fn a_suggestion_copies_without_its_plus_and_minus_markers() {
    let mut a = app((160, 40), false, true);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "Shorter:");
    let (_, last) = find(&buffer, "+ let cjk");
    let cmds = drag(&mut a, (x, y), (x + 100, last));
    let text = copied(&cmds).expect("copied");
    assert!(text.starts_with("Shorter:"), "{text:?}");
    assert!(text.contains("let cjk = \"短い\";"), "{text:?}");
    assert!(
        !text
            .lines()
            .any(|l| l.starts_with("- ") || l.starts_with("+ ")),
        "{text:?}"
    );
}

#[test]
fn code_selection_skips_comment_blocks_between_lines() {
    let mut a = app((160, 40), false, true);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let added");
    let (_, to) = find(&buffer, "let cjk");
    let cmds = drag(&mut a, (x, y), (x + 8, to));
    let text = copied(&cmds).expect("copied");
    assert!(
        !text.contains("Should this") && !text.contains("jo ·"),
        "{text:?}"
    );
    assert!(
        text.starts_with("let added") && text.contains("let tabbed"),
        "{text:?}"
    );
}

#[test]
fn a_tab_expands_in_the_copied_code_as_it_looks() {
    let mut a = app((160, 40), false, false);
    let buffer = render(&mut a);
    let (_, y) = find(&buffer, "let tabbed");
    // Press at the very left of the code column on the tabbed line.
    let (code_x, _) = find(&buffer, "let short");
    let cmds = drag(&mut a, (code_x, y), (code_x + 30, y));
    let text = copied(&cmds).unwrap();
    assert!(text.starts_with("    let tabbed"), "{text:?}");
}

#[test]
fn y_copies_nothing_when_the_click_never_moved_and_the_link_otherwise() {
    let mut a = app((160, 40), false, false);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let short");
    drag(&mut a, (x, y), (x, y));
    let cmds = key(&mut a, 'y');
    assert!(copied(&cmds).is_none());
}

#[test]
fn big_y_copies_the_whole_thread_and_says_when_there_is_none() {
    let mut a = app((160, 40), false, true);
    go_to(&mut a, "let added");
    let text = copied(&key(&mut a, 'Y')).expect("the thread");
    assert!(text.starts_with("jo · "), "{text:?}");
    assert!(
        text.contains("Should this wrap round to the first item?"),
        "{text:?}"
    );
    assert!(text.contains("\n\nada · "), "{text:?}");
    assert!(text.ends_with("Yes."), "{text:?}");

    key(&mut a, 'g');
    let cmds = key(&mut a, 'Y');
    assert!(copied(&cmds).is_none());
    assert!(a
        .status
        .as_ref()
        .unwrap()
        .notice
        .text
        .contains("no comment on this line"));

    go_to(&mut a, "let cjk");
    let draft = copied(&key(&mut a, 'Y')).expect("the suggestion as written");
    assert_eq!(draft, "Shorter:\n\n```suggestion\nlet cjk = \"短い\";\n```");
}

#[test]
fn copy_mode_selects_with_hjkl_and_y_copies() {
    let mut a = app((160, 40), false, false);
    go_to(&mut a, "let short");
    render(&mut a);
    key(&mut a, 'v');
    assert!(a.selection.as_ref().is_some_and(|s| s.keyboard));
    for _ in 0..3 {
        key(&mut a, 'l');
        render(&mut a);
    }
    key(&mut a, 'j');
    render(&mut a);
    let text = copied(&key(&mut a, 'y')).expect("copied");
    // The start cell, three more, then the next line up to the same column.
    assert!(text.starts_with("let short = 1;\n"), "{text:?}");
    assert!(a.selection.is_none(), "y ends copy mode");
}

#[test]
fn copy_mode_word_keys_and_esc() {
    let mut a = app((160, 40), false, false);
    go_to(&mut a, "let short");
    render(&mut a);
    key(&mut a, 'v');
    key(&mut a, 'w');
    key(&mut a, 'w');
    let text = copied(&key(&mut a, 'y')).expect("copied");
    assert_eq!(text, "let short =");

    go_to(&mut a, "let short");
    render(&mut a);
    key(&mut a, 'v');
    let cmds = press(&mut a, KeyCode::Esc);
    assert!(a.selection.is_none() && cmds.is_empty());
    assert_eq!(a.screen, Screen::Diff, "esc only cancelled copy mode");
}

#[test]
fn copy_mode_tab_moves_into_the_comment_under_the_line() {
    let mut a = app((160, 40), false, true);
    go_to(&mut a, "let added");
    render(&mut a);
    key(&mut a, 'v');
    press(&mut a, KeyCode::Tab);
    render(&mut a);
    key(&mut a, '$');
    let text = copied(&key(&mut a, 'y')).expect("copied");
    assert!(
        text.starts_with("jo · ") && !text.contains('\n'),
        "{text:?}"
    );
}

#[test]
fn copy_mode_swallows_other_keys() {
    let mut a = app((160, 40), false, false);
    go_to(&mut a, "let short");
    render(&mut a);
    let before = a.diff_state().unwrap().view.cursor;
    key(&mut a, 'v');
    key(&mut a, 'c');
    key(&mut a, 'q');
    assert_eq!(a.screen, Screen::Diff);
    assert!(a.diff_state().unwrap().composer.is_none());
    assert_eq!(a.diff_state().unwrap().view.cursor, before);
}

#[test]
fn the_selection_is_dropped_when_the_screen_changes_under_it() {
    let mut a = app((100, 30), true, false);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let added");
    drag(&mut a, (x + 4, y), (x + 20, y + 1));
    assert!(a.selection.is_some());
    key(&mut a, 'z');
    assert!(a.selection.is_none(), "toggling wrap re-flows the rows");

    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let added");
    drag(&mut a, (x + 4, y), (x + 14, y));
    assert!(a.selection.is_some());
    update(&mut a, Msg::Resize(120, 30));
    assert!(a.selection.is_none());
}

#[test]
fn m_turns_the_mouse_off_and_back_on() {
    let mut a = app((160, 40), false, false);
    let cmds = key(&mut a, 'M');
    assert!(matches!(cmds.first(), Some(Cmd::SetMouse(false))));
    assert!(!a.mouse);
    a.status = None;
    let shown: String = {
        let b = render(&mut a);
        (0..b.area.height).map(|y| row_text(&b, y)).collect()
    };
    assert!(shown.contains("mouse off"), "the footer says so");
    let cmds = key(&mut a, 'M');
    assert!(matches!(cmds.first(), Some(Cmd::SetMouse(true))));
    assert!(a.mouse);
}

#[test]
fn m_clears_a_selection_it_can_no_longer_serve() {
    let mut a = app((100, 30), false, false);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let short");
    drag(&mut a, (x, y), (x + 5, y));
    assert!(a.selection.is_some());
    key(&mut a, 'M');
    assert!(a.selection.is_none());
}

#[test]
fn the_first_text_press_hints_once() {
    let mut a = app((160, 40), false, false);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let short");
    send(&mut a, MouseEventKind::Down(MouseButton::Left), (x, y));
    let hint = a.status.as_ref().map(|e| e.notice.text.clone()).unwrap();
    assert!(hint.contains("Drag code to copy text"), "{hint}");
    a.status = None;
    send(&mut a, MouseEventKind::Up(MouseButton::Left), (x, y));
    send(&mut a, MouseEventKind::Down(MouseButton::Left), (x, y));
    assert!(a.status.is_none(), "only the first time");
}

#[test]
fn the_composer_keeps_its_own_mouse_and_never_starts_a_selection() {
    let mut a = app((160, 40), false, true);
    go_to(&mut a, "let added");
    key(&mut a, 'c');
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "let short");
    let cmds = drag(&mut a, (x, y), (x + 8, y));
    assert!(copied(&cmds).is_none());
    assert!(a.selection.is_none());
}
