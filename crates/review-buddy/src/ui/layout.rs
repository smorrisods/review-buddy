//! Where the dashboard's panes sit. Pure geometry, shared by drawing and by `update`
//! (which needs viewport sizes to keep the selection on screen).

use ratatui::layout::{Constraint, Layout, Rect};

use crate::config::{DetailMode, DetailPosition, SourcesLayout};

pub const SOURCES_WIDTH: u16 = 26;
pub const QUEUE_WIDTH: u16 = 48;
/// Below this many columns the Sources pane folds into a strip of tabs.
pub const COLLAPSE_BELOW: u16 = 130;

/// With the Sources pane replaced by the strip (`ui.sources = "top"`), the Queue takes some of
/// the freed width once the terminal is wide enough to have shown the pane.
pub const QUEUE_WIDTH_TOP: u16 = 60;

/// Side by side needs a Queue (48) and a useful Detail (50); `auto` also wants the whole
/// terminal at least this wide, so a narrow terminal stacks the panes instead.
pub const DETAIL_WIDTH_MIN: u16 = 50;
pub const SIDE_BY_SIDE_MIN: u16 = 110;
/// Stacked: the Queue gets this share of the height (percent), and never fewer rows than
/// [`QUEUE_ROWS_MIN`]; the Detail gets the rest, down to [`DETAIL_ROWS_MIN`] when there is room.
pub const QUEUE_SHARE: u16 = 55;
pub const QUEUE_ROWS_MIN: u16 = 8;
pub const DETAIL_ROWS_MIN: u16 = 6;

/// Smallest and largest sizes a dragged split may take.
pub const QUEUE_COLUMNS_MIN: u16 = 30;
pub const DETAIL_COLUMNS_MIN: u16 = 36;
pub const QUEUE_ROWS_DRAG_MIN: u16 = 6;
pub const SOURCES_WIDTH_MIN: u16 = 16;
pub const SOURCES_WIDTH_MAX: u16 = 60;
/// Columns or rows a keyboard nudge moves a split by.
pub const NUDGE: i32 = 2;

/// A pane size: a count of columns or rows, or a share of the space the split divides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Cells(u16),
    Percent(u8),
}

impl Size {
    /// Parses `52` or `60%`. Percentages run from 10 to 90 and cell counts from 1 up.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let bad = || {
            format!(
                "`{text}` isn't a size. Use a number of columns like 52, or a percentage like 60%"
            )
        };
        if let Some(number) = text.strip_suffix('%') {
            let n: u8 = number.trim().parse().map_err(|_| bad())?;
            if !(10..=90).contains(&n) {
                return Err(format!(
                    "{n}% is out of range. Use a percentage from 10% to 90%"
                ));
            }
            return Ok(Self::Percent(n));
        }
        match text.parse::<u16>() {
            Ok(0) | Err(_) => Err(bad()),
            Ok(n) => Ok(Self::Cells(n)),
        }
    }

    /// How many cells this is of `total`.
    pub fn resolve(self, total: u16) -> u16 {
        match self {
            Self::Cells(n) => n,
            Self::Percent(p) => (u32::from(total) * u32::from(p) / 100) as u16,
        }
    }
}

impl std::fmt::Display for Size {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cells(n) => write!(f, "{n}"),
            Self::Percent(p) => write!(f, "{p}%"),
        }
    }
}

/// Which seam: Sources against its neighbour, or Queue against Detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeamKind {
    Sources,
    Queue,
}

/// Session sizes for the dashboard's splits. `None` means automatic. The Queue's width and
/// height are separate, so each arrangement (side by side, stacked) keeps its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Split {
    pub queue_width: Option<Size>,
    pub queue_height: Option<Size>,
    pub sources_width: Option<Size>,
}

