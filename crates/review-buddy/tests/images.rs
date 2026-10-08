//! Pictures in the Overview, rendered headlessly: the demo change with two screenshots as notes,
//! as halfblocks (the one renderer whose output is plain text and the same on every machine),
//! scrolled so one is partly off the top, and with one selected. Real sixel, kitty and iTerm2
//! bytes are never snapshotted; those protocols are the library's, and are slow to review.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Cmd, Msg};
use review_buddy::demo::{images as demo_images, parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::images::State;

#[path = "support/render.rs"]
mod render_support;
use render_support::{render, rows, text};

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

fn press(app: &mut App, c: char) -> Vec<Cmd> {
    update(
        app,
        Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
    )
}

/// The demo dashboard with change 209 (two screenshots in its description) selected.
fn screen(theme: &str, (w, h): (u16, u16), images: State, answered: bool) -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (w, h),
    });
    app.images = images;
    let snapshot = block_on(world.snapshot()).unwrap();
    let mut asked = update(&mut app, Msg::Loaded(Box::new(snapshot)));
    for _ in 0..20 {
        if app.selected_change().is_some_and(|c| c.id.number == 209) {
            break;
        }
        asked.extend(press(&mut app, 'j'));
    }
    assert_eq!(app.selected_change().unwrap().id.number, 209);
    if answered {
        let urls: Vec<String> = asked
            .iter()
            .filter_map(|c| match c {
                Cmd::FetchImage { url, .. } => Some(url.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(urls, [demo_images::BEFORE, demo_images::AFTER], "{asked:?}");
        for url in urls {
            let result = demo_images::load(&url);
            update(&mut app, Msg::ImageLoaded { url, result });
        }
    }
    app
}

macro_rules! frames {
    ($($name:ident: $theme:literal, $w:literal, $h:literal, $images:expr, $answered:literal;)*) => {
        $(
            #[test]
            fn $name() {
                let mut app = screen($theme, ($w, $h), $images, $answered);
                insta::assert_snapshot!(text(&render(&mut app)));
            }
        )*
    };
}

frames! {
    images_as_halfblocks_liminal_hq_160x40: "liminal-hq", 160, 40, State::with_halfblocks(), true;
    images_as_halfblocks_dusk_160x40: "dusk", 160, 40, State::with_halfblocks(), true;
    images_as_halfblocks_liminal_hq_100x30: "liminal-hq", 100, 30, State::with_halfblocks(), true;
    images_as_halfblocks_dusk_100x30: "dusk", 100, 30, State::with_halfblocks(), true;
    images_as_notes_liminal_hq_160x40: "liminal-hq", 160, 40, State::default(), false;
    images_as_notes_dusk_160x40: "dusk", 160, 40, State::default(), false;
    images_as_notes_liminal_hq_100x30: "liminal-hq", 100, 30, State::default(), false;
    images_as_notes_dusk_100x30: "dusk", 100, 30, State::default(), false;
}

#[test]
fn images_still_loading_say_so() {
    let mut app = screen("liminal-hq", (160, 40), State::with_halfblocks(), false);
    let frame = text(&render(&mut app));
    assert!(
        frame.contains("▣ image: Muted text before the change – loading…"),
        "{frame}"
    );
    assert!(!frame.contains("![") && !frame.contains("<img"), "{frame}");
}

#[test]
fn a_failed_image_says_why_and_what_it_was() {
    let mut app = screen("liminal-hq", (160, 40), State::with_halfblocks(), false);
    update(
        &mut app,
        Msg::ImageLoaded {
            url: demo_images::BEFORE.into(),
            result: Err(review_buddy::images::Failure::Svg),
        },
    );
    let frame = text(&render(&mut app));
    assert!(
        frame.contains("▣ image: Muted text before the change – SVG images aren't drawn"),
        "{frame}"
    );
}

#[test]
fn the_selected_image_is_marked_with_more_than_colour() {
    let mut app = screen("liminal-hq", (160, 40), State::default(), false);
    press(&mut app, 'i');
    let frame = text(&render(&mut app));
    assert!(
        frame.contains("▸ ▣ image: Muted text before the change – can't be drawn in this terminal"),
        "{frame}"
    );
    assert!(
        frame.contains("with o)"),
        "the hint to open it shows: {frame}"
    );
    press(&mut app, 'i');
    let frame = text(&render(&mut app));
    assert!(
        frame.contains("▸ ▣ image: Muted text after the change"),
        "{frame}"
    );
}

/// Rows of the frame that hold halfblock cells, as `(first, last)`.
fn picture_rows(app: &mut App) -> Option<(usize, usize)> {
    let buffer = render(app);
    let hits: Vec<usize> = rows(&buffer)
        .iter()
        .enumerate()
        .filter(|(_, row)| row.contains('▀'))
        .map(|(y, _)| y)
        .collect();
    Some((*hits.first()?, *hits.last()?))
}

#[test]
fn a_partly_scrolled_picture_is_clipped_to_the_pane_and_never_spills() {
    let mut app = screen("liminal-hq", (100, 30), State::with_halfblocks(), true);
    let (top, _) = picture_rows(&mut app).expect("the pictures are drawn");
    let pane_bottom = usize::from(app.size.1) - 2;
    app.dashboard.focus = review_buddy::app::Pane::Detail;
    let mut seen_at_top = false;
    for _ in 0..40 {
        if let Some((first, last)) = picture_rows(&mut app) {
            assert!(first >= top.min(7), "never above the tabs: {first}");
            assert!(last < pane_bottom, "never below the pane: {last}");
            seen_at_top |= first <= top - 3;
        }
        press(&mut app, 'j');
    }
    assert!(seen_at_top, "scrolling moved a picture up toward the tabs");
}

#[test]
fn pictures_scroll_with_the_text() {
    let mut app = screen("liminal-hq", (100, 30), State::with_halfblocks(), true);
    let before = picture_rows(&mut app).unwrap();
    app.dashboard.focus = review_buddy::app::Pane::Detail;
    press(&mut app, 'j');
    press(&mut app, 'j');
    let after = picture_rows(&mut app).unwrap();
    assert_eq!(
        after.0 + 2,
        before.0,
        "two lines of scroll move the picture up two rows"
    );
}

#[test]
fn selecting_an_image_scrolls_it_into_view() {
    let mut app = screen("liminal-hq", (100, 30), State::with_halfblocks(), true);
    app.dashboard.focus = review_buddy::app::Pane::Detail;
    for _ in 0..40 {
        press(&mut app, 'j');
    }
    assert!(
        picture_rows(&mut app).is_none(),
        "scrolled past both pictures"
    );
    press(&mut app, 'i');
    assert!(
        picture_rows(&mut app).is_some(),
        "i brings the first picture back"
    );
}

#[test]
fn drawing_twice_gives_the_same_frame() {
    let mut app = screen("dusk", (100, 30), State::with_halfblocks(), true);
    let first = text(&render(&mut app));
    assert_eq!(first, text(&render(&mut app)));
}
