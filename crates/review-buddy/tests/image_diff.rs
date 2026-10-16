//! Image files in the diff, rendered headlessly from the demo change that carries one: the
//! original and the replacement in each of the four layouts, as halfblocks (plain text on every
//! machine), and the notes when images are off or a version is missing.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Cmd, Msg};
use review_buddy::demo::{images as demo_images, parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::images::diff::Layout;
use review_buddy::images::State;

#[path = "support/render.rs"]
mod render_support;
use render_support::{render, text};

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

/// Opens the diff of demo change 209 on its picture. Returns the app and the fetches asked for.
fn on_the_picture(images: State, size: (u16, u16)) -> (App, Vec<Cmd>) {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let mut app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size,
    });
    app.images = images;
    let snapshot = block_on(world.snapshot()).unwrap();
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    while app.selected_change().is_none_or(|c| c.id.number != 209) {
        press(&mut app, 'j');
    }
    let id = app.selected_change().unwrap().id.clone();
    update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Enter)));
    let data = block_on(world.diff_data(&id)).unwrap();
    let mut asked = update(
        &mut app,
        Msg::DiffLoaded {
            id,
            result: Ok(Box::new(data)),
        },
    );
    asked.extend(update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Right))));
    (app, asked)
}

fn fetches(cmds: &[Cmd]) -> Vec<String> {
    cmds.iter()
        .filter_map(|c| match c {
            Cmd::FetchImage { url, .. } => Some(url.clone()),
            _ => None,
        })
        .collect()
}

fn answer(app: &mut App, urls: Vec<String>) {
    for url in urls {
        let result = demo_images::load(&url);
        update(app, Msg::ImageLoaded { url, result });
    }
}

#[test]
fn opening_a_picture_asks_for_both_versions_by_commit() {
    let (_, asked) = on_the_picture(State::with_halfblocks(), (160, 40));
    let urls = fetches(&asked);
    assert_eq!(urls.len(), 2, "{asked:?}");
    assert!(urls[0].ends_with("muted-text.png?ref=3ab7d05"), "{urls:?}");
    assert!(urls[1].ends_with("muted-text.png?ref=e81b6a4"), "{urls:?}");
}

#[test]
fn nothing_is_fetched_with_images_off() {
    let (app, asked) = on_the_picture(State::default(), (160, 40));
    assert!(fetches(&asked).is_empty());
    let mut app = app;
    let screen = text(&render(&mut app));
    assert!(screen.contains("binary or very large"), "{screen}");
}

#[test]
fn each_layout_draws_captions_and_pictures() {
    let (mut app, asked) = on_the_picture(State::with_halfblocks(), (160, 40));
    answer(&mut app, fetches(&asked));
    let mut seen = Vec::new();
    for _ in Layout::ALL {
        let screen = text(&render(&mut app));
        assert!(screen.contains("240×120"), "{screen}");
        assert!(
            screen.contains("pictures:") && screen.contains("i changes"),
            "{screen}"
        );
        seen.push((
            app.image_layout,
            screen.contains("Original"),
            screen.contains("Replacement"),
        ));
        press(&mut app, 'i');
    }
    assert_eq!(
        seen,
        [
            (Layout::Stacked, true, true),
            (Layout::SideBySide, true, true),
            (Layout::Original, true, false),
            (Layout::Replacement, false, true),
        ]
    );
    assert_eq!(app.image_layout, Layout::Stacked, "cycles back round");
}

#[test]
fn stacked_puts_the_original_above_and_side_by_side_puts_it_left() {
    let (mut app, asked) = on_the_picture(State::with_halfblocks(), (160, 40));
    answer(&mut app, fetches(&asked));
    let find = |app: &mut App, needle: &str| {
        let buffer = render(app);
        render_support::find(&buffer, needle).unwrap()
    };
    let (ox, oy) = find(&mut app, "Original");
    let (rx, ry) = find(&mut app, "Replacement");
    assert!(oy < ry && ox == rx, "stacked: {oy} {ry} {ox} {rx}");
    press(&mut app, 'i');
    let (ox, oy) = find(&mut app, "Original");
    let (rx, ry) = find(&mut app, "Replacement");
    assert!(ox < rx && oy == ry, "side by side: {ox} {rx} {oy} {ry}");
}

#[test]
fn a_failed_version_says_why_and_the_other_still_draws() {
    let (mut app, asked) = on_the_picture(State::with_halfblocks(), (160, 40));
    let urls = fetches(&asked);
    update(
        &mut app,
        Msg::ImageLoaded {
            url: urls[0].clone(),
            result: Err(review_buddy::images::Failure::Status(404)),
        },
    );
    answer(&mut app, vec![urls[1].clone()]);
    let screen = text(&render(&mut app));
    assert!(screen.contains("couldn't load"), "{screen}");
    assert!(screen.contains("Replacement · 240×120"), "{screen}");
}

#[test]
fn i_on_a_text_file_says_what_it_is_for() {
    let (mut app, _) = on_the_picture(State::with_halfblocks(), (160, 40));
    update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Left)));
    press(&mut app, 'i');
    let note = &app.status.as_ref().unwrap().notice.text;
    assert!(note.contains("image"), "{note}");
    assert_eq!(app.image_layout, Layout::Stacked);
}
