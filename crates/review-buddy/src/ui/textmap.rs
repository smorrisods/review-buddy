//! The text the last draw put on screen, kept as logical rows so a selection copies what the
//! words say rather than what the buffer holds: no gutters, borders, padding or wrap artefacts.
//!
//! A draw records one [`TextRegion`] per block of selectable text (the diff's code column, one
//! comment or thread block, the description, the Files path rows). A selection stays inside the
//! region it started in. Rows are numbered by `ord`, which does not change when the view scrolls.

use ratatui::layout::Rect;

pub use super::text::Join;

/// Which block of text a row belongs to. A selection never leaves its region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RegionKey {
    /// The code column of the diff.
    Diff,
    /// The text of one comment or thread block under a diff line.
    Block(u32),
    /// The paths in the Files pane.
    Files,
    /// The description in the Detail pane.
    Description,
    /// The body of the latest comment in the Detail pane.
    LatestComment,
    /// One comment on the Detail pane's Conversation tab, numbered across all its threads.
    Comment(u32),
}

/// One screen row of text. `text` is what the row shows, without any decoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRow {
    /// The row's place in its region, stable while the view scrolls.
    pub ord: usize,
    pub x: u16,
    /// The screen row, or [`TextRow::OFF_SCREEN`] for a row recorded just outside the view so
    /// a selection can grow into it.
    pub y: u16,
    /// Cells from `x` where a press lands on this row.
    pub span: u16,
    pub text: String,
    pub join: Join,
    /// What to copy when any of the row is selected, when that isn't the row's text (a path
    /// that was shortened to fit).
    pub copy: Option<String>,
}

impl TextRow {
    pub const OFF_SCREEN: u16 = u16::MAX;

    pub fn new(ord: usize, (x, y): (u16, u16), span: u16, text: impl Into<String>) -> Self {
        Self {
            ord,
            x,
            y,
            span,
            text: text.into(),
            join: Join::Break,
            copy: None,
        }
    }

    pub fn joined(mut self, join: Join) -> Self {
        self.join = join;
        self
    }

    pub fn copying(mut self, copy: String) -> Self {
        self.copy = Some(copy);
        self
    }

    pub fn on_screen(&self) -> bool {
        self.y != Self::OFF_SCREEN
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRegion {
    pub key: RegionKey,
    /// The area the region's text sits in.
    pub rect: Rect,
    /// In ascending `ord` order.
    pub rows: Vec<TextRow>,
}

impl TextRegion {
    pub fn new(key: RegionKey, rect: Rect) -> Self {
        Self {
            key,
            rect,
            rows: Vec::new(),
        }
    }

    /// The row a pointer at `(x, y)` is over, if any.
    pub fn row_at(&self, x: u16, y: u16) -> Option<&TextRow> {
        self.rows
            .iter()
            .find(|r| r.on_screen() && r.y == y && x >= r.x && x < r.x.saturating_add(r.span))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextMap {
    regions: Vec<TextRegion>,
}

impl TextMap {
    pub fn push(&mut self, region: TextRegion) {
        if !region.rows.is_empty() {
            self.regions.push(region);
        }
    }

    pub fn region(&self, key: RegionKey) -> Option<&TextRegion> {
        self.regions.iter().find(|r| r.key == key)
    }

    pub fn regions(&self) -> &[TextRegion] {
        &self.regions
    }

    /// The region and row under a pointer at `(x, y)`.
    pub fn at(&self, x: u16, y: u16) -> Option<(&TextRegion, &TextRow)> {
        self.regions
            .iter()
            .find_map(|region| region.row_at(x, y).map(|row| (region, row)))
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }
}

/// Highlights the selected cells of the frame just drawn. It runs before the overlays are drawn,
/// so a toast, the help or a composer covers it as it covers everything behind.
pub fn paint(frame: &mut ratatui::Frame, app: &crate::app::App, texts: &TextMap) {
    let Some(sel) = app
        .selection
        .as_ref()
        .filter(|s| !s.is_empty() || s.keyboard)
    else {
        return;
    };
    let Some(region) = texts.region(sel.key) else {
        return;
    };
    let look = super::style::text_selection(&app.palette);
    let area = frame.area();
    for row in region.rows.iter().filter(|r| r.on_screen()) {
        let Some((from, to)) = sel.highlight(row) else {
            continue;
        };
        for cell in from..to.min(usize::from(row.span)) {
            let x = row.x.saturating_add(cell as u16);
            if x < area.right() && row.y < area.bottom() {
                super::style::paint_selected(&mut frame.buffer_mut()[(x, row.y)], look);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region() -> TextRegion {
        let mut r = TextRegion::new(RegionKey::Description, Rect::new(2, 3, 20, 2));
        r.rows.push(TextRow::new(0, (2, 3), 20, "hello"));
        r.rows.push(TextRow::new(1, (2, 4), 5, "world"));
        r.rows
            .push(TextRow::new(2, (2, TextRow::OFF_SCREEN), 5, "later"));
        r
    }

    #[test]
    fn finds_the_row_under_the_pointer_within_its_span() {
        let r = region();
        assert_eq!(r.row_at(2, 3).map(|r| r.ord), Some(0));
        assert_eq!(r.row_at(21, 3).map(|r| r.ord), Some(0));
        assert_eq!(r.row_at(22, 3), None);
        assert_eq!(r.row_at(6, 4).map(|r| r.ord), Some(1));
        assert_eq!(r.row_at(7, 4), None);
        assert_eq!(r.row_at(1, 3), None);
    }

    #[test]
    fn rows_outside_the_view_are_never_hit() {
        let r = region();
        assert_eq!(r.row_at(2, TextRow::OFF_SCREEN), None);
    }

    #[test]
    fn empty_regions_are_not_kept() {
        let mut map = TextMap::default();
        map.push(TextRegion::new(RegionKey::Diff, Rect::new(0, 0, 1, 1)));
        assert!(map.is_empty());
        map.push(region());
        assert!(map.region(RegionKey::Description).is_some());
        assert!(map.at(2, 3).is_some());
        assert!(map.at(0, 0).is_none());
    }
}
