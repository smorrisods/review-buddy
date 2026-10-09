//! The Conversation tab: every thread with its location, every comment with author, age and full
//! body, resolved threads folded, a thread cursor with `n` `N` `z` `Z` `c` `⏎`, scrolling over a
//! long conversation, and selectable comment text. The snapshots use the demo change "Tidy zsh
//! startup" (three threads, replies, a resolved one and a long body); the rest build their own
//! threads on the same change so the demo data stays as it is. See `docs/testing.md`.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use rb_core::{ChangeId, Comment, CommentId, Side, Thread, ThreadId, Timestamp};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Cmd, Intent, Msg, Pane, Screen, Tab};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::{detail, textmap::RegionKey};

#[path = "support/render.rs"]
mod render_support;
use render_support::{find, plain_dump, render, rows, text};

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
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

fn click(app: &mut App, column: u16, row: u16) {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
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
}

fn world() -> DemoWorld {
    DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap()
}

/// The demo dashboard with change `number` selected, on the Conversation tab, Detail focused.
fn screen_for(number: u64, theme: &str, (w, h): (u16, u16), no_color: bool) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color,
        size: (w, h),
    });
    let snapshot = block_on(world().snapshot()).unwrap();
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    for _ in 0..30 {
        if app.selected_change().is_some_and(|c| c.id.number == number) {
            break;
        }
        key(&mut app, 'j');
    }
    assert_eq!(app.selected_change().unwrap().id.number, number);
    for _ in 0..3 {
        key(&mut app, ']');
    }
    assert_eq!(app.dashboard.tab, Tab::Conversation);
    app.dashboard.focus = Pane::Detail;
    app
}

fn zsh(theme: &str, size: (u16, u16), no_color: bool) -> App {
    screen_for(31, theme, size, no_color)
}

fn id_of(app: &App) -> ChangeId {
    app.selected_change().unwrap().id.clone()
}

fn set_threads(app: &mut App, threads: Vec<Thread>) {
    let id = id_of(app);
    app.state.details.get_mut(&id).unwrap().threads = threads;
}

fn comment(app: &App, n: usize, author: &str, secs_ago: i64, body: &str) -> Comment {
    Comment {
        id: CommentId::new(format!("c{n}-{author}")),
        author: author.into(),
        body: body.into(),
        created_at: Timestamp(app.state.now.unwrap().0 - secs_ago),
        pending: false,
    }
}

fn thread(id: &str, place: Option<(&str, u32)>, comments: Vec<Comment>) -> Thread {
    Thread {
        id: ThreadId::new(id),
        path: place.map(|(p, _)| p.to_string()),
        line: place.map(|(_, l)| l),
        side: Side::New,
        start_line: None,
        start_side: None,
        resolved: false,
        outdated: false,
        pending: false,
        comments,
    }
}

fn dump(app: &mut App) -> String {
    text(&render(app))
}

// Snapshots of the demo conversation.

fn demo_frame(theme: &str, size: (u16, u16)) -> String {
    let mut app = zsh(theme, size, false);
    if size.0 >= 160 {
        key(&mut app, 'n');
    }
    dump(&mut app)
}

#[test]
fn conversation_160x40_default_theme() {
    insta::assert_snapshot!(demo_frame("liminal-hq", (160, 40)));
}

#[test]
fn conversation_160x40_dusk() {
    insta::assert_snapshot!(demo_frame("dusk", (160, 40)));
}

#[test]
fn conversation_100x30_default_theme() {
    insta::assert_snapshot!(demo_frame("liminal-hq", (100, 30)));
}

#[test]
fn conversation_100x30_dusk() {
    insta::assert_snapshot!(demo_frame("dusk", (100, 30)));
}

#[test]
fn conversation_160x40_no_colour() {
    let mut app = zsh("liminal-hq", (160, 40), true);
    key(&mut app, 'n');
    insta::assert_snapshot!(plain_dump(&mut app));
}

#[test]
fn conversation_100x30_no_colour() {
    let mut app = zsh("liminal-hq", (100, 30), true);
    insta::assert_snapshot!(plain_dump(&mut app));
}

#[test]
fn conversation_160x40_unfocused_pane_says_how_to_focus_it() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    app.dashboard.focus = Pane::Queue;
    insta::assert_snapshot!(dump(&mut app));
}

// Layout.

