//! The status cluster on queue rows: review state from other people, comments, open threads, CI
//! in words and size, rendered headlessly from the frozen demo data. The snapshots close Detail so
//! the queue has the whole width at 160x40; at 100x30 Detail sits below and the queue is full
//! width anyway. The rest pin the narrow-width order, the `ui.queue_status` choices and the
//! wording. See `docs/testing.md`.
#![cfg(feature = "demo")]

use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Msg};
use review_buddy::config::{DetailMode, QueuePiece};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::layout::{Options, Size, Split};

#[path = "support/render.rs"]
mod render_support;
use render_support::{plain_dump, render, rows, text};

fn app(theme: &str, size: (u16, u16), no_color: bool, options: Options) -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let snapshot = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(world.snapshot())
        .unwrap();
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color,
        size,
    });
    app.layout = options;
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    app
}

fn closed() -> Options {
    Options {
        detail: DetailMode::Closed,
        ..Options::default()
    }
}

/// Detail on the right with the Queue `width` columns wide.
fn queue_wide(width: u16) -> Options {
    Options {
        split: Split {
            queue_width: Some(Size::Cells(width)),
            ..Split::default()
        },
        ..Options::default()
    }
}

/// The part of `line` inside the Queue pane, which starts and ends where its top border does.
fn in_queue(app: &mut App, line: &str) -> String {
    let top = rows(&render(app))
        .into_iter()
        .find(|r| r.contains("╭ queue"))
        .expect("the queue pane has a top border");
    let from = top
        .find("╭ queue")
        .map(|byte| top[..byte].chars().count())
        .unwrap();
    let width = top.chars().skip(from).take_while(|c| *c != '╮').count() + 1;
    line.chars().skip(from).take(width).collect()
}

/// The second line of the row whose reference starts with `reference`.
fn second_line(app: &mut App, reference: &str) -> String {
    let buffer = render(app);
    let all = rows(&buffer);
    let line = all
        .iter()
        .find(|r| r.contains(reference))
        .unwrap_or_else(|| panic!("no row for {reference}:\n{}", text(&buffer)));
    line.to_string()
}

#[test]
fn queue_status_160x40_default_theme() {
    let mut app = app("liminal-hq", (160, 40), false, closed());
    insta::assert_snapshot!(text(&render(&mut app)));
}

#[test]
fn queue_status_160x40_dusk() {
    let mut app = app("dusk", (160, 40), false, closed());
    insta::assert_snapshot!(text(&render(&mut app)));
}

#[test]
fn queue_status_160x40_no_colour() {
    let mut app = app("liminal-hq", (160, 40), true, closed());
    insta::assert_snapshot!(plain_dump(&mut app));
}

#[test]
fn queue_status_100x30_default_theme() {
    let mut app = app("liminal-hq", (100, 30), false, Options::default());
    insta::assert_snapshot!(text(&render(&mut app)));
}

#[test]
fn queue_status_100x30_dusk() {
    let mut app = app("dusk", (100, 30), false, Options::default());
    insta::assert_snapshot!(text(&render(&mut app)));
}

#[test]
fn queue_status_100x30_no_colour() {
    let mut app = app("liminal-hq", (100, 30), true, Options::default());
    insta::assert_snapshot!(plain_dump(&mut app));
}

#[test]
fn the_demo_shows_every_state_in_words_and_glyphs() {
    let mut app = app("liminal-hq", (160, 40), true, closed());
    let all = text(&render(&mut app));
    for needle in [
        "✓1",
        "✕1",
        "○1",
        "¶6",
        "2 open",
        "CI running",
        "CI failing",
        "+",
        "−",
    ] {
        assert!(all.contains(needle), "{needle} is on the queue:\n{all}");
    }
    let approved = second_line(&mut app, "GH review-buddy#209");
    assert!(approved.contains("approved by you"), "{approved}");
    assert!(
        approved.contains("✓1") && approved.contains("✕1"),
        "{approved}"
    );
}

