//! Mouse input driven through `update` against headless renders of the frozen demo data. Each
//! click is aimed at text found on screen, so it exercises the `HitMap` the draw registered.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_theme::ColourDepth;
use review_buddy::app::{
    update, Cmd, DiffFocus, Msg, Notice, NoticeKind, Pane, Screen, Tab, {App, AppConfig, Snapshot},
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

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn diff(theme: &str, width: u16, height: u16) -> App {
    let mut app = dashboard(theme, width, height);
    let id = app.selected_change().unwrap().id.clone();
    let data = block_on(world().diff_data(&id)).unwrap();
    let Some(Cmd::LoadDiff(id)) = press(&mut app, KeyCode::Enter).into_iter().next() else {
        panic!("enter opens the diff");
    };
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

fn find(buffer: &Buffer, needle: &str) -> (u16, u16) {
    let width = buffer.area.width;
    (0..buffer.area.height)
        .find_map(|y| {
            let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
            line.find(needle)
                .map(|byte| (line[..byte].chars().count() as u16, y))
        })
        .unwrap_or_else(|| panic!("{needle:?} on screen:\n{}", text(buffer)))
}

fn mouse(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16), mods: KeyModifiers) {
    update(
        app,
        Msg::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: mods,
        }),
    );
}

fn click(app: &mut App, at: (u16, u16)) {
    mouse(
        app,
        MouseEventKind::Down(MouseButton::Left),
        at,
        KeyModifiers::NONE,
    );
    mouse(
        app,
        MouseEventKind::Up(MouseButton::Left),
        at,
        KeyModifiers::NONE,
    );
}

fn click_text(app: &mut App, needle: &str) {
    let (x, y) = find(&render(app), needle);
    click(app, (x + 1, y));
}

fn wheel(app: &mut App, at: (u16, u16), down: bool) {
    let kind = if down {
        MouseEventKind::ScrollDown
    } else {
        MouseEventKind::ScrollUp
    };
    mouse(app, kind, at, KeyModifiers::NONE);
}

fn tick(app: &mut App, n: usize) {
    for _ in 0..n {
        update(app, Msg::Tick);
    }
}

#[test]
fn clicking_tabs_switches_the_detail_view() {
    let mut a = dashboard("liminal-hq", 160, 40);
    for (label, tab) in [
        ("Checks", Tab::Checks),
        ("Conversation", Tab::Conversation),
        ("Files", Tab::Files),
        ("Overview", Tab::Overview),
    ] {
        click_text(&mut a, label);
        assert_eq!(a.dashboard.tab, tab, "{label}");
    }
}

#[test]
fn clicking_chips_acts_like_their_keys() {
    let mut a = dashboard("liminal-hq", 160, 40);
    click_text(&mut a, "Merge");
    assert!(a.status.is_some(), "the chip explains itself");
    assert_eq!(a.screen, Screen::Dashboard);
    click_text(&mut a, "Approve");
    assert_eq!(a.screen, Screen::Diff, "Approve opens the diff");
    let mut a = dashboard("liminal-hq", 160, 40);
    click_text(&mut a, "Diff");
    assert_eq!(a.screen, Screen::Diff);
}

#[test]
fn clicking_source_rows_and_collapsed_source_tabs_filters_the_queue() {
    let mut wide = dashboard("liminal-hq", 160, 40);
    let label = wide.state.sources[1].label.clone();
    click_text(&mut wide, &label);
    assert_eq!(wide.dashboard.source, 2);
    assert_eq!(wide.dashboard.focus, Pane::Sources);

    let mut narrow = dashboard("liminal-hq", 100, 30);
    let label = narrow.state.sources[0].label.clone();
    click_text(&mut narrow, &label);
    assert_eq!(narrow.dashboard.source, 1);
}

#[test]
fn clicking_an_empty_pane_focuses_it_and_the_theme_name_cycles() {
    let mut a = dashboard("liminal-hq", 160, 40);
    render(&mut a);
    click(&mut a, (150, 36));
    assert_eq!(a.dashboard.focus, Pane::Detail);
    let before = a.theme_name().to_string();
    click_text(&mut a, &before);
    assert_ne!(a.theme_name(), before);
}