#[test]
fn every_comment_shows_author_age_and_full_body() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    let shown = dump(&mut app);
    assert!(shown.contains("5 comments in 3 threads"), "{shown}");
    assert!(shown.contains(".zshrc:9"));
    assert!(shown.contains("ada · 1d ago") || shown.contains("resolved"));
    key(&mut app, 'n');
    let shown = dump(&mut app);
    assert!(shown.contains("ada · 10h ago"), "{shown}");
    assert!(shown.contains("Looks good overall."));
    assert!(shown.contains("The dump goes stale when fpath changes."));
    assert!(
        !shown.contains("Would a `rehash-completions`") || shown.contains("jo · 2h ago"),
        "{shown}"
    );
}

#[test]
fn resolved_threads_start_folded_with_a_label_in_words() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    let shown = dump(&mut app);
    let header = shown.lines().find(|l| l.contains(".zshrc:9")).unwrap();
    assert!(header.contains('▸'), "{header}");
    assert!(header.contains("resolved"), "{header}");
    assert!(header.contains("folded, z expands"), "{header}");
    assert!(
        shown.contains("ada: Does -C skip the security check?"),
        "the first line stays visible: {shown}"
    );
    assert!(!shown.contains("It does. I'll note that"), "{shown}");
}

#[test]
fn z_unfolds_and_folds_the_thread_under_the_cursor() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    key(&mut app, 'z');
    let shown = dump(&mut app);
    assert!(
        shown.contains("It does. I'll note that in the file."),
        "{shown}"
    );
    assert!(shown.contains("smorris · 1d ago"));
    assert!(shown
        .lines()
        .any(|l| l.contains(".zshrc:9") && l.contains('▾')));
    key(&mut app, 'z');
    assert!(!dump(&mut app).contains("It does. I'll note that"));
}

#[test]
fn big_z_folds_everything_then_unfolds_everything() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    key(&mut app, 'Z');
    let folded = dump(&mut app);
    let headers = |shown: &str, glyph: char| {
        shown
            .lines()
            .filter(|l| l.contains(glyph) && (l.contains(".zshrc:9") || l.contains("general")))
            .count()
    };
    assert_eq!(headers(&folded, '▸'), 3, "{folded}");
    assert!(!folded.contains("The dump goes stale"));
    key(&mut app, 'Z');
    let threads = app.state.details[&id_of(&app)].threads.clone();
    assert!(threads.iter().all(|t| !detail::thread_folded(&app, t)));
    assert!(dump(&mut app).contains("Looks good overall."));
}

#[test]
fn the_cursor_is_marked_in_text_and_n_and_big_n_move_it() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    let on = |shown: &str| -> Vec<String> {
        shown
            .lines()
            .filter(|l| l.contains('›'))
            .map(|l| l.to_string())
            .collect()
    };
    let first = on(&dump(&mut app));
    assert_eq!(first.len(), 1);
    assert!(first[0].contains(".zshrc:9"));
    key(&mut app, 'n');
    let second = on(&dump(&mut app));
    assert_eq!(second.len(), 1);
    assert!(second[0].contains("general"), "{second:?}");
    key(&mut app, 'N');
    assert!(on(&dump(&mut app))[0].contains(".zshrc:9"));
    key(&mut app, 'N');
    assert_eq!(app.dashboard.thread, 0, "N stops at the first thread");
    for _ in 0..9 {
        key(&mut app, 'n');
    }
    assert_eq!(app.dashboard.thread, 2, "n stops at the last thread");
}

#[test]
fn outdated_pending_and_range_threads_are_labelled_in_words() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    let mut old = thread(
        "t-old",
        Some(("src/ui/menus.rs", 7)),
        vec![comment(&app, 1, "jo", 7200, "Is this still needed?")],
    );
    old.outdated = true;
    let mut range = thread(
        "t-range",
        Some(("src/ui/menubar.rs", 7)),
        vec![comment(&app, 2, "ada", 3600, "These three lines.")],
    );
    range.start_line = Some(5);
    let mut draft = thread(
        "t-draft",
        None,
        vec![comment(&app, 3, "smorris", 60, "Not sent yet.")],
    );
    draft.pending = true;
    draft.comments[0].pending = true;
    set_threads(&mut app, vec![old, range, draft]);
    let shown = dump(&mut app);
    let line = |needle: &str| {
        shown
            .lines()
            .find(|l| l.contains(needle))
            .unwrap()
            .to_string()
    };
    assert!(line("menus.rs:7").contains("outdated"), "{shown}");
    assert!(line("menubar.rs:5–7").contains("1 comment"), "{shown}");
    assert!(line("general").contains("pending"), "{shown}");
    assert!(shown.contains("smorris · 1m ago · pending"), "{shown}");
}