/// A draggable boundary between two panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seam {
    pub kind: SeamKind,
    /// A vertical line (columns resize) rather than a horizontal one (rows resize).
    pub vertical: bool,
    /// The column or row where the second pane starts.
    pub boundary: u16,
    /// The two border columns or rows next to the boundary.
    pub hit: Rect,
}

/// Where the Detail pane actually sits once `auto` has been resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Right,
    Left,
    Top,
    Bottom,
}

impl Place {
    /// What is on screen, in terms of the list (Queue) and the details.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Right => "list left, details right",
            Self::Left => "list right, details left",
            Self::Top => "list on bottom, details above",
            Self::Bottom => "list on top, details below",
        }
    }

    pub fn stacked(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }
}

/// The dashboard's layout choices: `ui.sources`, `ui.detail` and `ui.detail_position`, as
/// changed during a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub sources: SourcesLayout,
    pub detail: DetailMode,
    pub position: DetailPosition,
    pub split: Split,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            sources: SourcesLayout::Auto,
            detail: DetailMode::Auto,
            position: DetailPosition::Auto,
            split: Split::default(),
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

    /// Where Detail sits for a body this wide: the setting, or for `auto` right when the pair
    /// has room side by side and bottom otherwise.
    pub fn place(self, width: u16) -> Place {
        match self.position {
            DetailPosition::Right => Place::Right,
            DetailPosition::Left => Place::Left,
            DetailPosition::Top => Place::Top,
            DetailPosition::Bottom => Place::Bottom,
            DetailPosition::Auto => {
                let pair = if self.collapsed(width) {
                    width
                } else {
                    width.saturating_sub(SOURCES_WIDTH)
                };
                if width >= SIDE_BY_SIDE_MIN && pair >= QUEUE_WIDTH + DETAIL_WIDTH_MIN {
                    Place::Right
                } else {
                    Place::Bottom
                }
            }
        }
    }

    /// `P`: moves the list panel left → top → right → bottom, then `auto`. As detail positions
    /// that is auto → bottom → left → top → right → auto. A stop that would look the same as
    /// the layout showing now is skipped, and so is a fixed stop that looks the same as the
    /// `auto` stop right after it.
    pub fn next_position(self, width: u16) -> DetailPosition {
        const RING: [DetailPosition; 5] = [
            DetailPosition::Auto,
            DetailPosition::Bottom,
            DetailPosition::Left,
            DetailPosition::Top,
            DetailPosition::Right,
        ];
        let step = |p: DetailPosition| {
            let at = RING.iter().position(|r| *r == p).unwrap_or(0);
            RING[(at + 1) % RING.len()]
        };
        let look = |p: DetailPosition| {
            Options {
                position: p,
                ..self
            }
            .place(width)
        };
        let shown = look(self.position);
        let auto = look(DetailPosition::Auto);
        let mut next = step(self.position);
        for _ in 0..RING.len() {
            let same_as_shown = look(next) == shown;
            let same_as_auto_after = next != DetailPosition::Auto
                && look(next) == auto
                && step(next) == DetailPosition::Auto;
            if !same_as_shown && !same_as_auto_after {
                break;
            }
            next = step(next);
        }
        next
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
        let width = sources_width(body.width, options);
        let [sources, rest] =
            Layout::horizontal([Constraint::Length(width), Constraint::Min(0)]).areas(rest);
        (Some(sources), rest)
    };
    let wide_top = options.sources == SourcesLayout::Top && body.width >= COLLAPSE_BELOW;
    let (queue, detail) = if !options.detail_open() {
        let detail = Rect::new(rest.right(), rest.y, 0, rest.height);
        (rest, detail)
    } else {
        let place = options.place(body.width);
        if place.stacked() {
            let h = rest.height;
            let queue_h = match options.split.queue_height {
                Some(size) => {
                    let hi = h.saturating_sub(DETAIL_ROWS_MIN);
                    size.resolve(h).clamp(QUEUE_ROWS_DRAG_MIN.min(hi), hi)
                }
                None => (h * QUEUE_SHARE / 100)
                    .max(QUEUE_ROWS_MIN)
                    .min(h.saturating_sub(DETAIL_ROWS_MIN))
                    .max(QUEUE_ROWS_MIN.min(h)),
            };
            let detail_h = h - queue_h;
            if place == Place::Top {
                let [detail, queue] =
                    Layout::vertical([Constraint::Length(detail_h), Constraint::Min(0)])
                        .areas(rest);
                (queue, detail)
            } else {
                let [queue, detail] =
                    Layout::vertical([Constraint::Length(queue_h), Constraint::Min(0)]).areas(rest);
                (queue, detail)
            }
        } else {
            let width = match options.split.queue_width {
                Some(size) => {
                    let hi = rest.width.saturating_sub(DETAIL_COLUMNS_MIN);
                    size.resolve(rest.width)
                        .clamp(QUEUE_COLUMNS_MIN.min(hi), hi)
                }
                None if wide_top => QUEUE_WIDTH_TOP,
                None => QUEUE_WIDTH,
            };
            if place == Place::Left {
                let [detail, queue] =
                    Layout::horizontal([Constraint::Min(0), Constraint::Length(width)]).areas(rest);
                (queue, detail)
            } else {
                let [queue, detail] =
                    Layout::horizontal([Constraint::Length(width), Constraint::Min(0)]).areas(rest);
                (queue, detail)
            }
        }
    };
    DashboardLayout {
        strip,
        sources,
        queue,
        detail,
    }
}