#[test]
fn clicking_a_toast_dismisses_it() {
    let mut a = dashboard("liminal-hq", 160, 40);
    update(
        &mut a,
        Msg::Notify(Notice::new(NoticeKind::Info, "Saved your place")),
    );
    click_text(&mut a, "Saved your place");
    assert!(a.toasts.is_empty());
}

#[test]
fn a_double_click_on_a_queue_row_opens_the_diff_and_a_slow_one_does_not() {
    let mut a = dashboard("liminal-hq", 160, 40);
    let (x, y) = find(&render(&mut a), "Waiting on you");
    let row = (x + 2, y + 2);
    click(&mut a, row);
    assert_eq!(a.screen, Screen::Dashboard);
    tick(&mut a, 8);
    click(&mut a, row);
    assert_eq!(a.screen, Screen::Dashboard, "too slow to count");
    tick(&mut a, 1);
    click(&mut a, row);
    assert_eq!(a.screen, Screen::Diff);
}

#[test]
fn the_wheel_scrolls_the_pane_under_the_pointer_not_the_focused_one() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    assert_eq!(a.diff_state().unwrap().focus, DiffFocus::Diff);
    let before = a.diff_state().unwrap().files_scroll;
    wheel(&mut a, (5, 5), true);
    assert!(a.diff_state().unwrap().files_scroll >= before);
    assert_eq!(
        a.diff_state().unwrap().view.scroll,
        0,
        "the diff stayed put"
    );
    wheel(&mut a, (100, 20), true);
    assert!(a.diff_state().unwrap().view.scroll > 0);
    wheel(&mut a, (100, 20), false);
    assert_eq!(a.diff_state().unwrap().view.scroll, 0);

    let mut d = dashboard("liminal-hq", 100, 30);
    render(&mut d);
    d.dashboard.focus = Pane::Queue;
    let before = d.dashboard.queue_scroll;
    wheel(&mut d, (80, 22), true);
    assert_eq!(
        d.dashboard.queue_scroll, before,
        "the detail pane is under the pointer"
    );
}

fn code_row(offset: u16) -> (u16, u16) {
    (44, 1 + 1 + 1 + offset)
}

fn drag(app: &mut App, from: (u16, u16), to: (u16, u16)) {
    mouse(
        app,
        MouseEventKind::Down(MouseButton::Left),
        from,
        KeyModifiers::NONE,
    );
    mouse(
        app,
        MouseEventKind::Drag(MouseButton::Left),
        to,
        KeyModifiers::NONE,
    );
    mouse(
        app,
        MouseEventKind::Up(MouseButton::Left),
        to,
        KeyModifiers::NONE,
    );
}

fn range_bounds(app: &App) -> Option<(usize, usize)> {
    app.diff_state()?.range.map(|r| r.bounds())
}

#[test]
fn dragging_over_diff_lines_selects_a_range_and_esc_clears_it() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    let top = code_row(1);
    let bottom = code_row(4);
    drag(&mut a, top, bottom);
    let (lo, hi) = range_bounds(&a).expect("a range");
    assert!(hi > lo);
    assert_eq!(a.diff_state().unwrap().view.cursor, hi);
    assert!(a.diff_state().unwrap().drag.is_none(), "released");

    let shown = text(&render(&mut a));
    assert!(shown.contains('▌'), "range rows carry a marker");

    press(&mut a, KeyCode::Esc);
    assert_eq!(range_bounds(&a), None);
    assert_eq!(a.screen, Screen::Diff, "esc cleared the range first");
    press(&mut a, KeyCode::Esc);
    assert_eq!(a.screen, Screen::Dashboard);
}

#[test]
fn a_plain_click_clears_the_range_and_a_drag_back_up_works() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    drag(&mut a, code_row(5), code_row(2));
    let (lo, hi) = range_bounds(&a).unwrap();
    assert!(lo < hi);
    assert_eq!(a.diff_state().unwrap().view.cursor, lo, "head sits on top");
    click(&mut a, code_row(3));
    assert_eq!(range_bounds(&a), None);
}

