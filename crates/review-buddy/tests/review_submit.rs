//! The review modal's keys, buttons and the thread reply hint, driven headlessly through the
//! real `ui::draw` hit map on the frozen demo data.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_theme::ColourDepth;
use review_buddy::app::{update, Action, App, AppConfig, Cmd, Msg, Screen};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::{self, HitMap};

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(f)
}

fn open(theme: &str, width: u16, height: u16) -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (width, height),
    });
    update(
        &mut app,
        Msg::Loaded(Box::new(block_on(world.snapshot()).unwrap())),
    );
    let id = app.selected_change().unwrap().id.clone();
    press(&mut app, KeyCode::Enter);
    let data = block_on(world.diff_data(&id)).unwrap();
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

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn render(app: &mut App) -> Buffer {
    let (w, h) = app.size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|frame| hits = ui::draw(frame, app)).unwrap();
    app.hits = hits;
    terminal.backend().buffer().clone()
}

fn rows(buffer: &Buffer) -> Vec<String> {
    let width = usize::from(buffer.area.width);
    buffer
        .content()
        .chunks(width)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect()
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) -> Vec<Cmd> {
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

fn click(app: &mut App, column: u16, row: u16) -> Vec<Cmd> {
    let mut cmds = mouse(app, MouseEventKind::Down(MouseButton::Left), column, row);
    cmds.extend(mouse(
        app,
        MouseEventKind::Up(MouseButton::Left),
        column,
        row,
    ));
    cmds
}

/// Column ranges of the Cancel and Approve buttons, padding included, and their row.
fn buttons(app: &mut App) -> (u16, std::ops::Range<u16>, std::ops::Range<u16>) {
    let shown = rows(&render(app));
    let (y, line) = shown
        .iter()
        .enumerate()
        .find(|(_, r)| r.contains("Cancel") && r.contains("Approve"))
        .expect("the button row");
    let chars: Vec<char> = line.chars().collect();
    let find = |word: &str| {
        let w: Vec<char> = word.chars().collect();
        chars.windows(w.len()).position(|c| c == w).unwrap() as u16
    };
    let cancel = find("Cancel") - 2..find("Cancel") + 6 + 2;
    let go = find("Approve") - 2..find("Approve") + 7 + 2;
    (y as u16, cancel, go)
}

const SIZES: [(u16, u16); 4] = [(100, 30), (160, 40), (110, 50), (200, 60)];

#[test]
fn every_cell_of_both_buttons_resolves_through_the_real_hit_map() {
    for (w, h) in SIZES {
        let mut a = open("liminal-hq", w, h);
        press(&mut a, KeyCode::Char('a'));
        let (y, cancel, go) = buttons(&mut a);
        for x in cancel {
            assert_eq!(
                a.hits.at(x, y),
                Some(&Action::ReviewButton(false)),
                "{w}x{h} cancel at {x}"
            );
        }
        for x in go {
            assert_eq!(
                a.hits.at(x, y),
                Some(&Action::ReviewButton(true)),
                "{w}x{h} approve at {x}"
            );
        }
    }
}

#[test]
fn clicking_first_middle_and_last_cell_of_each_button_acts() {
    for (w, h) in SIZES {
        let probe = {
            let mut a = open("dusk", w, h);
            press(&mut a, KeyCode::Char('a'));
            buttons(&mut a)
        };
        let (y, cancel, go) = probe;
        let picks = |r: &std::ops::Range<u16>| [r.start, (r.start + r.end) / 2, r.end - 1];
        for x in picks(&cancel) {
            let mut a = open("dusk", w, h);
            press(&mut a, KeyCode::Char('a'));
            render(&mut a);
            click(&mut a, x, y);
            assert!(
                a.diff.as_ref().unwrap().review.is_none(),
                "{w}x{h} cancel {x}"
            );
        }
        for x in picks(&go) {
            let mut a = open("dusk", w, h);
            press(&mut a, KeyCode::Char('a'));
            render(&mut a);
            let cmds = click(&mut a, x, y);
            assert!(
                matches!(cmds.as_slice(), [Cmd::SubmitReview { .. }]),
                "{w}x{h} approve {x}"
            );
        }
    }
}

#[test]
fn a_lone_release_fires_but_a_press_alone_or_dragged_away_does_not() {
    let mut a = open("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('a'));
    let (y, cancel, go) = buttons(&mut a);
    let x = go.start + 3;

    let cmds = mouse(&mut a, MouseEventKind::Up(MouseButton::Left), x, y);
    assert!(matches!(cmds.as_slice(), [Cmd::SubmitReview { .. }]));

    let mut a = open("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('a'));
    buttons(&mut a);
    assert!(mouse(&mut a, MouseEventKind::Down(MouseButton::Left), x, y).is_empty());
    assert!(!a.diff.as_ref().unwrap().submitting);
    assert!(mouse(
        &mut a,
        MouseEventKind::Up(MouseButton::Left),
        cancel.start + 3,
        y
    )
    .is_empty());
    assert!(
        a.diff.as_ref().unwrap().review.is_some(),
        "pressing one button and releasing on another does nothing"
    );
}

#[test]
fn a_click_inside_the_modal_on_no_control_changes_nothing() {
    let mut a = open("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('a'));
    let (y, _, go) = buttons(&mut a);
    let before = a.diff.as_ref().unwrap().review.clone();
    let cmds = click(&mut a, go.end + 12, y);
    assert!(cmds.is_empty());
    assert_eq!(a.diff.as_ref().unwrap().review, before);
}

#[test]
fn the_hint_follows_whether_the_terminal_can_send_ctrl_enter() {
    let mut a = open("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('R'));
    press(&mut a, KeyCode::Char('x'));
    a.diff.as_mut().unwrap().review.as_mut().unwrap().focus =
        review_buddy::app::review::ReviewFocus::Summary;
    let plain = rows(&render(&mut a)).join("\n");
    assert!(
        plain.contains("tab then ⏎ submits · ⌃P submits now"),
        "{plain}"
    );
    assert!(!plain.contains("⌃⏎"));
    a.kitty_keys = true;
    let kitty = rows(&render(&mut a)).join("\n");
    assert!(kitty.contains("⌃⏎ or ⌃P submits"), "{kitty}");
}

fn at_thread(app: &mut App) {
    for _ in 0..60 {
        let shown = rows(&render(app)).join("\n");
        if shown
            .lines()
            .last()
            .is_some_and(|_| shown.contains(" r reply  "))
            && footer_has_reply(&shown)
        {
            return;
        }
        press(app, KeyCode::Char('j'));
    }
    panic!("no thread line reachable");
}

fn footer_has_reply(shown: &str) -> bool {
    shown
        .lines()
        .rev()
        .take(2)
        .any(|l| l.contains("c comment") && l.contains("r reply"))
}

#[test]
fn a_thread_line_offers_r_reply_in_the_footer_and_the_block_and_a_click_opens_it() {
    let mut a = open("liminal-hq", 160, 40);
    at_thread(&mut a);
    let shown = rows(&render(&mut a));
    let y = shown
        .iter()
        .position(|r| r.contains("╰") && r.contains(" r reply "))
        .expect("a thread block with the hint in its bottom border") as u16;
    let x = shown[y as usize].find("r reply").unwrap() as u16;
    let col = shown[y as usize][..x as usize].chars().count() as u16;
    let cmds = click(&mut a, col, y);
    assert!(cmds.is_empty());
    assert!(
        a.diff.as_ref().unwrap().composer.is_some(),
        "the reply box opens"
    );
}

#[test]
fn a_line_without_a_thread_has_no_reply_hint_in_the_footer() {
    let mut a = open("liminal-hq", 160, 40);
    for _ in 0..60 {
        let shown = rows(&render(&mut a)).join("\n");
        if !footer_has_reply(&shown) {
            return;
        }
        press(&mut a, KeyCode::Char('j'));
    }
    panic!("every line had a thread");
}