#[test]
fn ci_failing_is_not_repeated_when_the_reason_already_says_it() {
    let mut app = app("liminal-hq", (160, 40), false, closed());
    let line = second_line(&mut app, "flow!1182");
    assert!(line.contains("CI failing"), "{line}");
    let mut change = app.state.changes[0].clone();
    change.ci = rb_core::CiState::Fail;
    change.my_role = rb_core::MyRole::Authored;
    change.my_review = rb_core::MyReview::None;
    change.id.number = 4242;
    change.title = "Own failing change".into();
    app.state.changes.push(change);
    app.change_count = app.state.changes.len();
    app.mark_dirty();
    let line = second_line(&mut app, "#4242");
    assert_eq!(line.matches("CI failing").count(), 1, "{line}");
}

#[test]
fn a_narrow_queue_drops_pieces_from_the_right_and_keeps_the_reason() {
    // The cluster pieces are 11, 12, 10 and 11 cells with two between. The longest second line is
    // 46 cells, and the cluster wants two more for the gap and one for the margin, inside two
    // border cells, so each piece needs the Queue this wide: 62, 76, 88 and 101.
    let all = [
        ("GH review-buddy#209", "✓1"),
        ("GH review-buddy#214", "¶6"),
        ("GH review-buddy#214", "CI running"),
        ("GH review-buddy#214", "+32"),
    ];
    for (width, kept) in [
        (101, 4),
        (100, 3),
        (88, 3),
        (87, 2),
        (76, 2),
        (75, 1),
        (62, 1),
        (61, 0),
    ] {
        let mut app = app("liminal-hq", (200, 40), false, queue_wide(width));
        let line = second_line(&mut app, "GH review-buddy#214");
        assert!(line.contains("review requested"), "{width}: {line}");
        for (i, (reference, needle)) in all.iter().enumerate() {
            let line = second_line(&mut app, reference);
            let line = in_queue(&mut app, &line);
            assert_eq!(
                line.contains(needle),
                i < kept,
                "{width} should keep {kept} pieces, {needle}: {line}"
            );
        }
    }
}

#[test]
fn the_title_and_age_keep_their_place_at_every_width() {
    for width in [50_u16, 62, 76, 88, 101] {
        let mut app = app("liminal-hq", (200, 40), false, queue_wide(width));
        let line = second_line(&mut app, "◐ Add a menu bar");
        let queue = in_queue(&mut app, &line);
        assert!(queue.contains("Add a menu bar"), "{width}: {queue}");
        assert!(queue.trim_end().ends_with("2h │"), "{width}: {queue}");
    }
}

#[test]
fn columns_line_up_down_the_list() {
    let mut app = app("liminal-hq", (160, 40), true, closed());
    let buffer = render(&mut app);
    let all = rows(&buffer);
    let col = |needle: &str| {
        let line = all.iter().find(|r| r.contains(needle)).unwrap();
        let byte = line.find(needle).unwrap();
        line[..byte].chars().count()
    };
    assert_eq!(
        col("¶6"),
        col("¶3"),
        "the comment column is the same on every row"
    );
    assert_eq!(col("¶6"), col("¶5"));
}

#[test]
fn the_choice_of_pieces_decides_what_shows() {
    let mut app = app("liminal-hq", (160, 40), false, closed());
    app.queue_status = vec![QueuePiece::Size, QueuePiece::Ci];
    let line = second_line(&mut app, "GH review-buddy#214");
    assert!(line.contains("CI running") && line.contains("+"), "{line}");
    assert!(!line.contains('¶') && !line.contains("✓"), "{line}");

    app.queue_status = Vec::new();
    let all = text(&render(&mut app));
    for needle in ["¶", "✓1", "CI running", "CI failing", "2 open"] {
        assert!(!all.contains(needle), "off drops {needle}:\n{all}");
    }
    assert!(all.contains("review requested"));
}

#[test]
fn old_rows_without_counts_read_as_blank_not_zero() {
    let mut app = app("liminal-hq", (160, 40), false, closed());
    for change in &mut app.state.changes {
        change.signals = Default::default();
    }
    app.state.details.clear();
    app.mark_dirty();
    let all = text(&render(&mut app));
    assert!(!all.contains('¶'), "{all}");
    assert!(!all.contains("✓"), "{all}");
}