#[test]
fn shift_click_extends_from_the_cursor_and_shift_drag_is_left_alone() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    click(&mut a, code_row(1));
    let anchor = a.diff_state().unwrap().view.cursor;
    mouse(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        code_row(5),
        KeyModifiers::SHIFT,
    );
    let (lo, hi) = range_bounds(&a).expect("shift-click extends");
    assert_eq!(lo, anchor);
    assert!(hi > lo);

    mouse(
        &mut a,
        MouseEventKind::Drag(MouseButton::Left),
        code_row(8),
        KeyModifiers::SHIFT,
    );
    assert_eq!(
        range_bounds(&a),
        Some((lo, hi)),
        "shift-drag changes nothing"
    );

    mouse(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        code_row(7),
        KeyModifiers::SHIFT,
    );
    let (lo2, hi2) = range_bounds(&a).unwrap();
    assert_eq!(lo2, lo, "the anchor holds");
    assert!(hi2 > hi);
}

#[test]
fn shift_clicks_outside_the_diff_do_nothing() {
    let mut a = dashboard("liminal-hq", 160, 40);
    let (x, y) = find(&render(&mut a), "Checks");
    mouse(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        (x + 1, y),
        KeyModifiers::SHIFT,
    );
    assert_eq!(a.dashboard.tab, Tab::Overview);
}

#[test]
fn clicking_files_and_the_review_block_focuses_their_pane() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    click_text(&mut a, "Your review");
    assert_eq!(a.diff_state().unwrap().focus, DiffFocus::Files);
    click(&mut a, code_row(1));
    assert_eq!(a.diff_state().unwrap().focus, DiffFocus::Diff);
    click(&mut a, (100, 1));
    assert_eq!(a.diff_state().unwrap().focus, DiffFocus::Diff);
    click(&mut a, (3, 20));
    assert_eq!(a.diff_state().unwrap().focus, DiffFocus::Files);
}

#[test]
fn clicking_a_thread_block_puts_the_cursor_on_the_line_it_hangs_from() {
    let mut a = diff("liminal-hq", 160, 40);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "thread · line");
    click(&mut a, (x + 4, y));
    let state = a.diff_state().unwrap();
    assert_eq!(state.focus, DiffFocus::Diff);
    let row = state
        .view
        .rows
        .nearest_line(usize::from(y) - 2 + state.view.scroll);
    assert_eq!(Some(state.view.cursor), row);
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        let code = if c == '\n' {
            KeyCode::Enter
        } else {
            KeyCode::Char(c)
        };
        let mods = if c == '\n' {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        update(app, Msg::Key(KeyEvent::new(code, mods)));
    }
}

