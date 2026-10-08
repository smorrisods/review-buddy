//! A framework-neutral snapshot of the emulator's visible grid.

/// A colour as the child asked for it. Defaults and indexed colours stay symbolic so the widget
/// can leave them to the host terminal; only 24-bit colours carry values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Text attributes, as a small bit set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Attrs(u16);

impl Attrs {
    pub const BOLD: Self = Self(1);
    pub const DIM: Self = Self(1 << 1);
    pub const ITALIC: Self = Self(1 << 2);
    pub const UNDERLINE: Self = Self(1 << 3);
    pub const INVERSE: Self = Self(1 << 4);
    pub const HIDDEN: Self = Self(1 << 5);
    pub const STRIKEOUT: Self = Self(1 << 6);
    /// The first half of a double-width character.
    pub const WIDE: Self = Self(1 << 7);
    /// The empty second half of a double-width character.
    pub const SPACER: Self = Self(1 << 8);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub const fn bits(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    /// Combining characters that follow `ch`.
    pub marks: Vec<char>,
    pub fg: Color,
    pub bg: Color,
    pub attrs: Attrs,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            marks: Vec::new(),
            fg: Color::Default,
            bg: Color::Default,
            attrs: Attrs::empty(),
        }
    }
}

impl Cell {
    /// The text this cell draws, `ch` followed by its combining marks.
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(1 + self.marks.len());
        out.push(self.ch);
        out.extend(self.marks.iter());
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Beam,
    HollowBlock,
}

/// Where the cursor is on the visible grid, when it is shown and on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub shape: CursorShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    /// Row-major, `cols * rows` cells.
    pub cells: Vec<Cell>,
    /// `None` when the child hid it, or when the viewport is scrolled back into history.
    pub cursor: Option<Cursor>,
    /// How many lines the viewport is scrolled back from the live screen.
    pub scrolled: usize,
    /// How many lines of history exist above the live screen.
    pub history: usize,
}

impl Screen {
    pub fn cell(&self, col: u16, row: u16) -> Option<&Cell> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        self.cells
            .get(usize::from(row) * usize::from(self.cols) + usize::from(col))
    }

    /// The text of one row with trailing blanks trimmed. Spacer cells add nothing.
    pub fn row_text(&self, row: u16) -> String {
        let mut out = String::new();
        for col in 0..self.cols {
            if let Some(cell) = self.cell(col, row) {
                if !cell.attrs.contains(Attrs::SPACER) {
                    out.push(cell.ch);
                    out.extend(cell.marks.iter());
                }
            }
        }
        out.trim_end().to_string()
    }

    /// Every row as text, for tests and plain-text fallbacks.
    pub fn text(&self) -> String {
        (0..self.rows)
            .map(|r| self.row_text(r))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
