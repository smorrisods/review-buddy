//! Soft line wrap in the diff (issue 136): the cursor, ranges and navigation stay on logical
//! lines while the screen rows follow the wrapped text. The fixtures here are built through
//! `DiffData::new` from a dedicated patch, so the demo patches and their snapshots stay as
//! they are.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, Terminal};
use rb_core::{
    ChangeId, Comment, CommentId, DraftComment, FilePatch, FileStatus, ForgeKind, ReviewDraft,
    Side, SourceId, Thread, ThreadId, Timestamp,
};
use rb_theme::ColourDepth;
use review_buddy::app::diffview::{self, GUTTER};
use review_buddy::app::{update, App, AppConfig, Cmd, DiffData, Msg, Screen};
use review_buddy::ui::{self, text::row_start, text::wrap_points, HitMap};

const PLAIN: &str = "@@ -1,6 +1,6 @@ fn long()
 let short = 1;
-let removed = \"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon\";
+let added = \"w000 w001 w002 w003 w004 w005 w006 w007 w008 w009 w010 w011 w012 w013 w014 w015 w016 w017 w018 w019 w020 w021\";
 \tlet tabbed = \"a\tb\tc\td\te\tf\tg\th\ti\tj\tk\tl\tm\tn\to\tp\tq\tr\ts\tt\tu\tv\tw\tx\ty\tz\";
 let cjk = \"日本語のとても長い文字列がここに入ります。日本語のとても長い文字列がここに入ります。日本語のとても長い文字列がここに入ります。\";
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