#[test]
fn clicking_in_the_composer_places_the_caret_and_outside_clicks_are_ignored() {
    let mut a = diff("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('c'));
    type_text(&mut a, "first line\nsecond");
    let (x, y) = find(&render(&mut a), "first line");
    click(&mut a, (x + 3, y));
    let composer = a.diff_state().unwrap().composer.as_ref().unwrap();
    assert_eq!(composer.editor.cursor(), (0, 3));
    click(&mut a, (x + 80, y));
    let composer = a.diff_state().unwrap().composer.as_ref().unwrap();
    assert_eq!(
        composer.editor.cursor(),
        (0, 10),
        "past the end lands at the end"
    );

    type_text(&mut a, "!");
    let composer = a.diff_state().unwrap().composer.as_ref().unwrap();
    assert_eq!(composer.editor.lines()[0], "first line!");

    click(&mut a, (3, 20));
    click(&mut a, (100, 1));
    let state = a.diff_state().unwrap();
    assert!(state.composer.is_some(), "the draft survives");
    assert_eq!(state.focus, DiffFocus::Diff);

    wheel(&mut a, (x + 3, y), true);
    let composer = a.diff_state().unwrap().composer.as_ref().unwrap();
    assert_eq!(composer.editor.cursor().0, 1, "the wheel moved down a line");
}

#[test]
fn the_discard_confirm_defaults_to_cancel_and_its_buttons_answer_clicks() {
    let mut a = diff("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('c'));
    type_text(&mut a, "keep me");
    press(&mut a, KeyCode::Esc);
    assert!(a.diff_state().unwrap().confirm.is_some());
    assert!(!a.diff_state().unwrap().confirm.as_ref().unwrap().yes);

    click(&mut a, (3, 20));
    assert!(
        a.diff_state().unwrap().confirm.is_some(),
        "outside is ignored"
    );

    click_text(&mut a, "No, keep editing");
    let state = a.diff_state().unwrap();
    assert!(state.confirm.is_none());
    assert!(state.composer.is_some(), "cancel keeps the draft");

    press(&mut a, KeyCode::Esc);
    click_text(&mut a, "  Discard  ");
    assert!(a.diff_state().unwrap().composer.is_none());
}

#[test]
fn the_approve_preview_buttons_are_clickable() {
    let mut a = diff("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('a'));
    click_text(&mut a, "Cancel");
    assert!(a.diff_state().unwrap().review.is_none());
    press(&mut a, KeyCode::Char('a'));
    assert!(a.diff_state().unwrap().review.is_some());
    click_text(&mut a, "› Approve ‹");
    assert!(a.diff_state().unwrap().submitting, "the approval was sent");
}

#[test]
fn every_control_of_the_review_modal_answers_the_mouse() {
    use rb_core::Verdict;
    let mut a = diff("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('R'));
    click_text(&mut a, "( ) 2 Approve");
    assert_eq!(
        a.diff_state().unwrap().review.as_ref().unwrap().verdict,
        Verdict::Approve
    );
    click_text(&mut a, "( ) 3 Request changes");
    let m = a.diff_state().unwrap().review.as_ref().unwrap();
    assert_eq!(m.verdict, Verdict::RequestChanges);

    let before = a.diff_state().unwrap().view.cursor;
    click(&mut a, (2, 2));
    assert!(
        a.diff_state().unwrap().review.is_some(),
        "outside is ignored"
    );
    assert_eq!(a.diff_state().unwrap().view.cursor, before);

    let (x, y) = find(&render(&mut a), "▏");
    click(&mut a, (x + 4, y));
    for c in "Needs work".chars() {
        press(&mut a, KeyCode::Char(c));
    }
    let m = a.diff_state().unwrap().review.as_ref().unwrap();
    assert_eq!(m.summary.text(), "Needs work");
    assert_eq!(m.focus, review_buddy::app::review::ReviewFocus::Summary);

    click_text(&mut a, "Cancel");
    assert!(a.diff_state().unwrap().review.is_none());
}

#[test]
fn clicking_a_disabled_submit_explains_and_sends_nothing() {
    let mut a = diff("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('x'));
    click_text(&mut a, "  Request changes  ");
    let s = a.diff_state().unwrap();
    assert!(s.review.is_some() && !s.submitting);
    assert!(a
        .status
        .as_ref()
        .unwrap()
        .notice
        .text
        .contains("short summary"));
}

#[test]
fn the_help_overlay_scrolls_with_the_wheel_and_a_click_closes_it() {
    let mut a = diff("liminal-hq", 100, 30);
    press(&mut a, KeyCode::Char('?'));
    render(&mut a);
    let max = review_buddy::ui::help::max_scroll(&a);
    for _ in 0..(max + 3) {
        wheel(&mut a, (50, 15), true);
    }
    assert_eq!(a.help_scroll, max);
    wheel(&mut a, (50, 15), false);
    assert_eq!(a.help_scroll, max.saturating_sub(1));
    click(&mut a, (50, 15));
    assert!(!a.help);
    assert_eq!(a.help_scroll, 0);
}

fn range_app(theme: &str, width: u16, height: u16) -> App {
    let mut a = diff(theme, width, height);
    render(&mut a);
    drag(&mut a, code_row(2), code_row(5));
    assert!(range_bounds(&a).is_some());
    a
}

#[test]
fn range_160x40_default_theme() {
    let mut a = range_app("liminal-hq", 160, 40);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn range_160x40_dusk() {
    let mut a = range_app("dusk", 160, 40);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn range_100x30_default_theme() {
    let mut a = range_app("liminal-hq", 100, 30);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn range_100x30_dusk() {
    let mut a = range_app("dusk", 100, 30);
    insta::assert_snapshot!(text(&render(&mut a)));
}

#[test]
fn the_range_is_drawn_with_the_selection_role() {
    use rb_theme::Role;
    let mut a = range_app("liminal-hq", 160, 40);
    let buffer = render(&mut a);
    let (lo, _) = range_bounds(&a).unwrap();
    let state = a.diff_state().unwrap();
    let y = 3 + (lo - state.view.scroll) as u16;
    let want = ui::style::bg(&a.palette, Role::Selection).bg;
    assert_ne!(want, None);
    assert_eq!(buffer[(60, y)].bg, want.unwrap());
}

fn mouse_cmds(
    app: &mut App,
    kind: MouseEventKind,
    (column, row): (u16, u16),
    mods: KeyModifiers,
) -> Vec<Cmd> {
    update(
        app,
        Msg::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: mods,
        }),
    )
}

/// Presses at `from`, drags to `to` and releases there, drawing between events as the runtime
/// does, and returns the effects of the release.
fn drag_cmds(app: &mut App, from: (u16, u16), to: (u16, u16)) -> Vec<Cmd> {
    let none = KeyModifiers::NONE;
    mouse_cmds(app, MouseEventKind::Down(MouseButton::Left), from, none);
    render(app);
    mouse_cmds(app, MouseEventKind::Drag(MouseButton::Left), to, none);
    render(app);
    mouse_cmds(app, MouseEventKind::Up(MouseButton::Left), to, none)
}

fn copied(cmds: &[Cmd]) -> Option<&str> {
    cmds.iter().find_map(|c| match c {
        Cmd::CopySelection(text) => Some(text.as_str()),
        _ => None,
    })
}

/// The screen as text with the cells painted as selected text wrapped in `⟦ ⟧`, so a snapshot
/// shows exactly what is highlighted.
fn marked(app: &App, buffer: &Buffer) -> String {
    let look = ui::style::text_selection(&app.palette);
    let on = |x: u16, y: u16| {
        let cell = &buffer[(x, y)];
        match look.bg {
            Some(bg) => cell.bg == bg,
            None => cell.modifier.contains(ratatui::style::Modifier::REVERSED),
        }
    };
    (0..buffer.area.height)
        .map(|y| {
            let mut line = String::new();
            let mut open = false;
            for x in 0..buffer.area.width {
                let now = on(x, y);
                if now != open {
                    line.push(if now { '⟦' } else { '⟧' });
                    open = now;
                }
                line.push_str(buffer[(x, y)].symbol());
            }
            if open {
                line.push('⟧');
            }
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn dragging_on_code_selects_text_and_copies_it_without_a_line_range() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    let cmds = drag_cmds(&mut a, (60, 4), (80, 6));
    let text = copied(&cmds).expect("a drag on code copies on release");
    assert!(text.contains('\n'), "three rows were covered: {text:?}");
    assert!(!text.contains(" \n"), "trailing spaces are trimmed");
    assert!(
        !text.contains('│') && !text.contains('▌'),
        "no gutter or border: {text:?}"
    );
    assert_eq!(
        range_bounds(&a),
        None,
        "text drags do not build a line range"
    );
    assert!(a.selection.is_some(), "the highlight stays until esc");
}

#[test]
fn a_drag_that_starts_in_the_gutter_keeps_the_line_range() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    let cmds = drag_cmds(&mut a, (44, 4), (80, 7));
    assert!(copied(&cmds).is_none());
    assert!(range_bounds(&a).is_some());
    assert!(a.selection.is_none());
}

#[test]
fn esc_and_a_click_elsewhere_clear_the_text_highlight() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    drag_cmds(&mut a, (60, 4), (80, 6));
    assert!(a.selection.is_some());
    press(&mut a, KeyCode::Esc);
    assert!(a.selection.is_none());
    assert_eq!(a.screen, Screen::Diff, "esc cleared the highlight first");

    drag_cmds(&mut a, (60, 4), (80, 6));
    render(&mut a);
    click(&mut a, (44, 10));
    assert!(a.selection.is_none(), "a click in the gutter clears it");
}

#[test]
fn a_plain_click_on_code_selects_nothing() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    let cmds = drag_cmds(&mut a, (60, 6), (60, 6));
    assert!(copied(&cmds).is_none());
    assert!(a.selection.is_none());
}

#[test]
fn shift_events_in_the_text_never_select() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    let shift = KeyModifiers::SHIFT;
    mouse_cmds(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        (60, 4),
        shift,
    );
    mouse_cmds(
        &mut a,
        MouseEventKind::Drag(MouseButton::Left),
        (90, 6),
        shift,
    );
    let cmds = mouse_cmds(
        &mut a,
        MouseEventKind::Up(MouseButton::Left),
        (90, 6),
        shift,
    );
    assert!(a.selection.is_none() && copied(&cmds).is_none());
}

#[test]
fn double_click_selects_a_word_and_triple_click_a_line() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    let none = KeyModifiers::NONE;
    let at = (60, 5);
    let click_cmds = |a: &mut App| {
        mouse_cmds(a, MouseEventKind::Down(MouseButton::Left), at, none);
        render(a);
        mouse_cmds(a, MouseEventKind::Up(MouseButton::Left), at, none)
    };
    assert!(
        copied(&click_cmds(&mut a)).is_none(),
        "one click copies nothing"
    );
    let word = click_cmds(&mut a);
    let word = copied(&word)
        .expect("a double-click copies a word")
        .to_string();
    assert!(!word.contains(' ') && !word.contains('\n'), "{word:?}");
    let line = click_cmds(&mut a);
    let line = copied(&line)
        .expect("a triple-click copies a line")
        .to_string();
    assert!(
        line.len() >= word.len() && line.contains(&word),
        "{line:?} / {word:?}"
    );
    assert!(!line.contains('\n'));
}

#[test]
fn y_copies_the_selection_and_otherwise_keeps_its_meaning() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    let cmds = press(&mut a, KeyCode::Char('y'));
    assert!(
        matches!(cmds.first(), Some(Cmd::Copy(_))),
        "no selection: the link"
    );
    drag_cmds(&mut a, (60, 4), (80, 5));
    let cmds = press(&mut a, KeyCode::Char('y'));
    assert!(copied(&cmds).is_some(), "a selection: its text");
}

#[test]
fn text_drags_are_drawn_apart_from_the_cursor_line() {
    let mut a = diff("liminal-hq", 160, 40);
    render(&mut a);
    drag_cmds(&mut a, (60, 4), (90, 6));
    let buffer = render(&mut a);
    let look = ui::style::text_selection(&a.palette);
    let cursor = ui::style::bg(&a.palette, rb_theme::Role::Selection).bg;
    assert_ne!(look.bg, None);
    assert_ne!(look.bg, cursor, "stronger than the cursor line's tint");
    assert_eq!(buffer[(55, 5)].bg, look.bg.unwrap());
    assert_ne!(
        buffer[(40, 5)].bg,
        look.bg.unwrap(),
        "the gutter is not selected"
    );
}

fn selection_app(theme: &str, width: u16, height: u16) -> App {
    let mut a = diff(theme, width, height);
    render(&mut a);
    let x0 = 60;
    drag_cmds(&mut a, (x0, 4), (x0 + 18, 6));
    assert!(a.selection.is_some());
    a
}

#[test]
fn text_selection_160x40_default_theme() {
    let mut a = selection_app("liminal-hq", 160, 40);
    let buffer = render(&mut a);
    insta::assert_snapshot!(marked(&a, &buffer));
}

#[test]
fn text_selection_160x40_dusk() {
    let mut a = selection_app("dusk", 160, 40);
    let buffer = render(&mut a);
    insta::assert_snapshot!(marked(&a, &buffer));
}

#[test]
fn text_selection_100x30_default_theme() {
    let mut a = selection_app("liminal-hq", 100, 30);
    let buffer = render(&mut a);
    insta::assert_snapshot!(marked(&a, &buffer));
}

#[test]
fn text_selection_100x30_dusk() {
    let mut a = selection_app("dusk", 100, 30);
    let buffer = render(&mut a);
    insta::assert_snapshot!(marked(&a, &buffer));
}

#[test]
fn dragging_in_the_description_copies_its_bullets_and_stays_in_the_description() {
    let mut a = dashboard("liminal-hq", 160, 40);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "• Menu::select_next");
    // The pointer ends far below, in the Latest comment text: still the description.
    let (_, low) = find(&buffer, "us?");
    let cmds = drag_cmds(&mut a, (x, y), (x + 40, low));
    let text = copied(&cmds).expect("copied");
    assert!(
        text.starts_with("• Menu::select_next and Menu::select_prev"),
        "{text:?}"
    );
    assert!(
        text.contains("• Menu::activate returns the action"),
        "{text:?}"
    );
    assert!(text.ends_with("picks up the active theme"), "{text:?}");
    assert!(
        !text.contains("Reviewers") && !text.contains("jo ·"),
        "{text:?}"
    );
}

#[test]
fn a_wrapped_comment_in_the_detail_pane_copies_as_one_paragraph() {
    let mut a = dashboard("liminal-hq", 160, 40);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "These three lines");
    let (_, low) = find(&buffer, "us?");
    let cmds = drag_cmds(&mut a, (x, y), (x + 5, low));
    let text = copied(&cmds).expect("copied");
    assert_eq!(
        text,
        "These three lines could be one join. Is the intermediate Vec doing anything for us?"
    );
}

