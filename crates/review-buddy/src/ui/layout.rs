//! Where the dashboard's panes sit. Pure geometry, shared by drawing and by `update`
//! (which needs viewport sizes to keep the selection on screen).

use ratatui::layout::{Constraint, Layout, Rect};

use crate::config::{DetailMode, SourcesLayout};

pub const SOURCES_WIDTH: u16 = 26;
pub const QUEUE_WIDTH: u16 = 48;
/// Below this many columns the Sources pane folds into a strip of tabs.
pub const COLLAPSE_BELOW: u16 = 130;

/// With the Sources pane replaced by the strip (`ui.sources = "top"`), the Queue takes some of
/// the freed width once the terminal is wide enough to have shown the pane.
pub const QUEUE_WIDTH_TOP: u16 = 60;

/// The dashboard's layout choices: `ui.sources` and `ui.detail`, as changed during a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub sources: SourcesLayout,
    pub detail: DetailMode,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            sources: SourcesLayout::Auto,
            detail: DetailMode::Auto,
        }
    }
}

impl Options {
    /// Whether Sources is the strip of tabs rather than a pane.
    pub fn collapsed(self, width: u16) -> bool {
        match self.sources {
            SourcesLayout::Auto => width < COLLAPSE_BELOW,
            SourcesLayout::Left => false,
            SourcesLayout::Top => true,
        }
    }

    pub fn detail_open(self) -> bool {
        self.detail != DetailMode::Closed
    }

    /// `p`: closes the Detail pane, or reopens it.
    pub fn toggled_detail(self) -> DetailMode {
        if self.detail_open() {
            DetailMode::Closed
        } else {
            DetailMode::Open
        }
    }

    /// `S`: auto, then left, then top.
    pub fn next_sources(self) -> SourcesLayout {
        match self.sources {
            SourcesLayout::Auto => SourcesLayout::Left,
            SourcesLayout::Left => SourcesLayout::Top,
            SourcesLayout::Top => SourcesLayout::Auto,
        }
    }
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

pub fn dashboard(body: Rect, options: Options) -> DashboardLayout {
    let collapsed = options.collapsed(body.width);
    let (strip, rest) = if collapsed {
        let [strip, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(body);
        (Some(strip), rest)
    } else {
        (None, body)
    };
    let (sources, rest) = if collapsed {
        (None, rest)
    } else {
        let [sources, rest] =
            Layout::horizontal([Constraint::Length(SOURCES_WIDTH), Constraint::Min(0)]).areas(rest);
        (Some(sources), rest)
    };
    let wide_top = options.sources == SourcesLayout::Top && body.width >= COLLAPSE_BELOW;
    let (queue, detail) = if !options.detail_open() {
        let detail = Rect::new(rest.right(), rest.y, 0, rest.height);
        (rest, detail)
    } else {
        let width = if wide_top {
            QUEUE_WIDTH_TOP
        } else {
            QUEUE_WIDTH
        };
        let [queue, detail] =
            Layout::horizontal([Constraint::Length(width), Constraint::Min(0)]).areas(rest);
        (queue, detail)
    };
    DashboardLayout {
        strip,
        sources,
        queue,
        detail,
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

/// Text rows the composer shows before it scrolls.
pub const COMPOSER_MIN_ROWS: usize = 3;
pub const COMPOSER_MAX_ROWS: usize = 8;

/// The composer's bottom-docked rectangle over the diff's rows, border included.
pub fn composer_dock(code: Rect, lines: usize) -> Rect {
    let rows = lines.clamp(COMPOSER_MIN_ROWS, COMPOSER_MAX_ROWS) as u16;
    let height = (rows + 2).min(code.height.saturating_sub(3)).max(3);
    let height = height.min(code.height);
    Rect::new(code.x, code.bottom() - height, code.width, height)
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
    fn the_composer_docks_at_the_bottom_and_grows_with_its_text() {
        let code = Rect::new(35, 2, 124, 36);
        let one = composer_dock(code, 1);
        assert_eq!((one.x, one.width, one.bottom()), (35, 124, 38));
        assert_eq!(one.height, 5);
        assert_eq!(composer_dock(code, 6).height, 8);
        assert_eq!(composer_dock(code, 50).height, 10, "capped");
        assert!(composer_dock(Rect::new(0, 0, 40, 6), 8).height <= 6);
    }

    #[test]
    fn wide_layout_has_three_panes_that_fill_the_width() {
        let l = dashboard(body((160, 40)), Options::default());
        assert_eq!(l.strip, None);
        let s = l.sources.unwrap();
        assert_eq!((s.width, l.queue.width, l.detail.width), (26, 48, 86));
        assert_eq!(l.queue.x, 26);
        assert_eq!(l.detail.x, 74);
        assert_eq!(l.queue.height, 38);
    }

    #[test]
    fn collapse_starts_below_130() {
        assert!(!Options::default().collapsed(130));
        assert!(Options::default().collapsed(129));
        let l = dashboard(body((130, 40)), Options::default());
        assert!(l.sources.is_some());
        let l = dashboard(body((129, 40)), Options::default());
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

    fn opts(sources: SourcesLayout, detail: DetailMode) -> Options {
        Options { sources, detail }
    }

    fn fills(l: &DashboardLayout, width: u16) {
        let left = l.sources.map_or(0, |s| s.width);
        assert_eq!(left + l.queue.width + l.detail.width, width);
    }

    #[test]
    fn left_keeps_the_pane_even_when_narrow() {
        let l = dashboard(body((110, 40)), opts(SourcesLayout::Left, DetailMode::Auto));
        assert!(l.strip.is_none() && l.sources.is_some());
        fills(&l, 110);
    }

    #[test]
    fn top_forces_the_strip_and_hands_width_to_the_queue_when_wide() {
        let o = opts(SourcesLayout::Top, DetailMode::Auto);
        let l = dashboard(body((160, 40)), o);
        assert!(l.strip.is_some() && l.sources.is_none());
        assert_eq!((l.queue.width, l.detail.width), (QUEUE_WIDTH_TOP, 100));
        let l = dashboard(body((129, 40)), o);
        assert_eq!(l.queue.width, QUEUE_WIDTH);
        fills(&l, 129);
    }

    #[test]
    fn closed_detail_gives_the_queue_the_rest() {
        for width in [100, 129, 130, 160] {
            let l = dashboard(
                body((width, 40)),
                opts(SourcesLayout::Auto, DetailMode::Closed),
            );
            assert_eq!(l.detail.width, 0);
            fills(&l, width);
        }
        let l = dashboard(
            body((160, 40)),
            opts(SourcesLayout::Top, DetailMode::Closed),
        );
        assert_eq!(l.queue.width, 160);
        assert_eq!(l.queue.y, 2);
        let l = dashboard(
            body((160, 40)),
            opts(SourcesLayout::Auto, DetailMode::Closed),
        );
        assert_eq!(l.queue.width, 134);
    }

    #[test]
    fn open_and_auto_detail_are_alike_and_toggles_cycle() {
        let a = dashboard(body((160, 40)), opts(SourcesLayout::Auto, DetailMode::Open));
        assert_eq!(a, dashboard(body((160, 40)), Options::default()));
        let o = Options::default();
        assert_eq!(o.toggled_detail(), DetailMode::Closed);
        assert_eq!(
            opts(SourcesLayout::Auto, DetailMode::Closed).toggled_detail(),
            DetailMode::Open
        );
        assert_eq!(o.next_sources(), SourcesLayout::Left);
        assert_eq!(
            opts(SourcesLayout::Top, DetailMode::Auto).next_sources(),
            SourcesLayout::Auto
        );
    }
}