fn sources_width(total: u16, options: Options) -> u16 {
    let Some(size) = options.split.sources_width else {
        return SOURCES_WIDTH;
    };
    let rest_min = QUEUE_COLUMNS_MIN
        + if options.detail_open() && !options.place(total).stacked() {
            DETAIL_COLUMNS_MIN
        } else {
            0
        };
    let hi = total.saturating_sub(rest_min).min(SOURCES_WIDTH_MAX);
    size.resolve(total).clamp(SOURCES_WIDTH_MIN.min(hi), hi)
}

/// The boundaries between panes that can be dragged, for this body and these options.
pub fn seams(body: Rect, options: Options) -> Vec<Seam> {
    let l = dashboard(body, options);
    let mut out = Vec::new();
    let vertical = |kind, boundary: u16, from: Rect| Seam {
        kind,
        vertical: true,
        boundary,
        hit: Rect::new(
            boundary.saturating_sub(1),
            from.y.saturating_add(1),
            2,
            from.height.saturating_sub(2),
        ),
    };
    if let Some(sources) = l.sources {
        out.push(vertical(SeamKind::Sources, sources.right(), sources));
    }
    if options.detail_open() {
        let seam = match options.place(body.width) {
            Place::Right => vertical(SeamKind::Queue, l.queue.right(), l.queue),
            Place::Left => vertical(SeamKind::Queue, l.queue.x, l.queue),
            place => {
                let boundary = if place == Place::Bottom {
                    l.queue.bottom()
                } else {
                    l.queue.y
                };
                Seam {
                    kind: SeamKind::Queue,
                    vertical: false,
                    boundary,
                    hit: Rect::new(
                        l.queue.x.saturating_add(1),
                        boundary.saturating_sub(1),
                        l.queue.width.saturating_sub(2),
                        2,
                    ),
                }
            }
        };
        out.push(seam);
    }
    out
}

pub fn seam_at(body: Rect, options: Options, column: u16, row: u16) -> Option<Seam> {
    seams(body, options).into_iter().find(|s| {
        column >= s.hit.x && column < s.hit.right() && row >= s.hit.y && row < s.hit.bottom()
    })
}

