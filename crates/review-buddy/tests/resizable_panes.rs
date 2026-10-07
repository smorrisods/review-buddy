//! Dragging the seams between dashboard panes, headless: snapshots of dragged layouts, the
//! accent cue while dragging, hit-testing through a real draw, and the minimums.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use rb_theme::{ColourDepth, Role};
use review_buddy::app::{update, App, AppConfig, Msg};
use review_buddy::config::DetailPosition;
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::layout::{self, Options, Size};

#[path = "support/render.rs"]
mod render_support;
use render_support::{render, text};

fn app(size: (u16, u16), position: DetailPosition) -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let snapshot = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(world.snapshot())
        .unwrap();
    let mut app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size,
    });
    app.layout = Options {
        position,
        ..Options::default()
    };
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    render(&mut app);
    app
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) {
    update(
        app,
        Msg::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers,
        }),
    );
}

fn queue_seam(app: &App) -> layout::Seam {
    layout::seams(layout::body(app.size), app.layout)
        .into_iter()
        .find(|s| s.kind == layout::SeamKind::Queue)
        .unwrap()
}

/// Presses on the Queue seam and drags it `by` columns or rows, leaving the button down.
fn drag_by(app: &mut App, by: i32) {
    let seam = queue_seam(app);
    let (c, r) = if seam.vertical {
        (seam.boundary, 12)
    } else {
        (40, seam.boundary)
    };
    mouse(
        app,
        MouseEventKind::Down(MouseButton::Left),
        c,
        r,
        KeyModifiers::NONE,
    );
    let (c, r) = if seam.vertical {
        ((i32::from(c) + by) as u16, r)
    } else {
        (c, (i32::from(r) + by) as u16)
    };
    mouse(
        app,
        MouseEventKind::Drag(MouseButton::Left),
        c,
        r,
        KeyModifiers::NONE,
    );
    render(app);
}

macro_rules! dragged {
    ($($name:ident: $pos:ident, $w:literal, $h:literal, $by:literal;)*) => {
        $(
            #[test]
            fn $name() {
                let mut app = app(($w, $h), DetailPosition::$pos);
                drag_by(&mut app, $by);
                insta::assert_snapshot!(text(&render(&mut app)));
            }
        )*
    };
}

dragged! {
    dragged_side_by_side_160x40: Right, 160, 40, 10;
    dragged_side_by_side_100x30: Right, 100, 30, -8;
    dragged_stacked_160x40: Bottom, 160, 40, -6;
    dragged_stacked_100x30: Bottom, 100, 30, 4;
}

#[test]
fn the_seam_wears_the_accent_role_only_while_dragging() {
    let mut a = app((160, 40), DetailPosition::Right);
    let seam = queue_seam(&a);
    let accent = render_support::role_colour(&a, Role::Accent);
    let cell = |buf: &ratatui::buffer::Buffer| buf[(seam.boundary, 12)].fg;
    assert_ne!(cell(&render(&mut a)), accent);
    drag_by(&mut a, 0);
    assert_eq!(cell(&render(&mut a)), accent);
    let seam = queue_seam(&a);
    mouse(
        &mut a,
        MouseEventKind::Up(MouseButton::Left),
        seam.boundary,
        12,
        KeyModifiers::NONE,
    );
    assert_ne!(cell(&render(&mut a)), accent);
}

#[test]
fn a_drag_shows_its_size_and_up_leaves_it_in_place() {
    let mut a = app((160, 40), DetailPosition::Right);
    drag_by(&mut a, 6);
    assert!(text(&render(&mut a)).contains("queue 54 columns"));
    let seam = queue_seam(&a);
    mouse(
        &mut a,
        MouseEventKind::Up(MouseButton::Left),
        seam.boundary,
        12,
        KeyModifiers::NONE,
    );
    assert_eq!(a.layout.split.queue_width, Some(Size::Cells(54)));
}

#[test]
fn seams_in_both_arrangements_are_hit_after_a_real_draw() {
    let side = app((160, 40), DetailPosition::Right);
    let seam = queue_seam(&side);
    for column in [seam.boundary - 1, seam.boundary] {
        assert!(side.hits.at(column, 12).is_none(), "{column}");
        assert!(layout::seam_at(layout::body(side.size), side.layout, column, 12).is_some());
    }
    let stacked = app((100, 30), DetailPosition::Bottom);
    let seam = queue_seam(&stacked);
    for row in [seam.boundary - 1, seam.boundary] {
        assert!(layout::seam_at(layout::body(stacked.size), stacked.layout, 40, row).is_some());
    }
}

#[test]
fn contents_drags_and_shift_drags_do_not_resize() {
    let mut a = app((160, 40), DetailPosition::Right);
    let seam = queue_seam(&a);
    mouse(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        seam.boundary + 10,
        12,
        KeyModifiers::NONE,
    );
    mouse(
        &mut a,
        MouseEventKind::Drag(MouseButton::Left),
        seam.boundary + 20,
        12,
        KeyModifiers::NONE,
    );
    assert_eq!(a.layout.split, layout::Split::default());
    mouse(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        seam.boundary,
        12,
        KeyModifiers::SHIFT,
    );
    mouse(
        &mut a,
        MouseEventKind::Drag(MouseButton::Left),
        seam.boundary + 9,
        12,
        KeyModifiers::SHIFT,
    );
    assert_eq!(a.layout.split, layout::Split::default());
}

#[test]
fn keys_resize_and_a_resize_of_the_terminal_clamps() {
    let mut a = app((200, 40), DetailPosition::Right);
    for _ in 0..20 {
        update(
            &mut a,
            Msg::Key(KeyEvent::new(KeyCode::Char('>'), KeyModifiers::NONE)),
        );
    }
    assert_eq!(
        layout::dashboard(layout::body(a.size), a.layout)
            .queue
            .width,
        88
    );
    update(&mut a, Msg::Resize(110, 30));
    let l = layout::dashboard(layout::body(a.size), a.layout);
    assert!(l.detail.width >= 36 && l.queue.width >= 30);
    assert!(text(&render(&mut a)).contains("queue"));
}
