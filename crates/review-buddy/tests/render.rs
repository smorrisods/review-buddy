//! Headless renders of the chrome with ratatui's `TestBackend`, pinned with `insta`.

use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, Terminal};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Msg, Notice, NoticeKind};
use review_buddy::ui::{self, chrome::WORDMARK, HitMap};

fn app(width: u16, height: u16, no_color: bool) -> App {
    App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color,
        size: (width, height),
    })
}

fn render(app: &App, width: u16, height: u16) -> (Buffer, HitMap) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|frame| hits = ui::draw(frame, app)).unwrap();
    (terminal.backend().buffer().clone(), hits)
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

fn find_row(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let width = buffer.area.width;
    (0..buffer.area.height).find_map(|y| {
        let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        line.find(needle).map(|byte| {
            let col = line[..byte].chars().count() as u16;
            (col, y)
        })
    })
}

#[test]
fn empty_chrome_160x40() {
    let (buffer, _) = render(&app(160, 40, false), 160, 40);
    insta::assert_snapshot!(text(&buffer));
}

#[test]
fn empty_chrome_100x30() {
    let (buffer, _) = render(&app(100, 30, false), 100, 30);
    insta::assert_snapshot!(text(&buffer));
}

#[test]
fn too_small_notice() {
    let (buffer, hits) = render(&app(80, 24, false), 80, 24);
    insta::assert_snapshot!(text(&buffer));
    assert!(hits.is_empty(), "no click targets under the notice");
}

#[test]
fn too_narrow_only() {
    let (buffer, _) = render(&app(90, 40, false), 90, 40);
    insta::assert_snapshot!(text(&buffer));
}

#[test]
fn status_and_toasts_100x30() {
    let mut a = app(100, 30, false);
    update(
        &mut a,
        Msg::Notify(Notice::new(NoticeKind::Success, "Approved spindle#214")),
    );
    update(
        &mut a,
        Msg::Notify(Notice::new(
            NoticeKind::Warning,
            "Couldn't reach gitlab.work.ca (timed out). Showing cached data · r to retry",
        )),
    );
    update(
        &mut a,
        Msg::Key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Char('T'),
        )),
    );
    let (buffer, hits) = render(&a, 100, 30);
    insta::assert_snapshot!(text(&buffer));
    assert!(!hits.is_empty());
}

#[test]
fn wordmark_is_drawn_along_the_gradient() {
    let (buffer, _) = render(&app(160, 40, false), 160, 40);
    let (x, y) = find_row(&buffer, WORDMARK).expect("wordmark on the top bar");
    assert_eq!(y, 0);
    let first = buffer[(x, y)].fg;
    let last = buffer[(x + WORDMARK.len() as u16 - 1, y)].fg;
    assert!(matches!(first, Color::Rgb(255, 170, 64)), "{first:?}");
    assert!(matches!(last, Color::Rgb(167, 139, 250)), "{last:?}");
    let mid = buffer[(x + 6, y)].fg;
    assert_ne!(mid, first);
    assert_ne!(mid, last);
}

#[test]
fn transparent_background_is_never_painted() {
    let (buffer, _) = render(&app(100, 30, false), 100, 30);
    assert!(buffer.content().iter().all(|c| c.bg == Color::Reset));
}

#[test]
fn no_color_paints_no_colours() {
    let (buffer, _) = render(&app(100, 30, true), 100, 30);
    assert!(buffer
        .content()
        .iter()
        .all(|c| c.fg == Color::Reset && c.bg == Color::Reset));
    assert!(text(&buffer).contains(WORDMARK));
}

#[test]
fn hint_and_theme_name_are_click_targets() {
    let a = app(160, 40, false);
    let (buffer, hits) = render(&a, 160, 40);
    let (x, y) = find_row(&buffer, "quit").unwrap();
    assert_eq!(hits.at(x, y), Some(&review_buddy::app::Action::Quit));
    let (x, y) = find_row(&buffer, "Liminal HQ").unwrap();
    assert_eq!(y, 0);
    assert_eq!(
        hits.at(x + 1, y),
        Some(&review_buddy::app::Action::CycleTheme)
    );
}

#[test]
fn cycling_the_theme_changes_the_top_bar() {
    let mut a = app(160, 40, false);
    update(
        &mut a,
        Msg::Key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Char('T'),
        )),
    );
    let (buffer, _) = render(&a, 160, 40);
    assert!(!text(&buffer).lines().next().unwrap().contains("Liminal HQ"));
}