#[test]
fn markdown_lists_code_and_images_read_like_the_description() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    let body = "Two things:\n\n- first uses `Vec::new`\n- second\n\n```rust\nlet x = 1;\n```\n\n![before](https://example.invalid/a.png)";
    let c = comment(&app, 1, "ada", 600, body);
    set_threads(&mut app, vec![thread("t", None, vec![c])]);
    let shown = dump(&mut app);
    assert!(shown.contains("Two things:"), "{shown}");
    assert!(shown.contains("first uses `Vec::new`") || shown.contains("first uses Vec::new"));
    assert!(shown.contains("let x = 1;"));
    assert!(shown.contains("▣ image: before"), "{shown}");
    assert!(
        !shown.contains("```"),
        "fences aren't shown as markup: {shown}"
    );
}

#[test]
fn wide_characters_and_long_words_fit_the_pane() {
    let mut app = zsh("liminal-hq", (100, 30), false);
    let long = "x".repeat(300);
    let body = format!("日本語のコメントがとても長い場合でも折り返されます。{long}");
    let c = comment(&app, 1, "kai", 600, &body);
    set_threads(&mut app, vec![thread("t", None, vec![c])]);
    let buffer = render(&mut app);
    for (y, row) in rows(&buffer).iter().enumerate() {
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(row.as_str()),
            100,
            "row {y} keeps the frame's width"
        );
    }
}

#[test]
fn a_change_with_no_comments_says_so() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    set_threads(&mut app, vec![]);
    assert!(dump(&mut app).contains("No comments yet."));
    key(&mut app, 'n');
    key(&mut app, 'z');
    assert!(dump(&mut app).contains("No comments yet."));
}

// Keys that lead into the diff.

fn load_diff(app: &mut App) {
    let id = id_of(app);
    let data = block_on(world().diff_data(&id)).unwrap();
    update(
        app,
        Msg::DiffLoaded {
            id,
            result: Ok(Box::new(data)),
        },
    );
}

fn menus() -> App {
    screen_for(214, "liminal-hq", (160, 40), false)
}

#[test]
fn enter_opens_the_diff_on_the_threads_line() {
    let mut app = menus();
    let first = app.state.details[&id_of(&app)].threads[0].id.clone();
    let cmds = press(&mut app, KeyCode::Enter);
    assert!(matches!(cmds.first(), Some(Cmd::LoadDiff(_))), "{cmds:?}");
    assert_eq!(app.screen, Screen::Diff);
    assert_eq!(app.diff.as_ref().unwrap().intent, Some(Intent::Goto(first)));
    load_diff(&mut app);
    let state = app.diff.as_ref().unwrap();
    assert!(state.intent.is_none());
    assert_eq!(state.current().unwrap().diff.path, "src/ui/menus.rs");
    let want = state
        .current()
        .unwrap()
        .diff
        .parsed()
        .unwrap()
        .find_by_anchor(rb_diff::Anchor {
            side: Side::New,
            line: 44,
        })
        .unwrap();
    assert_eq!(state.view.rows.line_id(state.view.cursor), Some(want));
    assert!(state.composer.is_none());
}

#[test]
fn enter_on_the_second_thread_goes_to_that_threads_file() {
    let mut app = menus();
    let threads = app.state.details[&id_of(&app)].threads.clone();
    let at = threads
        .iter()
        .position(|t| t.path.as_deref() == Some("src/ui/menubar.rs"))
        .unwrap();
    for _ in 0..at {
        key(&mut app, 'n');
    }
    press(&mut app, KeyCode::Enter);
    load_diff(&mut app);
    assert_eq!(
        app.diff.as_ref().unwrap().current().unwrap().diff.path,
        "src/ui/menubar.rs"
    );
}

