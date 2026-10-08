//! The line range a drag or a shift-click selects in the diff. Pure row arithmetic: both ends
//! are rows that rest on a diff line, and the range includes everything between them.

use super::diffview::ScreenMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowRange {
    /// Where the drag began, or where the range was last anchored.
    pub anchor: usize,
    /// The end the pointer is on. It may sit above the anchor.
    pub head: usize,
}

impl RowRange {
    /// `None` when both ends meet: a single line is just the cursor.
    pub fn between(anchor: usize, head: usize) -> Option<Self> {
        (anchor != head).then_some(Self { anchor, head })
    }

    /// The first and last row, top to bottom.
    pub fn bounds(self) -> (usize, usize) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub fn contains(self, row: usize) -> bool {
        let (top, bottom) = self.bounds();
        (top..=bottom).contains(&row)
    }
}

/// The logical row under a pointer at screen row `y` in a code area starting at `top` and
/// showing screen rows from `scroll`, clamped to the rows that exist. `None` when there are none.
pub fn row_at(y: u16, top: u16, height: u16, scroll: usize, map: &ScreenMap) -> Option<usize> {
    let last = map.total().checked_sub(1)?;
    let offset = usize::from(y.saturating_sub(top)).min(usize::from(height.saturating_sub(1)));
    map.locate((scroll + offset).min(last)).map(|(row, _)| row)
}

/// Which way a drag past the edge of the code area should scroll: -1 up, 1 down, 0 neither.
pub fn edge_scroll(y: u16, top: u16, height: u16) -> isize {
    if y < top {
        -1
    } else if y >= top.saturating_add(height) {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_of_one_line_is_not_a_range() {
        assert_eq!(RowRange::between(4, 4), None);
        assert_eq!(
            RowRange::between(4, 7),
            Some(RowRange { anchor: 4, head: 7 })
        );
    }

    #[test]
    fn bounds_order_the_ends_whichever_way_the_drag_went() {
        let up = RowRange::between(9, 5).unwrap();
        assert_eq!(up.bounds(), (5, 9));
        assert!(up.contains(5) && up.contains(7) && up.contains(9));
        assert!(!up.contains(4) && !up.contains(10));
    }

    #[test]
    fn row_at_maps_the_pointer_through_the_scroll_and_clamps() {
        let map = ScreenMap::identity(100);
        assert_eq!(row_at(5, 2, 10, 30, &map), Some(33));
        assert_eq!(row_at(0, 2, 10, 30, &map), Some(30), "above the area");
        assert_eq!(row_at(40, 2, 10, 30, &map), Some(39), "below the area");
        assert_eq!(row_at(11, 2, 10, 95, &map), Some(99), "past the last row");
        assert_eq!(row_at(5, 2, 10, 0, &ScreenMap::identity(0)), None);
    }

    #[test]
    fn edge_scroll_only_triggers_outside_the_area() {
        assert_eq!(edge_scroll(1, 2, 10), -1);
        assert_eq!(edge_scroll(2, 2, 10), 0);
        assert_eq!(edge_scroll(11, 2, 10), 0);
        assert_eq!(edge_scroll(12, 2, 10), 1);
    }
}
