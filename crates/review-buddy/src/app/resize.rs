//! Resizing the dashboard's splits: dragging a seam with the mouse, nudging from the keyboard,
//! resetting to automatic, and saving the sizes to the config file.

use ratatui::layout::Rect;

use super::dashboard::Pane;
use super::update::{self, set_status};
use super::{App, Cmd, Entry, Notice, NoticeKind, Screen};
use crate::ui::layout::{self, Seam, SeamKind};

/// A seam being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drag {
    pub seam: Seam,
    /// The boundary minus the pointer where the press landed, so the seam doesn't jump.
    offset: i32,
}

fn body(app: &App) -> Rect {
    super::terminal::app_body(app)
}

/// A left press on a seam starts a drag, or resets that split when it is a second press.
/// Returns `None` when the press isn't on a seam.
pub fn press(app: &mut App, column: u16, row: u16, double: bool) -> Option<Vec<Cmd>> {
    if app.screen != Screen::Dashboard {
        return None;
    }
    let seam = layout::seam_at(body(app), app.layout, column, row)?;
    app.last_click = None;
    if double {
        app.last_seam = None;
        app.drag = None;
        return Some(reset(app, seam.kind));
    }
    app.last_seam = Some((seam.kind, app.ticks));
    let pointer = if seam.vertical { column } else { row };
    app.drag = Some(Drag {
        seam,
        offset: i32::from(seam.boundary) - i32::from(pointer),
    });
    app.mark_dirty();
    Some(Vec::new())
}

pub fn is_double(app: &App, column: u16, row: u16, window: u64) -> bool {
    let Some(seam) = layout::seam_at(body(app), app.layout, column, row) else {
        return false;
    };
    app.last_seam
        .is_some_and(|(kind, tick)| kind == seam.kind && app.ticks.saturating_sub(tick) <= window)
}

pub fn drag(app: &mut App, column: u16, row: u16) {
    let Some(drag) = app.drag else {
        return;
    };
    let pointer = if drag.seam.vertical { column } else { row };
    let boundary = (i32::from(pointer) + drag.offset).clamp(0, i32::from(u16::MAX)) as u16;
    let area = body(app);
    layout::drag_to(&mut app.layout, area, &drag.seam, boundary);
    let text = layout::describe(app.layout, area, drag.seam.kind);
    let id = app.take_id();
    app.status = Some(Entry {
        id,
        notice: Notice::new(NoticeKind::Info, text),
    });
    super::dashboard::on_resize(app);
    app.mark_dirty();
}

/// Ends a drag, leaving the final size in the status line for a moment.
pub fn release(app: &mut App) -> Vec<Cmd> {
    let Some(drag) = app.drag.take() else {
        return Vec::new();
    };
    let text = layout::describe(app.layout, body(app), drag.seam.kind);
    set_status(app, Notice::new(NoticeKind::Info, text))
}

/// The seam `<`, `>` and `=` act on: the one next to the focused pane.
fn target(app: &App) -> Option<(SeamKind, i32)> {
    let seams = layout::seams(body(app), app.layout);
    let has = |kind| seams.iter().any(|s| s.kind == kind);
    match app.dashboard.focus {
        Pane::Sources if has(SeamKind::Sources) => Some((SeamKind::Sources, 1)),
        Pane::Queue if has(SeamKind::Queue) => Some((SeamKind::Queue, 1)),
        Pane::Detail if has(SeamKind::Queue) => Some((SeamKind::Queue, -1)),
        Pane::Queue if has(SeamKind::Sources) => Some((SeamKind::Sources, -1)),
        Pane::Detail if has(SeamKind::Sources) => Some((SeamKind::Sources, -1)),
        _ => None,
    }
}

fn nothing_to_resize(app: &mut App) -> Vec<Cmd> {
    let text = "There's no seam next to this pane to resize.";
    set_status(app, Notice::new(NoticeKind::Info, text))
}

/// `>` grows the focused pane and `<` shrinks it, by two columns or rows.
pub fn nudge(app: &mut App, grow: bool) -> Vec<Cmd> {
    let Some((kind, sign)) = target(app) else {
        return nothing_to_resize(app);
    };
    let delta = layout::NUDGE * sign * if grow { 1 } else { -1 };
    let area = body(app);
    layout::nudge(&mut app.layout, area, kind, delta);
    super::dashboard::on_resize(app);
    let text = layout::describe(app.layout, area, kind);
    set_status(app, Notice::new(NoticeKind::Info, text))
}

/// `=` puts the split next to the focused pane back to its automatic size.
pub fn reset_focused(app: &mut App) -> Vec<Cmd> {
    match target(app) {
        Some((kind, _)) => reset(app, kind),
        None => nothing_to_resize(app),
    }
}

fn reset(app: &mut App, kind: SeamKind) -> Vec<Cmd> {
    let area = body(app);
    layout::reset(&mut app.layout, area, kind);
    super::dashboard::on_resize(app);
    let text = format!("{} (automatic)", layout::describe(app.layout, area, kind));
    set_status(app, Notice::new(NoticeKind::Info, text))
}