#[test]
fn a_text_drag_in_the_detail_pane_does_not_disturb_the_queue_or_its_double_click() {
    let mut a = dashboard("liminal-hq", 160, 40);
    let before = a.dashboard.selected.clone();
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "Adds a menu bar");
    drag_cmds(&mut a, (x, y), (x + 20, y));
    assert_eq!(a.dashboard.selected, before);
    assert_eq!(a.screen, Screen::Dashboard);
    // A double-click on a queue row still opens the diff.
    let (x, y) = find(&render(&mut a), "Waiting on you");
    let row = (x + 2, y + 2);
    click(&mut a, row);
    tick(&mut a, 1);
    click(&mut a, row);
    assert_eq!(a.screen, Screen::Diff);
}

#[test]
fn a_drag_over_file_paths_copies_them_row_by_row() {
    let mut a = diff("liminal-hq", 160, 40);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "src/ui/menus.rs       +22");
    let (_, low) = find(&buffer, "src/ui/mod.rs");
    let cmds = drag_cmds(&mut a, (x + 4, y), (x + 8, low));
    let text = copied(&cmds).expect("copied");
    assert_eq!(text, "ui/menus.rs\nsrc/ui/menubar.rs\nsrc/ui/mo");
    assert_eq!(
        a.diff_state().unwrap().file,
        0,
        "the first press chose the first file"
    );
}

