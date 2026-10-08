use ratatui::layout::Rect;

use super::textmap::TextMap;
use crate::app::{Action, Pane};

/// Clickable rectangles registered while drawing; later entries sit on top.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HitMap {
    entries: Vec<(Rect, Action)>,
    panes: Vec<(Rect, Pane)>,
    /// The text of the selectable regions, as drawn.
    pub texts: TextMap,
}

impl HitMap {
    pub fn push(&mut self, rect: Rect, action: Action) {
        if rect.width > 0 && rect.height > 0 {
            self.entries.push((rect, action));
        }
    }

    pub fn at(&self, column: u16, row: u16) -> Option<&Action> {
        self.entries
            .iter()
            .rev()
            .find(|(rect, _)| contains(*rect, column, row))
            .map(|(_, action)| action)
    }

    /// Records where each dashboard pane sits, for the wheel and for clicks on empty space.
    pub fn set_panes(&mut self, panes: Vec<(Rect, Pane)>) {
        self.panes = panes;
    }

    pub fn pane_at(&self, column: u16, row: u16) -> Option<Pane> {
        self.panes
            .iter()
            .find(|(rect, _)| contains(*rect, column, row))
            .map(|(_, pane)| *pane)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x && column < rect.right() && row >= rect.y && row < rect.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_targets_and_misses_outside() {
        let mut map = HitMap::default();
        map.push(Rect::new(2, 1, 4, 2), Action::Quit);
        assert_eq!(map.at(2, 1), Some(&Action::Quit));
        assert_eq!(map.at(5, 2), Some(&Action::Quit));
        assert_eq!(map.at(6, 2), None);
        assert_eq!(map.at(5, 3), None);
        assert_eq!(map.at(1, 1), None);
    }

    #[test]
    fn topmost_entry_wins() {
        let mut map = HitMap::default();
        map.push(Rect::new(0, 0, 10, 10), Action::Quit);
        map.push(Rect::new(2, 2, 3, 3), Action::CycleTheme);
        assert_eq!(map.at(3, 3), Some(&Action::CycleTheme));
        assert_eq!(map.at(8, 8), Some(&Action::Quit));
    }

    #[test]
    fn empty_rects_are_ignored() {
        let mut map = HitMap::default();
        map.push(Rect::new(0, 0, 0, 5), Action::Quit);
        map.push(Rect::new(0, 0, 5, 0), Action::Quit);
        assert!(map.is_empty());
        assert_eq!(map.at(0, 0), None);
    }

    #[test]
    fn handles_edges_without_overflow() {
        let mut map = HitMap::default();
        map.push(Rect::new(u16::MAX - 2, u16::MAX - 2, 2, 2), Action::Quit);
        assert_eq!(map.at(u16::MAX - 1, u16::MAX - 1), Some(&Action::Quit));
        assert_eq!(map.len(), 1);
    }
}