#[test]
fn c_replies_to_the_thread_under_the_cursor() {
    let mut app = menus();
    let first = app.state.details[&id_of(&app)].threads[0].id.clone();
    key(&mut app, 'c');
    assert_eq!(app.screen, Screen::Diff);
    assert_eq!(
        app.diff.as_ref().unwrap().intent,
        Some(Intent::Reply(first.clone()))
    );
    load_diff(&mut app);
    let state = app.diff.as_ref().unwrap();
    let composer = state.composer.as_ref().expect("the reply box is open");
    assert_eq!(composer.title(), "Reply · menus.rs line 44");
    assert!(matches!(
        &composer.target,
        review_buddy::app::composer::Target::Reply { thread, .. } if *thread == first
    ));
}

#[test]
fn replying_to_a_general_thread_opens_the_reply_box_without_moving() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    key(&mut app, 'n');
    key(&mut app, 'c');
    load_diff(&mut app);
    let composer = app.diff.as_ref().unwrap().composer.as_ref().unwrap();
    assert_eq!(composer.title(), "Reply · general");
}

#[test]
fn enter_on_a_general_thread_opens_the_diff_and_says_it_has_no_line() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    key(&mut app, 'n');
    press(&mut app, KeyCode::Enter);
    load_diff(&mut app);
    assert_eq!(app.screen, Screen::Diff);
    let shown = dump(&mut app);
    assert!(
        shown.contains("general comment, so it has no line"),
        "{shown}"
    );
}

#[test]
fn from_the_queue_c_and_enter_keep_their_old_meaning() {
    let mut app = menus();
    app.dashboard.focus = Pane::Queue;
    key(&mut app, 'c');
    assert_eq!(app.diff.as_ref().unwrap().intent, Some(Intent::Comment));
    let mut app = menus();
    app.dashboard.focus = Pane::Queue;
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.diff.as_ref().unwrap().intent, None);
}

#[test]
fn clicking_a_header_selects_and_folds_that_thread() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    let buffer = render(&mut app);
    let (x, y) = find(&buffer, "general").expect("a general thread header");
    click(&mut app, x, y);
    assert_eq!(app.dashboard.thread, 1);
    assert!(
        !dump(&mut app).contains("The dump goes stale"),
        "folded by the click"
    );
    let buffer = render(&mut app);
    let (x, y) = find(&buffer, "general").unwrap();
    click(&mut app, x, y);
    assert!(dump(&mut app).contains("The dump goes stale"));
}

// Scrolling and the scroll limit.

fn long_conversation(app: &mut App, threads: usize) {
    let mut all = Vec::new();
    let mut n = 0;
    for t in 0..threads {
        let mut comments = Vec::new();
        for c in 0..4 {
            n += 1;
            let body = format!("comment number {n} in thread {t}, saying something long enough to wrap on a narrow pane when it keeps going for a while.");
            comments.push(comment(app, n, "ada", 3600 * (1000 - n as i64), &body));
            let _ = c;
        }
        all.push(thread(
            &format!("t{t}"),
            Some(("a.rs", t as u32 + 1)),
            comments,
        ));
    }
    set_threads(app, all);
}

#[test]
fn a_long_conversation_scrolls_to_its_last_line_and_no_further() {
    let mut app = zsh("liminal-hq", (100, 30), false);
    long_conversation(&mut app, 100);
    let max = detail::max_scroll(&app);
    assert!(
        max > 1000,
        "400 comments are far taller than the pane: {max}"
    );
    key(&mut app, 'G');
    assert_eq!(app.dashboard.detail_scroll, max);
    let shown = dump(&mut app);
    assert!(shown.contains("comment number 400 in thread 99"), "{shown}");
    key(&mut app, 'j');
    assert_eq!(app.dashboard.detail_scroll, max, "j stops at the end");
    key(&mut app, 'g');
    assert_eq!(app.dashboard.detail_scroll, 0);
    assert!(dump(&mut app).contains("400 comments in 100 threads"));
}

#[test]
fn n_scrolls_the_next_thread_into_view() {
    let mut app = zsh("liminal-hq", (100, 30), false);
    long_conversation(&mut app, 30);
    for _ in 0..12 {
        key(&mut app, 'n');
    }
    assert_eq!(app.dashboard.thread, 12);
    let shown = dump(&mut app);
    assert!(shown.contains("a.rs:13"), "{shown}");
    assert!(shown
        .lines()
        .any(|l| l.contains('›') && l.contains("a.rs:13")));
    for _ in 0..12 {
        key(&mut app, 'N');
    }
    assert_eq!(app.dashboard.detail_scroll, 0);
}