/// `W` writes the Queue sizes to the config file, when there is one to write.
pub fn save(app: &mut App) -> Vec<Cmd> {
    if app.demo {
        return update::push_toast(
            app,
            Notice::new(
                NoticeKind::Info,
                "Demo mode doesn't write your config, so nothing was saved (demo).",
            ),
        );
    }
    let Some(target) = app.project_save.clone() else {
        return update::push_toast(
            app,
            Notice::new(
                NoticeKind::Info,
                "There's no config file to save to. Run review-buddy --setup to create one.",
            ),
        );
    };
    vec![Cmd::SaveLayoutSizes {
        target,
        queue_width: app.layout.split.queue_width,
        queue_height: app.layout.split.queue_height,
    }]
}

#[cfg(test)]
mod tests {
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use rb_theme::ColourDepth;

    use super::*;
    use crate::app::{AppConfig, Msg};
    use crate::config::{DetailMode, DetailPosition};
    use crate::ui::layout::Size;

    fn app(size: (u16, u16)) -> App {
        let mut a = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size,
        });
        a.layout.sources = crate::config::SourcesLayout::Left;
        a
    }

    fn mouse(a: &mut App, kind: MouseEventKind, column: u16, row: u16, mods: KeyModifiers) {
        update::update(
            a,
            Msg::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: mods,
            }),
        );
    }

    fn down(a: &mut App, c: u16, r: u16) {
        mouse(
            a,
            MouseEventKind::Down(MouseButton::Left),
            c,
            r,
            KeyModifiers::NONE,
        );
    }

    fn queue_seam(a: &App) -> Seam {
        layout::seams(body(a), a.layout)
            .into_iter()
            .find(|s| s.kind == SeamKind::Queue)
            .unwrap()
    }

    fn key(a: &mut App, code: KeyCode) -> Vec<Cmd> {
        update::update(a, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    #[test]
    fn down_on_the_seam_drags_live_and_up_ends_it() {
        let mut a = app((160, 40));
        let seam = queue_seam(&a);
        assert_eq!(a.layout.split.queue_width, None);
        down(&mut a, seam.boundary, 10);
        assert!(a.drag.is_some());
        mouse(
            &mut a,
            MouseEventKind::Drag(MouseButton::Left),
            seam.boundary + 6,
            10,
            KeyModifiers::NONE,
        );
        assert_eq!(a.layout.split.queue_width, Some(Size::Cells(54)));
        assert!(a
            .status
            .as_ref()
            .unwrap()
            .notice
            .text
            .contains("queue 54 columns"));
        mouse(
            &mut a,
            MouseEventKind::Up(MouseButton::Left),
            seam.boundary + 6,
            10,
            KeyModifiers::NONE,
        );
        assert!(a.drag.is_none());
        assert_eq!(a.layout.split.queue_width, Some(Size::Cells(54)));
    }

    #[test]
    fn the_border_column_on_either_side_grabs_without_a_jump() {
        let mut a = app((160, 40));
        let seam = queue_seam(&a);
        down(&mut a, seam.boundary - 1, 10);
        mouse(
            &mut a,
            MouseEventKind::Drag(MouseButton::Left),
            seam.boundary - 1,
            10,
            KeyModifiers::NONE,
        );
        assert_eq!(a.layout.split.queue_width, Some(Size::Cells(48)));
    }

    #[test]
    fn a_press_off_the_seam_starts_nothing() {
        let mut a = app((160, 40));
        let seam = queue_seam(&a);
        down(&mut a, seam.boundary + 5, 10);
        mouse(
            &mut a,
            MouseEventKind::Drag(MouseButton::Left),
            seam.boundary + 9,
            10,
            KeyModifiers::NONE,
        );
        assert!(a.drag.is_none());
        assert_eq!(a.layout.split, layout::Split::default());
    }

    #[test]
    fn shift_events_never_resize() {
        let mut a = app((160, 40));
        let seam = queue_seam(&a);
        mouse(
            &mut a,
            MouseEventKind::Down(MouseButton::Left),
            seam.boundary,
            10,
            KeyModifiers::SHIFT,
        );
        assert!(a.drag.is_none());
        down(&mut a, seam.boundary, 10);
        mouse(
            &mut a,
            MouseEventKind::Drag(MouseButton::Left),
            seam.boundary + 8,
            10,
            KeyModifiers::SHIFT,
        );
        assert_eq!(a.layout.split.queue_width, None);
    }

    #[test]
    fn dragging_clamps_to_the_minimums() {
        let mut a = app((160, 40));
        let seam = queue_seam(&a);
        down(&mut a, seam.boundary, 10);
        mouse(
            &mut a,
            MouseEventKind::Drag(MouseButton::Left),
            28,
            10,
            KeyModifiers::NONE,
        );
        let l = layout::dashboard(body(&a), a.layout);
        assert_eq!(l.queue.width, 30);
        mouse(
            &mut a,
            MouseEventKind::Drag(MouseButton::Left),
            159,
            10,
            KeyModifiers::NONE,
        );
        let l = layout::dashboard(body(&a), a.layout);
        assert_eq!(l.detail.width, 36);
    }

    #[test]
    fn a_second_press_on_the_seam_resets_it() {
        let mut a = app((160, 40));
        let seam = queue_seam(&a);
        down(&mut a, seam.boundary, 10);
        mouse(
            &mut a,
            MouseEventKind::Drag(MouseButton::Left),
            seam.boundary + 6,
            10,
            KeyModifiers::NONE,
        );
        mouse(
            &mut a,
            MouseEventKind::Up(MouseButton::Left),
            seam.boundary + 6,
            10,
            KeyModifiers::NONE,
        );
        let moved = queue_seam(&a);
        down(&mut a, moved.boundary, 10);
        mouse(
            &mut a,
            MouseEventKind::Up(MouseButton::Left),
            moved.boundary,
            10,
            KeyModifiers::NONE,
        );
        down(&mut a, moved.boundary, 10);
        assert_eq!(a.layout.split.queue_width, None);
        assert!(a.drag.is_none());
    }

    #[test]
    fn stacked_drags_set_the_height_and_keep_the_width() {
        let mut a = app((100, 30));
        let seam = queue_seam(&a);
        assert!(!seam.vertical);
        let before = layout::dashboard(body(&a), a.layout).queue.height;
        down(&mut a, 40, seam.boundary);
        mouse(
            &mut a,
            MouseEventKind::Drag(MouseButton::Left),
            40,
            seam.boundary + 3,
            KeyModifiers::NONE,
        );
        assert_eq!(a.layout.split.queue_height, Some(Size::Cells(before + 3)));
        assert_eq!(a.layout.split.queue_width, None);
    }

    #[test]
    fn each_arrangement_remembers_its_own_size() {
        let mut a = app((160, 40));
        a.layout.split.queue_width = Some(Size::Cells(60));
        a.layout.split.queue_height = Some(Size::Cells(12));
        a.layout.position = DetailPosition::Bottom;
        assert_eq!(layout::dashboard(body(&a), a.layout).queue.height, 12);
        a.layout.position = DetailPosition::Right;
        assert_eq!(layout::dashboard(body(&a), a.layout).queue.width, 60);
    }

    #[test]
    fn keys_nudge_the_split_next_to_the_focused_pane() {
        let mut a = app((160, 40));
        a.dashboard.focus = Pane::Queue;
        key(&mut a, KeyCode::Char('>'));
        assert_eq!(a.layout.split.queue_width, Some(Size::Cells(50)));
        key(&mut a, KeyCode::Char('<'));
        key(&mut a, KeyCode::Char('<'));
        assert_eq!(a.layout.split.queue_width, Some(Size::Cells(46)));
        a.dashboard.focus = Pane::Detail;
        key(&mut a, KeyCode::Char('>'));
        assert_eq!(a.layout.split.queue_width, Some(Size::Cells(44)));
        key(&mut a, KeyCode::Char('='));
        assert_eq!(a.layout.split.queue_width, None);
        a.dashboard.focus = Pane::Sources;
        key(&mut a, KeyCode::Char('>'));
        assert_eq!(a.layout.split.sources_width, Some(Size::Cells(28)));
    }

    #[test]
    fn a_closed_detail_has_only_the_sources_seam() {
        let mut a = app((160, 40));
        a.layout.detail = DetailMode::Closed;
        assert_eq!(layout::seams(body(&a), a.layout).len(), 1);
        a.dashboard.focus = Pane::Queue;
        key(&mut a, KeyCode::Char('>'));
        assert_eq!(a.layout.split.sources_width, Some(Size::Cells(24)));
    }

    #[test]
    fn save_needs_a_writable_config_and_never_writes_in_demo() {
        let mut a = app((160, 40));
        a.layout.split.queue_width = Some(Size::Cells(52));
        let cmds = key(&mut a, KeyCode::Char('W'));
        assert!(!cmds
            .iter()
            .any(|c| matches!(c, Cmd::SaveLayoutSizes { .. })));
        assert_eq!(a.toasts.len(), 1);
        a.project_save = Some("/c/config.toml".into());
        let cmds = key(&mut a, KeyCode::Char('W'));
        assert!(matches!(
            &cmds[..],
            [Cmd::SaveLayoutSizes {
                queue_width: Some(Size::Cells(52)),
                queue_height: None,
                ..
            }]
        ));
        a.demo = true;
        let cmds = key(&mut a, KeyCode::Char('W'));
        assert!(!cmds
            .iter()
            .any(|c| matches!(c, Cmd::SaveLayoutSizes { .. })));
    }
}
