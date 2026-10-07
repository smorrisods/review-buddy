//! A ratatui widget that draws a [`Screen`].
//!
//! The child's colours pass through untouched: indexed colours stay indexed and 24-bit colours stay
//! 24-bit. Only the cells the child left on the default colours take the host's style, which is how
//! the pane picks up the active theme.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color as Tui, Modifier, Style};
use ratatui::widgets::Widget;

use crate::screen::{Attrs, Color, Cursor, CursorShape, Screen};

pub struct TerminalWidget<'a> {
    screen: &'a Screen,
    base: Style,
    show_cursor: bool,
}

impl<'a> TerminalWidget<'a> {
    pub fn new(screen: &'a Screen) -> Self {
        Self {
            screen,
            base: Style::default(),
            show_cursor: true,
        }
    }

    /// The style for cells that use the child's default colours: the host theme's text and
    /// background.
    pub fn base_style(mut self, base: Style) -> Self {
        self.base = base;
        self
    }

    /// Draws the cursor into the grid. Turn it off when the pane doesn't have focus and the host
    /// wants an unmarked grid.
    pub fn show_cursor(mut self, show: bool) -> Self {
        self.show_cursor = show;
        self
    }
}

fn convert(c: Color) -> Option<Tui> {
    match c {
        Color::Default => None,
        Color::Indexed(i) => Some(Tui::Indexed(i)),
        Color::Rgb(r, g, b) => Some(Tui::Rgb(r, g, b)),
    }
}

fn cursor_modifier(cursor: &Cursor) -> Modifier {
    match cursor.shape {
        CursorShape::Underline | CursorShape::Beam => Modifier::UNDERLINED,
        CursorShape::Block | CursorShape::HollowBlock => Modifier::REVERSED,
    }
}

impl Widget for TerminalWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let s = self.screen;
        for row in 0..area.height.min(s.rows) {
            for col in 0..area.width.min(s.cols) {
                let Some(cell) = s.cell(col, row) else {
                    continue;
                };
                let target = &mut buf[(area.x + col, area.y + row)];
                target.reset();
                target.set_style(self.base);
                if cell.attrs.contains(Attrs::SPACER) {
                    // The second half of a double-width character is drawn by the first half.
                    target.set_symbol("");
                    continue;
                }
                let mut style = Style::default();
                if let Some(fg) = convert(cell.fg) {
                    style = style.fg(fg);
                }
                if let Some(bg) = convert(cell.bg) {
                    style = style.bg(bg);
                }
                let mut m = Modifier::empty();
                for (attr, modifier) in [
                    (Attrs::BOLD, Modifier::BOLD),
                    (Attrs::DIM, Modifier::DIM),
                    (Attrs::ITALIC, Modifier::ITALIC),
                    (Attrs::UNDERLINE, Modifier::UNDERLINED),
                    (Attrs::INVERSE, Modifier::REVERSED),
                    (Attrs::HIDDEN, Modifier::HIDDEN),
                    (Attrs::STRIKEOUT, Modifier::CROSSED_OUT),
                ] {
                    if cell.attrs.contains(attr) {
                        m |= modifier;
                    }
                }
                target.set_style(style.add_modifier(m));
                if cell.marks.is_empty() {
                    let mut buf4 = [0u8; 4];
                    target.set_symbol(cell.ch.encode_utf8(&mut buf4));
                } else {
                    target.set_symbol(&cell.text());
                }
            }
        }
        if let (true, Some(cursor)) = (self.show_cursor, s.cursor) {
            if cursor.col < area.width && cursor.row < area.height {
                let target = &mut buf[(area.x + cursor.col, area.y + cursor.row)];
                target.modifier.toggle(cursor_modifier(&cursor));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::Emulator;

    fn draw(bytes: &[u8], cols: u16, rows: u16, cursor: bool) -> Buffer {
        let mut e = Emulator::new(cols, rows, 10);
        e.feed(bytes);
        let screen = e.screen();
        let area = Rect::new(2, 1, cols, rows);
        let mut buf = Buffer::empty(Rect::new(0, 0, cols + 4, rows + 2));
        TerminalWidget::new(&screen)
            .show_cursor(cursor)
            .render(area, &mut buf);
        buf
    }

    #[test]
    fn text_colours_and_attributes_are_drawn() {
        let buf = draw(b"\x1b[1;38;5;196mhi\x1b[0m \x1b[48;2;1;2;3mx", 10, 2, false);
        let h = &buf[(2, 1)];
        assert_eq!(h.symbol(), "h");
        assert_eq!(h.fg, Tui::Indexed(196));
        assert!(h.modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(5, 1)].bg, Tui::Rgb(1, 2, 3));
        assert_eq!(buf[(3, 1)].symbol(), "i");
    }

    #[test]
    fn wide_characters_and_marks_are_drawn_once() {
        let buf = draw("世e\u{301}".as_bytes(), 10, 1, false);
        assert_eq!(buf[(2, 1)].symbol(), "世");
        assert_eq!(buf[(3, 1)].symbol(), "");
        assert_eq!(buf[(4, 1)].symbol(), "e\u{301}");
    }

    #[test]
    fn the_cursor_is_marked_in_the_grid() {
        let on = draw(b"ab", 6, 1, true);
        assert!(on[(4, 1)].modifier.contains(Modifier::REVERSED));
        let off = draw(b"ab", 6, 1, false);
        assert!(!off[(4, 1)].modifier.contains(Modifier::REVERSED));
        let under = draw(b"\x1b[4 qab", 6, 1, true);
        assert!(under[(4, 1)].modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn default_colours_take_the_host_style() {
        let mut e = Emulator::new(4, 1, 10);
        e.feed(b"a\x1b[31mb");
        let screen = e.screen();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        TerminalWidget::new(&screen)
            .base_style(Style::default().fg(Tui::Rgb(9, 9, 9)).bg(Tui::Rgb(1, 1, 1)))
            .show_cursor(false)
            .render(Rect::new(0, 0, 4, 1), &mut buf);
        assert_eq!(buf[(0, 0)].fg, Tui::Rgb(9, 9, 9));
        assert_eq!(buf[(0, 0)].bg, Tui::Rgb(1, 1, 1));
        assert_eq!(
            buf[(1, 0)].fg,
            Tui::Indexed(1),
            "the child's colour passes through"
        );
        assert_eq!(buf[(1, 0)].bg, Tui::Rgb(1, 1, 1));
    }

    #[test]
    fn a_smaller_area_clips() {
        let mut e = Emulator::new(10, 3, 10);
        e.feed(b"abcdefghij");
        let screen = e.screen();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        TerminalWidget::new(&screen).render(Rect::new(0, 0, 4, 1), &mut buf);
        assert_eq!(buf[(3, 0)].symbol(), "d");
    }
}