/// Moves a seam so the second pane starts at `boundary` (column or row), clamped by the
/// layout. Returns the size that was stored.
pub fn drag_to(options: &mut Options, body: Rect, seam: &Seam, boundary: u16) -> Size {
    let l = dashboard(body, *options);
    let boundary = i32::from(boundary);
    let size = match (seam.kind, options.place(body.width)) {
        (SeamKind::Sources, _) => boundary - i32::from(body.x),
        (SeamKind::Queue, Place::Right) => boundary - i32::from(l.queue.x),
        (SeamKind::Queue, Place::Left) => i32::from(l.queue.right()) - boundary,
        (SeamKind::Queue, Place::Bottom) => boundary - i32::from(l.queue.y),
        (SeamKind::Queue, Place::Top) => i32::from(l.queue.bottom()) - boundary,
    };
    set_cells(
        options,
        seam.kind,
        size.clamp(1, i32::from(u16::MAX)) as u16,
        body,
    )
}

fn set_cells(options: &mut Options, kind: SeamKind, cells: u16, body: Rect) -> Size {
    let size = Size::Cells(cells);
    match kind {
        SeamKind::Sources => options.split.sources_width = Some(size),
        SeamKind::Queue if options.place(body.width).stacked() => {
            options.split.queue_height = Some(size);
        }
        SeamKind::Queue => options.split.queue_width = Some(size),
    }
    size
}

/// The size a split has right now, in columns or rows, after clamping.
pub fn current(options: Options, body: Rect, kind: SeamKind) -> u16 {
    let l = dashboard(body, options);
    match kind {
        SeamKind::Sources => l.sources.map_or(0, |s| s.width),
        SeamKind::Queue if options.place(body.width).stacked() => l.queue.height,
        SeamKind::Queue => l.queue.width,
    }
}

/// Grows (`delta` above zero) or shrinks the Queue side of a seam, or the Sources pane.
pub fn nudge(options: &mut Options, body: Rect, kind: SeamKind, delta: i32) -> u16 {
    let now = i32::from(current(*options, body, kind));
    let next = (now + delta).clamp(1, i32::from(u16::MAX)) as u16;
    set_cells(options, kind, next, body);
    current(*options, body, kind)
}

/// Puts one split back to its automatic size.
pub fn reset(options: &mut Options, body: Rect, kind: SeamKind) {
    match kind {
        SeamKind::Sources => options.split.sources_width = None,
        SeamKind::Queue if options.place(body.width).stacked() => {
            options.split.queue_height = None;
        }
        SeamKind::Queue => options.split.queue_width = None,
    }
}

/// A short phrase for the status line: `queue 52 columns`.
pub fn describe(options: Options, body: Rect, kind: SeamKind) -> String {
    let n = current(options, body, kind);
    let name = match kind {
        SeamKind::Sources => "sources",
        SeamKind::Queue => "queue",
    };
    let unit = if kind == SeamKind::Queue && options.place(body.width).stacked() {
        "rows"
    } else {
        "columns"
    };
    format!("{name} {n} {unit}")
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
    diff_screen_with(body, 0)
}

/// The most rows the review block grows by for the pending-comment list and its notes.
pub const REVIEW_EXTRA_MAX: u16 = 6;