#[test]
fn the_text_highlight_is_reversed_under_no_color_and_flips_on_the_cursor_line() {
    use ratatui::style::Modifier;
    let mut a = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: true,
        size: (160, 40),
    });
    update(&mut a, Msg::Loaded(Box::new(snapshot())));
    let id = a.selected_change().unwrap().id.clone();
    let data = block_on(world().diff_data(&id)).unwrap();
    update(
        &mut a,
        Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    update(
        &mut a,
        Msg::DiffLoaded {
            id,
            result: Ok(Box::new(data)),
        },
    );
    render(&mut a);
    let buffer = render(&mut a);
    let (x, y) = find(&buffer, "pub struct Menu {");
    let cmds = drag_cmds(&mut a, (x + 4, y + 1), (x + 12, y + 1));
    assert!(copied(&cmds).is_some());
    let buffer = render(&mut a);
    // y + 1 is the cursor line, which NO_COLOR draws reversed: the text flips back, underlined.
    let cell = &buffer[(x + 6, y + 1)];
    assert!(!cell.modifier.contains(Modifier::REVERSED));
    assert!(cell.modifier.contains(Modifier::UNDERLINED), "{cell:?}");
    let plain = &buffer[(x + 20, y + 1)];
    assert!(
        plain.modifier.contains(Modifier::REVERSED),
        "the rest of the line is reversed"
    );
}