fn file(patch: String) -> FilePatch {
    FilePatch {
        path: "src/wrap.rs".into(),
        old_path: None,
        status: FileStatus::Modified,
        adds: 1,
        dels: 1,
        patch: Some(patch),
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
        comments: vec![Comment {
            id: CommentId("c1".into()),
            author: "jo".into(),
            body: "Should this wrap round?".into(),
            created_at: Timestamp(0),
            pending: false,
        }],
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

fn app_with(patch: &str, size: (u16, u16), theme: &str, wrap: bool, blocks: bool) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
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
    let data = DiffData::new(vec![file(patch.to_string())], threads, draft);
    let mut state = review_buddy::app::DiffState::loading(id());
    state.phase = review_buddy::app::Phase::Loading;
    app.diff = Some(state);
    app.screen = Screen::Diff;
    update(
        &mut app,
        Msg::DiffLoaded {
            id: id(),
            result: Ok(Box::new(data)),
        },
    );
    app
}

fn plain(wrap: bool, size: (u16, u16)) -> App {
    app_with(PLAIN, size, "liminal-hq", wrap, false)
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

fn mouse(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) {
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
        .collect::<String>()
}

fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| row_text(buffer, y).trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The screen row and the column of the first cell of `needle`, in cells.
fn find(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    (0..buffer.area.height).find_map(|y| {
        let line = row_text(buffer, y);
        line.find(needle)
            .map(|byte| (line[..byte].chars().count() as u16, y))
    })
}

fn state(app: &App) -> &review_buddy::app::DiffState {
    app.diff.as_ref().unwrap()
}

fn cursor_text(app: &App) -> String {
    let s = state(app);
    let id = s.view.rows.line_id(s.view.cursor).unwrap();
    s.current()
        .unwrap()
        .diff
        .parsed()
        .unwrap()
        .line(id)
        .unwrap()
        .text
        .clone()
}

fn go_to(app: &mut App, starts_with: &str) {
    for _ in 0..40 {
        if cursor_text(app).starts_with(starts_with) {
            return;
        }
        key(app, 'j');
    }
    panic!("no line starting with {starts_with:?}");
}

/// The cursor marker in the diff pane (the Files pane has its own).
fn cursor_rows(buffer: &Buffer) -> Vec<u16> {
    (0..buffer.area.height)
        .filter(|&y| (34..buffer.area.width).any(|x| buffer[(x, y)].symbol() == "›"))
        .collect()
}

#[test]
fn wrap_is_off_by_default_and_long_lines_are_cut_with_an_ellipsis() {
    let mut a = plain(false, (100, 30));
    let buffer = render(&mut a);
    let shown = text(&buffer);
    assert!(!shown.contains('↪'));
    assert!(shown.contains('…'), "{shown}");
    assert!(!shown.contains("w021"));
    let s = state(&a);
    assert!(!s.view.screen.wrapping());
    assert_eq!(s.view.screen.total(), s.view.rows.len());
    assert!(!shown.contains("· wrap"), "no marker while wrap is off");
}

#[test]
fn z_toggles_wrap_and_says_so() {
    let mut a = plain(false, (100, 30));
    key(&mut a, 'z');
    assert!(a.diff_wrap);
    assert_eq!(a.status.as_ref().unwrap().notice.text, "Wrap on");
    let shown = text(&render(&mut a));
    assert!(
        shown.contains("· wrap"),
        "bottom status carries the marker:\n{shown}"
    );
    assert!(shown.contains("w021"), "the whole line is reachable");
    assert!(state(&a).view.screen.wrapping());

    key(&mut a, 'z');
    assert!(!a.diff_wrap);
    assert_eq!(a.status.as_ref().unwrap().notice.text, "Wrap off");
    let shown = text(&render(&mut a));
    assert!(!shown.contains("· wrap"));
    assert!(!shown.contains('↪'));
}

#[test]
fn continuation_rows_sit_under_the_code_column_not_the_gutter() {
    let mut a = plain(true, (100, 30));
    go_to(&mut a, "let added");
    let buffer = render(&mut a);
    let (code_x, y) = find(&buffer, "let added").expect("the line");
    let gutter = |y: u16| -> String {
        (code_x - GUTTER..code_x)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    };
    let first = gutter(y);
    assert!(
        first.contains("2"),
        "line numbers on the first row: {first:?}"
    );
    assert!(first.contains('›') && first.contains('+'), "{first:?}");

    let next = gutter(y + 1);
    assert!(next.contains('↪'), "a continuation marker: {next:?}");
    assert!(next.contains('+'), "the sign continues: {next:?}");
    assert!(!next.contains('›'), "one cursor marker: {next:?}");
    assert!(
        !next.chars().any(|c| c.is_ascii_digit()),
        "no numbers: {next:?}"
    );
    assert!(
        row_text(&buffer, y + 1)
            .chars()
            .skip(usize::from(code_x))
            .collect::<String>()
            .trim_start()
            .starts_with(|c: char| c.is_alphanumeric() || c == '"'),
        "the code carries on under the code column"
    );

    let tint = |y: u16| buffer[(code_x + 3, y)].bg;
    assert_ne!(tint(y + 1), Color::Reset);
    assert_eq!(
        tint(y),
        tint(y + 1),
        "the cursor highlight covers every row"
    );
}

#[test]
fn the_added_tint_and_sign_cover_every_row_of_an_unselected_line() {
    let mut a = plain(true, (100, 30));
    let buffer = render(&mut a);
    let (code_x, y) = find(&buffer, "let added").expect("the line");
    assert_ne!(buffer[(code_x, y)].bg, Color::Reset);
    assert_eq!(buffer[(code_x, y)].bg, buffer[(code_x, y + 1)].bg);
    let removed = find(&buffer, "let removed").expect("removed line").1;
    assert_ne!(buffer[(code_x, removed)].bg, buffer[(code_x, y)].bg);
    assert_eq!(
        buffer[(code_x, removed)].bg,
        buffer[(code_x, removed + 1)].bg
    );
    let sign = |y| buffer[(code_x - 2, y)].symbol().to_string();
    assert_eq!((sign(removed), sign(removed + 1)), ("-".into(), "-".into()));
    assert_eq!((sign(y), sign(y + 1)), ("+".into(), "+".into()));
}

#[test]
fn every_character_survives_the_wrap_in_order() {
    let mut a = plain(true, (100, 30));
    let buffer = render(&mut a);
    let (code_x, y) = find(&buffer, "let added").unwrap();
    let height = state(&a).view.screen.height(
        state(&a)
            .view
            .rows
            .row_of(rb_diff::LineId { hunk: 0, line: 2 })
            .unwrap(),
    );
    assert!(height >= 2);
    let right = ui::layout::diff_screen_with(review_buddy::app::terminal::app_body(&a), 0)
        .code
        .right();
    let rejoined: String = (y..y + height as u16)
        .map(|row| {
            (code_x..right)
                .map(|x| buffer[(x, row)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    assert_eq!(rejoined, ADDED);
}

#[test]
fn tabs_and_wide_characters_never_split_across_rows() {
    let mut a = plain(true, (100, 30));
    let buffer = render(&mut a);
    let shown = text(&buffer);
    // Every wide character is drawn whole: nothing but its own cell pair holds it.
    for line in shown.lines() {
        assert!(!line.contains('\u{fffd}'));
    }
    let s = state(&a);
    let patch = s.current().unwrap().diff.parsed().unwrap();
    let width = usize::from(s.view.width) - usize::from(GUTTER);
    let tabbed = rb_diff::LineId { hunk: 0, line: 3 };
    let raw = &patch.line(tabbed).unwrap().text;
    let expanded = rb_diff::expand_tabs(raw, a.tab_width);
    let points = wrap_points(raw, a.tab_width, width);
    assert!(!points.is_empty());
    let chars: Vec<char> = expanded.chars().collect();
    for p in &points {
        // A break never lands inside a run of spaces that came from one tab, and no row
        // starts with fewer spaces than the tab put there unless the tab itself is wider.
        let before = chars[p - 1];
        let after = chars[*p];
        assert!(
            !(before == ' ' && after == ' ') || a.tab_width as usize > width,
            "split a tab expansion at {p}: {expanded:?}"
        );
    }
    let cjk = rb_diff::LineId { hunk: 0, line: 4 };
    let raw = &patch.line(cjk).unwrap().text;
    let points = wrap_points(raw, a.tab_width, width);
    let mut start = 0;
    for p in points.iter().copied().chain([unicode_width_of(raw)]) {
        assert!(
            p - start <= width,
            "a row of {} cells in {width}",
            p - start
        );
        start = p;
    }
    assert!(shown.contains("日 本 語 の と て も"));
}

fn unicode_width_of(s: &str) -> usize {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

#[test]
fn j_moves_one_line_not_one_row_and_the_cursor_marker_stays_single() {
    let mut a = plain(true, (100, 30));
    let first = state(&a).view.cursor;
    go_to(&mut a, "let removed");
    assert_eq!(cursor_text(&a).split_whitespace().next(), Some("let"));
    key(&mut a, 'j');
    assert!(cursor_text(&a).starts_with("let added"));
    let buffer = render(&mut a);
    assert_eq!(cursor_rows(&buffer).len(), 1, "{}", text(&buffer));
    key(&mut a, 'k');
    key(&mut a, 'k');
    assert_eq!(state(&a).view.cursor, first);
}

#[test]
fn shift_j_extends_a_range_by_logical_lines_and_marks_every_row() {
    let mut a = plain(true, (100, 30));
    go_to(&mut a, "let removed");
    update(
        &mut a,
        Msg::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT)),
    );
    let range = state(&a).range.expect("a range");
    assert_eq!(range.bounds().1 - range.bounds().0, 1, "two logical lines");
    let buffer = render(&mut a);
    let (_, y) = find(&buffer, "let removed").unwrap();
    let (_, y2) = find(&buffer, "let added").unwrap();
    for row in [y, y + 1, y2, y2 + 1] {
        let line = row_text(&buffer, row);
        assert!(
            line.contains('▌') || line.contains('›'),
            "row {row} carries a range or cursor marker: {line}"
        );
    }
}

#[test]
fn clicking_a_continuation_row_selects_its_line() {
    let mut a = plain(true, (100, 30));
    let buffer = render(&mut a);
    let (code_x, y) = find(&buffer, "let added").unwrap();
    mouse(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        (code_x + 4, y + 1),
    );
    mouse(
        &mut a,
        MouseEventKind::Up(MouseButton::Left),
        (code_x + 4, y + 1),
    );
    assert!(cursor_text(&a).starts_with("let added"));
}

#[test]
fn dragging_across_wrapped_lines_selects_logical_lines() {
    let mut a = plain(true, (100, 30));
    let buffer = render(&mut a);
    // A drag that starts in the line-number gutter selects lines; one on the code selects text.
    let (code_x, y) = find(&buffer, "let removed").unwrap();
    let (_, y2) = find(&buffer, "let added").unwrap();
    mouse(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        (code_x - 6, y + 1),
    );
    mouse(
        &mut a,
        MouseEventKind::Drag(MouseButton::Left),
        (code_x - 6, y2 + 1),
    );
    mouse(
        &mut a,
        MouseEventKind::Up(MouseButton::Left),
        (code_x - 6, y2 + 1),
    );
    let range = state(&a).range.expect("a range");
    let rows = &state(&a).view.rows;
    assert_eq!(
        rows.line_id(range.bounds().0),
        Some(rb_diff::LineId { hunk: 0, line: 1 })
    );
    assert_eq!(
        rows.line_id(range.bounds().1),
        Some(rb_diff::LineId { hunk: 0, line: 2 })
    );
}

#[test]
fn threads_and_pending_suggestions_sit_below_the_last_row_of_their_line() {
    let mut a = app_with(PLAIN, (100, 30), "liminal-hq", true, true);
    let buffer = render(&mut a);
    let (_, line_y) = find(&buffer, "let added").unwrap();
    let (_, thread_y) = find(&buffer, "thread · line 2").unwrap();
    let rows = &state(&a).view.rows;
    let at = rows.row_of(rb_diff::LineId { hunk: 0, line: 2 }).unwrap();
    let height = state(&a).view.screen.height(at) as u16;
    assert!(height >= 2);
    assert_eq!(thread_y, line_y + height, "right under the last row");
    let (_, sug_y) = find(&buffer, "pending suggestion · line 4").unwrap();
    let (_, cjk_y) = find(&buffer, "let cjk").unwrap();
    let cjk = rows.row_of(rb_diff::LineId { hunk: 0, line: 4 }).unwrap();
    assert_eq!(sug_y, cjk_y + state(&a).view.screen.height(cjk) as u16);
}

fn wrapped_frame(theme: &str, size: (u16, u16)) -> String {
    let mut a = app_with(PLAIN, size, theme, true, false);
    go_to(&mut a, "let added");
    text(&render(&mut a))
}

#[test]
fn wrap_on_160x40_default_theme() {
    insta::assert_snapshot!(wrapped_frame("liminal-hq", (160, 40)));
}

#[test]
fn wrap_on_160x40_dusk() {
    insta::assert_snapshot!(wrapped_frame("dusk", (160, 40)));
}

#[test]
fn wrap_on_100x30_default_theme() {
    insta::assert_snapshot!(wrapped_frame("liminal-hq", (100, 30)));
}

#[test]
fn wrap_on_100x30_dusk() {
    insta::assert_snapshot!(wrapped_frame("dusk", (100, 30)));
}

#[test]
fn snapshot_with_wrap_on_and_blocks() {
    let mut a = app_with(PLAIN, (100, 30), "liminal-hq", true, true);
    insta::assert_snapshot!(text(&render(&mut a)));
}

fn long_file(lines: usize) -> String {
    let mut patch = format!("@@ -1,{lines} +1,{lines} @@\n");
    for i in 0..lines {
        patch.push_str(&format!(" row{i:03} {}\n", "word ".repeat(40)));
    }
    patch
}

#[test]
fn paging_and_jumps_use_screen_rows_and_keep_the_cursor_in_view() {
    let mut a = app_with(&long_file(60), (100, 30), "liminal-hq", true, false);
    assert!(state(&a).view.screen.total() > 3 * 60);
    let code_height = {
        let l = ui::layout::diff_screen_with(review_buddy::app::terminal::app_body(&a), 0);
        usize::from(l.code.height)
    };
    let mut last = state(&a).view.cursor;
    for _ in 0..12 {
        press(&mut a, KeyCode::PageDown);
        let s = state(&a);
        assert!(s.view.cursor > last, "paging moves forward");
        last = s.view.cursor;
        let (top, bottom) = s.view.rows.reveal_span(s.view.cursor);
        assert!(s.view.screen.start(top) >= s.view.scroll);
        assert!(
            s.view.screen.end(bottom) < s.view.scroll + code_height + 1
                || s.view.screen.start(top) == s.view.scroll,
            "the cursor line is in view"
        );
        assert!(cursor_rows(&render(&mut a)).len() == 1, "marker on screen");
    }
    key(&mut a, 'G');
    let buffer = render(&mut a);
    assert!(text(&buffer).contains("row059"));
    let s = state(&a);
    assert_eq!(
        s.view.scroll,
        diffview::max_scroll(s.view.screen.total(), code_height)
    );
    key(&mut a, 'g');
    assert_eq!(state(&a).view.scroll, 0);
    assert!(text(&render(&mut a)).contains("row000"));
}

#[test]
fn the_wheel_scrolls_screen_rows_and_pulls_the_cursor_along() {
    let mut a = app_with(&long_file(60), (100, 30), "liminal-hq", true, false);
    for _ in 0..8 {
        mouse(&mut a, MouseEventKind::ScrollDown, (60, 10));
    }
    let s = state(&a);
    assert_eq!(s.view.scroll, 24);
    let (row, _) = s.view.screen.locate(s.view.scroll).unwrap();
    assert!(s.view.cursor >= row, "the cursor followed the view");
    assert!(cursor_rows(&render(&mut a)).len() == 1);
}

#[test]
fn resizing_re_wraps_and_keeps_the_cursor_line() {
    let mut a = app_with(PLAIN, (160, 40), "liminal-hq", true, false);
    go_to(&mut a, "let cjk");
    let wide_total = state(&a).view.screen.total();
    update(&mut a, Msg::Resize(100, 30));
    let s = state(&a);
    assert!(s.view.screen.total() > wide_total, "narrower means taller");
    assert!(cursor_text(&a).starts_with("let cjk"));
    assert_eq!(
        s.view.width,
        ui::layout::diff_screen_with(review_buddy::app::terminal::app_body(&a), 0)
            .code
            .width
    );
    let buffer = render(&mut a);
    assert_eq!(
        cursor_rows(&buffer).len(),
        1,
        "the cursor line is on screen"
    );
    update(&mut a, Msg::Resize(160, 40));
    assert_eq!(state(&a).view.screen.total(), wide_total);
    assert!(cursor_text(&a).starts_with("let cjk"));
}

#[test]
fn the_terminal_pane_opening_and_closing_re_wraps() {
    let mut a = app_with(PLAIN, (160, 40), "liminal-hq", true, false);
    let before = (state(&a).view.width, state(&a).view.screen.total());
    key(&mut a, 't');
    assert!(a.term.pane.is_some());
    let docked = (state(&a).view.width, state(&a).view.screen.total());
    assert!(
        docked.0 < before.0 && docked.1 >= before.1,
        "the pane takes width: {before:?} -> {docked:?}"
    );
    review_buddy::app::terminal::close(&mut a);
    update(&mut a, Msg::Tick);
    assert_eq!(
        (state(&a).view.width, state(&a).view.screen.total()),
        before
    );
}

#[test]
fn toggling_keeps_the_cursor_line() {
    let mut a = plain(false, (100, 30));
    go_to(&mut a, "let cjk");
    key(&mut a, 'z');
    assert!(cursor_text(&a).starts_with("let cjk"));
    key(&mut a, 'z');
    assert!(cursor_text(&a).starts_with("let cjk"));
    assert_eq!(cursor_rows(&render(&mut a)).len(), 1);
}

#[test]
fn big_files_still_style_only_the_window_with_wrap_on() {
    let mut patch = String::from("@@ -1,6000 +1,6000 @@\n");
    for i in 1..=6_000 {
        patch.push_str(&format!(" let value_{i} = {i}; // {}\n", "pad ".repeat(25)));
    }
    let mut a = app_with(&patch, (100, 30), "liminal-hq", true, false);
    assert!(
        state(&a).view.screen.total() > 6_000,
        "{} {}",
        state(&a).view.screen.total(),
        state(&a).view.screen.wrapping()
    );
    for _ in 0..40 {
        press(&mut a, KeyCode::PageDown);
    }
    assert!(state(&a).view.chunks() < 20, "{}", state(&a).view.chunks());
    let buffer = render(&mut a);
    assert!(text(&buffer).contains("let value_"));
    assert!(a.hits.len() < 60, "visible rows only, got {}", a.hits.len());
}

#[test]
fn the_screen_to_logical_mapping_agrees_with_what_was_drawn() {
    let mut a = plain(true, (100, 30));
    go_to(&mut a, "let added");
    let buffer = render(&mut a);
    let (code_x, y) = find(&buffer, "let added").unwrap();
    let l = ui::layout::diff_screen_with(review_buddy::app::terminal::app_body(&a), 0);
    let s = state(&a);
    let at = s
        .view
        .rows
        .row_of(rb_diff::LineId { hunk: 0, line: 2 })
        .unwrap();
    let hit = diffview::hit_at(&s.view.screen, l.code, s.view.scroll, code_x + 5, y + 1).unwrap();
    assert_eq!((hit.row, hit.sub, hit.gutter), (at, 1, false));
    assert_eq!(hit.column, 5);
    let raw = &s
        .current()
        .unwrap()
        .diff
        .parsed()
        .unwrap()
        .line(rb_diff::LineId { hunk: 0, line: 2 })
        .unwrap()
        .text;
    let points = wrap_points(raw, a.tab_width, usize::from(l.code.width - GUTTER));
    let offset = row_start(&points, hit.sub) + hit.column;
    let expanded: Vec<char> = raw.chars().collect();
    let drawn = buffer[(code_x + 5, y + 1)].symbol().chars().next().unwrap();
    assert_eq!(expanded[offset], drawn);
    let gutter = diffview::hit_at(&s.view.screen, l.code, s.view.scroll, l.code.x + 3, y).unwrap();
    assert!(gutter.gutter);
}
