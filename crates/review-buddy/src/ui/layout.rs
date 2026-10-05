//! Where the dashboard's panes sit. Pure geometry, shared by drawing and by `update`
//! (which needs viewport sizes to keep the selection on screen).

use ratatui::layout::{Constraint, Layout, Rect};

pub const SOURCES_WIDTH: u16 = 26;
pub const QUEUE_WIDTH: u16 = 48;
/// Below this many columns the Sources pane folds into a strip of tabs.
pub const COLLAPSE_BELOW: u16 = 130;

pub fn is_collapsed(width: u16) -> bool {
    width < COLLAPSE_BELOW
}

/// The area between the top bar and the footer.
pub fn body(size: (u16, u16)) -> Rect {
    Rect::new(0, 1, size.0, size.1.saturating_sub(2))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardLayout {
    /// The source tabs, when Sources has collapsed.
    pub strip: Option<Rect>,
    pub sources: Option<Rect>,
    pub queue: Rect,
    pub detail: Rect,
}

pub fn dashboard(body: Rect) -> DashboardLayout {
    let collapsed = is_collapsed(body.width);
    let (strip, rest) = if collapsed {
        let [strip, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(body);
        (Some(strip), rest)
    } else {
        (None, body)
    };
    if collapsed {
        let [queue, detail] =
            Layout::horizontal([Constraint::Length(QUEUE_WIDTH), Constraint::Min(0)]).areas(rest);
        DashboardLayout {
            strip,
            sources: None,
            queue,
            detail,
        }
    } else {
        let [sources, queue, detail] = Layout::horizontal([
            Constraint::Length(SOURCES_WIDTH),
            Constraint::Length(QUEUE_WIDTH),
            Constraint::Min(0),
        ])
        .areas(rest);
        DashboardLayout {
            strip,
            sources: Some(sources),
            queue,
            detail,
        }
    }
}

/// The inside of a bordered pane.
pub fn inner(rect: Rect) -> Rect {
    Rect::new(
        rect.x.saturating_add(1),
        rect.y.saturating_add(1),
        rect.width.saturating_sub(2),
        rect.height.saturating_sub(2),
    )
}

/// The detail pane's text area: inside the border with a one-column margin each side.
pub fn detail_content(detail: Rect) -> Rect {
    let inside = inner(detail);
    Rect::new(
        inside.x.saturating_add(1),
        inside.y,
        inside.width.saturating_sub(2),
        inside.height,
    )
}

pub const FILES_WIDTH: u16 = 34;
/// Height of the review block at the foot of the Files pane, border included.
pub const REVIEW_HEIGHT: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffLayout {
    pub files: Rect,
    /// Where the file rows go, inside the Files pane's border.
    pub file_list: Rect,
    pub review: Rect,
    pub diff: Rect,
    /// The rows of the diff itself, inside the Diff pane's border.
    pub code: Rect,
}

pub fn diff_screen(body: Rect) -> DiffLayout {
    let [files, diff] =
        Layout::horizontal([Constraint::Length(FILES_WIDTH), Constraint::Min(0)]).areas(body);
    let inside = inner(files);
    let review_height = REVIEW_HEIGHT.min(inside.height);
    let [file_list, review] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(review_height)]).areas(inside);
    DiffLayout {
        files,
        file_list,
        review,
        diff,
        code: inner(diff),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_screen_splits_files_from_code() {
        let l = diff_screen(body((160, 40)));
        assert_eq!(l.files.width, 34);
        assert_eq!(l.diff.x, 34);
        assert_eq!(l.code.width, 124);
        assert_eq!(l.code.height, 36);
        assert_eq!(l.review.height, REVIEW_HEIGHT);
        assert_eq!(l.file_list.height + l.review.height, 36);
    }

    #[test]
    fn wide_layout_has_three_panes_that_fill_the_width() {
        let l = dashboard(body((160, 40)));
        assert_eq!(l.strip, None);
        let s = l.sources.unwrap();
        assert_eq!((s.width, l.queue.width, l.detail.width), (26, 48, 86));
        assert_eq!(l.queue.x, 26);
        assert_eq!(l.detail.x, 74);
        assert_eq!(l.queue.height, 38);
    }

    #[test]
    fn collapse_starts_below_130() {
        assert!(!is_collapsed(130));
        assert!(is_collapsed(129));
        let l = dashboard(body((130, 40)));
        assert!(l.sources.is_some());
        let l = dashboard(body((129, 40)));
        assert!(l.sources.is_none());
        let strip = l.strip.unwrap();
        assert_eq!((strip.y, strip.height), (1, 1));
        assert_eq!(l.queue.y, 2);
        assert_eq!(l.queue.width + l.detail.width, 129);
    }

    #[test]
    fn inner_shrinks_by_the_border_and_never_underflows() {
        assert_eq!(inner(Rect::new(2, 3, 10, 8)), Rect::new(3, 4, 8, 6));
        assert_eq!(inner(Rect::new(0, 0, 1, 1)).width, 0);
        assert_eq!(detail_content(Rect::new(0, 0, 20, 10)).width, 16);
    }
}