/// Like [`diff_screen`], with the review block `extra` rows taller (never more than half the
/// Files pane).
pub fn diff_screen_with(body: Rect, extra: u16) -> DiffLayout {
    let [files, diff] =
        Layout::horizontal([Constraint::Length(FILES_WIDTH), Constraint::Min(0)]).areas(body);
    let inside = inner(files);
    let extra = extra.min(REVIEW_EXTRA_MAX).min(inside.height / 2);
    let review_height = (REVIEW_HEIGHT + extra).min(inside.height);
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
        Options {
            sources,
            detail,
            position: DetailPosition::Right,
            split: Split::default(),
        }
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

#[cfg(test)]
mod position_tests {
    use super::*;

    fn opts(position: DetailPosition) -> Options {
        Options {
            position,
            ..Options::default()
        }
    }

    fn body_of(w: u16, h: u16) -> Rect {
        body((w, h + 2))
    }

    #[test]
    fn auto_is_right_from_110_columns_and_bottom_below() {
        for w in 40..=200 {
            let place = Options::default().place(w);
            let side = w >= 110;
            assert_eq!(place == Place::Right, side, "width {w}");
            if !side {
                assert_eq!(place, Place::Bottom, "width {w}");
            }
        }
    }

    #[test]
    fn auto_needs_room_for_both_panes_beside_a_sources_pane() {
        let left = Options {
            sources: SourcesLayout::Left,
            ..Options::default()
        };
        assert_eq!(left.place(120), Place::Bottom);
        assert_eq!(left.place(124), Place::Right);
        assert_eq!(left.place(160), Place::Right);
    }

    #[test]
    fn explicit_positions_ignore_width() {
        for (p, want) in [
            (DetailPosition::Right, Place::Right),
            (DetailPosition::Left, Place::Left),
            (DetailPosition::Top, Place::Top),
            (DetailPosition::Bottom, Place::Bottom),
        ] {
            for w in [60, 100, 160, 250] {
                assert_eq!(opts(p).place(w), want);
            }
        }
    }

    #[test]
    fn right_keeps_queue_48_and_detail_the_rest() {
        let l = dashboard(body_of(120, 30), opts(DetailPosition::Right));
        assert_eq!((l.queue.x, l.queue.width), (0, QUEUE_WIDTH));
        assert_eq!((l.detail.x, l.detail.width), (48, 72));
    }

    #[test]
    fn left_puts_detail_first() {
        let l = dashboard(body_of(120, 30), opts(DetailPosition::Left));
        assert_eq!((l.detail.x, l.detail.width), (0, 72));
        assert_eq!((l.queue.x, l.queue.width), (72, QUEUE_WIDTH));
    }

    #[test]
    fn stacked_splits_the_height_for_many_sizes() {
        for h in [10u16, 14, 20, 28, 38, 60] {
            for w in [80u16, 100, 160] {
                let area = body_of(w, h);
                let rest_h = if Options::default().collapsed(w) {
                    h - 1
                } else {
                    h
                };
                for p in [DetailPosition::Top, DetailPosition::Bottom] {
                    let l = dashboard(area, opts(p));
                    assert_eq!(l.queue.height + l.detail.height, rest_h, "{w}x{h}");
                    assert!(l.queue.height >= QUEUE_ROWS_MIN.min(rest_h), "{w}x{h}");
                    assert_eq!(l.queue.width, l.detail.width);
                    assert_eq!(l.queue.x, l.detail.x);
                    if p == DetailPosition::Top {
                        assert_eq!(l.detail.bottom(), l.queue.y);
                    } else {
                        assert_eq!(l.queue.bottom(), l.detail.y);
                    }
                }
            }
        }
    }

    #[test]
    fn stacked_queue_is_about_55_percent() {
        let l = dashboard(body_of(100, 40), opts(DetailPosition::Bottom));
        assert_eq!(l.queue.height, 21);
        assert_eq!(l.detail.height, 18);
    }

    #[test]
    fn stacked_with_a_sources_pane_sits_right_of_it() {
        let o = Options {
            sources: SourcesLayout::Left,
            position: DetailPosition::Top,
            ..Options::default()
        };
        let l = dashboard(body_of(160, 40), o);
        let s = l.sources.unwrap();
        assert_eq!((s.x, s.width), (0, SOURCES_WIDTH));
        assert_eq!(l.queue.x, SOURCES_WIDTH);
        assert_eq!(l.detail.x, SOURCES_WIDTH);
        assert!(l.detail.y < l.queue.y);
    }

    #[test]
    fn stacked_with_the_strip_keeps_the_strip_on_top() {
        let o = Options {
            sources: SourcesLayout::Top,
            position: DetailPosition::Bottom,
            ..Options::default()
        };
        let l = dashboard(body_of(100, 30), o);
        assert_eq!(l.strip.unwrap().height, 1);
        assert_eq!(l.queue.y, 2);
    }

    #[test]
    fn closed_detail_gives_the_queue_everything_in_every_position() {
        for p in [
            DetailPosition::Auto,
            DetailPosition::Right,
            DetailPosition::Left,
            DetailPosition::Top,
            DetailPosition::Bottom,
        ] {
            let o = Options {
                position: p,
                detail: DetailMode::Closed,
                ..Options::default()
            };
            let l = dashboard(body_of(100, 30), o);
            assert_eq!(l.queue, Rect::new(0, 2, 100, 29));
            assert_eq!(l.detail.width, 0);
        }
    }

    fn walk(start: DetailPosition, width: u16, steps: usize) -> Vec<DetailPosition> {
        let mut o = Options {
            position: start,
            ..Options::default()
        };
        (0..steps)
            .map(|_| {
                o.position = o.next_position(width);
                o.position
            })
            .collect()
    }

    #[test]
    fn narrow_cycle_skips_bottom_after_auto() {
        use DetailPosition::*;
        for width in [60, 100, 109] {
            assert_eq!(
                walk(Auto, width, 5),
                [Left, Top, Right, Auto, Left],
                "width {width}"
            );
            assert_eq!(walk(Bottom, width, 1), [Left]);
            assert_eq!(walk(Left, width, 1), [Top]);
            assert_eq!(walk(Top, width, 1), [Right]);
            assert_eq!(walk(Right, width, 1), [Auto]);
        }
    }

    #[test]
    fn wide_cycle_skips_right_before_auto() {
        use DetailPosition::*;
        for width in [110, 130, 160, 200] {
            assert_eq!(
                walk(Auto, width, 5),
                [Bottom, Left, Top, Auto, Bottom],
                "width {width}"
            );
            assert_eq!(walk(Right, width, 1), [Bottom], "remembered right");
        }
    }

    #[test]
    fn every_start_reaches_every_visible_arrangement_and_never_repeats_the_screen() {
        use DetailPosition::*;
        for width in [60, 100, 109, 110, 129, 130, 160, 220] {
            for start in [Auto, Right, Left, Top, Bottom] {
                let mut o = Options {
                    position: start,
                    ..Options::default()
                };
                let mut seen = std::collections::HashSet::new();
                for _ in 0..5 {
                    let before = o.place(width);
                    o.position = o.next_position(width);
                    assert_ne!(o.place(width), before, "{start:?} at {width}");
                    seen.insert(o.place(width).describe());
                }
                assert_eq!(seen.len(), 4, "{start:?} at {width}");
            }
        }
    }

    #[test]
    fn the_places_are_described_by_what_is_on_screen() {
        assert_eq!(Place::Bottom.describe(), "list on top, details below");
        assert_eq!(Place::Right.describe(), "list left, details right");
    }

    #[test]
    fn sizes_parse_columns_and_percentages_and_reject_the_rest() {
        assert_eq!(Size::parse("52"), Ok(Size::Cells(52)));
        assert_eq!(Size::parse(" 60% "), Ok(Size::Percent(60)));
        for bad in ["0", "-3", "wide", "5%", "95%", "", "%"] {
            assert!(Size::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(Size::Percent(50).resolve(101), 50);
        assert_eq!(Size::Percent(60).to_string(), "60%");
    }

    fn with_split(split: Split, position: DetailPosition) -> Options {
        Options {
            sources: SourcesLayout::Left,
            position,
            split,
            ..Options::default()
        }
    }

    #[test]
    fn overrides_set_the_width_and_clamp_to_the_minimums() {
        let b = body((160, 40));
        let width = |size| {
            let split = Split {
                queue_width: Some(size),
                ..Split::default()
            };
            dashboard(b, with_split(split, DetailPosition::Right))
                .queue
                .width
        };
        assert_eq!(width(Size::Cells(70)), 70);
        assert_eq!(width(Size::Cells(5)), QUEUE_COLUMNS_MIN);
        assert_eq!(width(Size::Percent(50)), 67);
        let o = with_split(
            Split {
                queue_width: Some(Size::Cells(500)),
                ..Split::default()
            },
            DetailPosition::Right,
        );
        assert_eq!(dashboard(b, o).detail.width, DETAIL_COLUMNS_MIN);
    }

    #[test]
    fn stacked_overrides_set_the_height_with_a_six_row_floor_each() {
        let b = body((100, 30));
        let height = |cells| {
            let split = Split {
                queue_height: Some(Size::Cells(cells)),
                ..Split::default()
            };
            dashboard(b, with_split(split, DetailPosition::Bottom))
        };
        assert_eq!(height(12).queue.height, 12);
        assert_eq!(height(1).queue.height, QUEUE_ROWS_DRAG_MIN);
        assert_eq!(height(99).detail.height, DETAIL_ROWS_MIN);
    }

    #[test]
    fn a_stored_size_clamps_again_when_the_terminal_shrinks() {
        let split = Split {
            queue_width: Some(Size::Cells(80)),
            sources_width: Some(Size::Cells(40)),
            ..Split::default()
        };
        let o = with_split(split, DetailPosition::Right);
        let wide = dashboard(body((200, 40)), o);
        assert_eq!((wide.queue.width, wide.sources.unwrap().width), (80, 40));
        let narrow = dashboard(body((130, 40)), o);
        assert!(narrow.detail.width >= DETAIL_COLUMNS_MIN);
        assert!(narrow.queue.width >= QUEUE_COLUMNS_MIN);
        let left = narrow.sources.map_or(0, |s| s.width);
        assert_eq!(left + narrow.queue.width + narrow.detail.width, 130);
    }

    #[test]
    fn seams_follow_the_arrangement() {
        let side_o = with_split(Split::default(), DetailPosition::Right);
        let side = seams(body((160, 40)), side_o);
        assert_eq!(side.len(), 2);
        let q = side.iter().find(|s| s.kind == SeamKind::Queue).unwrap();
        assert!(q.vertical && q.boundary == 26 + 48);
        let at = |c, r| seam_at(body((160, 40)), side_o, c, r);
        assert!(at(73, 10).is_some() && at(74, 10).is_some());
        assert!(at(75, 10).is_none() && at(73, 1).is_none());

        let stacked = with_split(Split::default(), DetailPosition::Bottom);
        let l = dashboard(body((100, 30)), stacked);
        let row = l.queue.bottom();
        assert!(seam_at(body((100, 30)), stacked, 50, row).is_some());
        assert!(seam_at(body((100, 30)), stacked, 50, row - 1).is_some());
        assert!(seam_at(body((100, 30)), stacked, 50, row + 1).is_none());

        let closed = Options {
            detail: DetailMode::Closed,
            ..stacked
        };
        assert_eq!(seams(body((100, 30)), closed).len(), 1);
        let top = Options {
            sources: SourcesLayout::Top,
            detail: DetailMode::Closed,
            ..stacked
        };
        assert!(seams(body((160, 40)), top).is_empty());
    }

    #[test]
    fn nudging_and_resetting_work_per_arrangement() {
        let b = body((160, 40));
        let mut o = with_split(Split::default(), DetailPosition::Right);
        assert_eq!(nudge(&mut o, b, SeamKind::Queue, 2), 50);
        assert_eq!(nudge(&mut o, b, SeamKind::Queue, -100), QUEUE_COLUMNS_MIN);
        assert_eq!(o.split.queue_height, None);
        reset(&mut o, b, SeamKind::Queue);
        assert_eq!(o.split.queue_width, None);
        assert_eq!(describe(o, b, SeamKind::Queue), "queue 48 columns");
        o.position = DetailPosition::Bottom;
        assert!(describe(o, b, SeamKind::Queue).ends_with("rows"));
    }
}