#[test]
fn folding_never_leaves_the_scroll_past_the_end() {
    let mut app = zsh("liminal-hq", (100, 30), false);
    long_conversation(&mut app, 30);
    key(&mut app, 'G');
    key(&mut app, 'Z');
    assert!(app.dashboard.detail_scroll <= detail::max_scroll(&app));
    assert!(
        dump(&mut app).contains("a.rs:1 "),
        "the cursor thread stays in view"
    );
}

#[test]
fn a_long_conversation_draws_quickly() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    long_conversation(&mut app, 250);
    let start = std::time::Instant::now();
    for _ in 0..5 {
        render(&mut app);
    }
    let per_frame = start.elapsed() / 5;
    assert!(
        per_frame < std::time::Duration::from_millis(400),
        "1,000 comments took {per_frame:?} a frame"
    );
}

// Selecting and copying.

fn comment_region(app: &App, n: u32) -> review_buddy::ui::textmap::TextRegion {
    app.hits
        .texts
        .region(RegionKey::Comment(n))
        .unwrap_or_else(|| panic!("comment {n} is a text region"))
        .clone()
}

#[test]
fn each_comment_is_a_region_and_wrapped_rows_join_into_its_paragraphs() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    let body = "Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega alpha beta gamma delta.\n\nSecond paragraph.";
    let c = comment(&app, 1, "ada", 600, body);
    let d = comment(&app, 2, "jo", 300, "A short reply.");
    set_threads(&mut app, vec![thread("t", None, vec![c, d])]);
    render(&mut app);
    let first = comment_region(&app, 0);
    assert!(first.rows.len() >= 3, "{:?}", first.rows);
    let joined = review_buddy::ui::textmap::Join::Space;
    assert!(first.rows[1..].iter().any(|r| r.join == joined));
    assert!(first
        .rows
        .iter()
        .all(|r| r.x == 4 + app.hits.texts.region(RegionKey::Comment(0)).unwrap().rect.x));
    assert_eq!(comment_region(&app, 1).rows[0].text, "A short reply.");
}

#[test]
fn dragging_across_a_wrapped_comment_copies_it_as_written_without_the_indent() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    let body = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo";
    let c = comment(&app, 1, "ada", 600, body);
    set_threads(&mut app, vec![thread("t", None, vec![c])]);
    render(&mut app);
    let region = comment_region(&app, 0);
    let first = region.rows.first().unwrap().clone();
    let last = region.rows.last().unwrap().clone();
    let drag = |app: &mut App, kind: MouseEventKind, x: u16, y: u16| {
        update(
            app,
            Msg::Mouse(MouseEvent {
                kind,
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            }),
        );
    };
    drag(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        first.x,
        first.y,
    );
    drag(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        last.x + last.text.chars().count() as u16,
        last.y,
    );
    let cmds = update(
        &mut app,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: last.x + last.text.chars().count() as u16,
            row: last.y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    let copied = cmds.iter().find_map(|c| match c {
        Cmd::CopySelection(t) => Some(t.clone()),
        _ => None,
    });
    assert_eq!(copied.as_deref(), Some(body));
}

#[test]
fn v_starts_copy_mode_on_the_cursor_thread_and_y_copies() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    key(&mut app, 'n');
    render(&mut app);
    key(&mut app, 'v');
    let sel = app.selection.as_ref().expect("copy mode is on");
    assert!(matches!(sel.key, RegionKey::Comment(_)));
    for _ in 0..4 {
        key(&mut app, 'w');
    }
    let cmds = key(&mut app, 'y');
    let copied = cmds.iter().find_map(|c| match c {
        Cmd::CopySelection(t) => Some(t.clone()),
        _ => None,
    });
    assert!(
        copied.is_some_and(|t| t.starts_with("Looks good overall.")),
        "copies from the start of the first comment"
    );
    assert!(app.selection.is_none());
}

#[test]
fn tab_in_copy_mode_moves_to_the_next_comment() {
    let mut app = zsh("liminal-hq", (160, 40), false);
    key(&mut app, 'n');
    render(&mut app);
    key(&mut app, 'v');
    let RegionKey::Comment(from) = app.selection.as_ref().unwrap().key else {
        panic!("a comment region");
    };
    press(&mut app, KeyCode::Tab);
    let RegionKey::Comment(to) = app.selection.as_ref().unwrap().key else {
        panic!("a comment region");
    };
    assert_ne!(from, to);
    press(&mut app, KeyCode::Esc);
    assert!(app.selection.is_none());
}
