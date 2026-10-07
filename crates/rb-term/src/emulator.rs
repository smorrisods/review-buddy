//! The emulator boundary: the only module that names `alacritty_terminal`.
//!
//! Bytes go in with [`Emulator::feed`]. What comes out is the bytes the child expects back (answers
//! to its queries) and a short list of [`Event`]s the host cares about. The grid is read as a
//! [`Screen`] snapshot, so no `alacritty_terminal` type crosses this boundary.

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event as Raw, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Osc52, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as RawColor, CursorShape as RawShape, NamedColor, Processor, Rgb,
};

use crate::screen::{Attrs, Cell, Color, Cursor, CursorShape, Screen};

const DEFAULT_CELL_PX: (u16, u16) = (8, 16);

/// What the host should know happened while feeding bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The child set the window title (OSC 0 or 2).
    Title(String),
    /// The child asked for the default title back.
    ResetTitle,
    /// The child rang the bell.
    Bell,
    /// The child asked to put text on the clipboard (OSC 52). Reads are never answered.
    Clipboard(String),
}

/// The result of [`Emulator::feed`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    /// Bytes to write back to the child: device attributes, colour and size reports, the kitty
    /// keyboard query answer.
    pub reply: Vec<u8>,
    pub events: Vec<Event>,
}

/// The colours the child sees when it asks. Output is never recoloured by these: they only answer
/// queries such as OSC 10 and OSC 11, so a child can choose a palette that suits the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorScheme {
    pub foreground: (u8, u8, u8),
    pub background: (u8, u8, u8),
    pub ansi: [(u8, u8, u8); 16],
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            foreground: (0xd8, 0xd8, 0xd8),
            background: (0x10, 0x10, 0x10),
            ansi: [
                (0x00, 0x00, 0x00),
                (0xcd, 0x00, 0x00),
                (0x00, 0xcd, 0x00),
                (0xcd, 0xcd, 0x00),
                (0x00, 0x00, 0xee),
                (0xcd, 0x00, 0xcd),
                (0x00, 0xcd, 0xcd),
                (0xe5, 0xe5, 0xe5),
                (0x7f, 0x7f, 0x7f),
                (0xff, 0x00, 0x00),
                (0x00, 0xff, 0x00),
                (0xff, 0xff, 0x00),
                (0x5c, 0x5c, 0xff),
                (0xff, 0x00, 0xff),
                (0x00, 0xff, 0xff),
                (0xff, 0xff, 0xff),
            ],
        }
    }
}

impl ColorScheme {
    /// The colour behind an OSC query index: 0 to 255 for the palette, 256 for the foreground, 257
    /// for the background and 258 for the cursor.
    pub fn lookup(&self, index: usize) -> (u8, u8, u8) {
        match index {
            0..=15 => self.ansi[index],
            16..=231 => {
                let i = index - 16;
                let level = |n: usize| if n == 0 { 0 } else { (55 + 40 * n) as u8 };
                (level(i / 36), level(i / 6 % 6), level(i % 6))
            }
            232..=255 => {
                let v = (8 + 10 * (index - 232)) as u8;
                (v, v, v)
            }
            257 => self.background,
            _ => self.foreground,
        }
    }
}

/// How much of the mouse the child wants to hear about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseMode {
    #[default]
    Off,
    /// Presses and releases (mode 1000).
    Click,
    /// Plus movement while a button is down (mode 1002).
    Drag,
    /// Plus all movement (mode 1003).
    Motion,
}

/// The kitty keyboard protocol flags the child has enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KittyFlags(pub u8);

impl KittyFlags {
    pub const DISAMBIGUATE: u8 = 1;
    pub const EVENT_TYPES: u8 = 2;
    pub const ALTERNATE_KEYS: u8 = 4;
    pub const ALL_KEYS: u8 = 8;
    pub const ASSOCIATED_TEXT: u8 = 16;

    pub fn has(self, flag: u8) -> bool {
        self.0 & flag == flag
    }

    pub fn any(self) -> bool {
        self.0 != 0
    }
}

/// The input modes the child has switched on. The key and mouse encoders read these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modes {
    pub bracketed_paste: bool,
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub mouse: MouseMode,
    pub sgr_mouse: bool,
    pub utf8_mouse: bool,
    pub focus_reporting: bool,
    pub alt_screen: bool,
    /// Wheel turns on the alternate screen become arrow keys when the child isn't reporting the mouse.
    pub alternate_scroll: bool,
    pub kitty: KittyFlags,
}

impl Modes {
    fn from_raw(m: TermMode) -> Self {
        let mut kitty = 0;
        for (flag, bit) in [
            (TermMode::DISAMBIGUATE_ESC_CODES, KittyFlags::DISAMBIGUATE),
            (TermMode::REPORT_EVENT_TYPES, KittyFlags::EVENT_TYPES),
            (TermMode::REPORT_ALTERNATE_KEYS, KittyFlags::ALTERNATE_KEYS),
            (TermMode::REPORT_ALL_KEYS_AS_ESC, KittyFlags::ALL_KEYS),
            (
                TermMode::REPORT_ASSOCIATED_TEXT,
                KittyFlags::ASSOCIATED_TEXT,
            ),
        ] {
            if m.contains(flag) {
                kitty |= bit;
            }
        }
        let mouse = if m.contains(TermMode::MOUSE_MOTION) {
            MouseMode::Motion
        } else if m.contains(TermMode::MOUSE_DRAG) {
            MouseMode::Drag
        } else if m.contains(TermMode::MOUSE_REPORT_CLICK) {
            MouseMode::Click
        } else {
            MouseMode::Off
        };
        Self {
            bracketed_paste: m.contains(TermMode::BRACKETED_PASTE),
            app_cursor: m.contains(TermMode::APP_CURSOR),
            app_keypad: m.contains(TermMode::APP_KEYPAD),
            mouse,
            sgr_mouse: m.contains(TermMode::SGR_MOUSE),
            utf8_mouse: m.contains(TermMode::UTF8_MOUSE),
            focus_reporting: m.contains(TermMode::FOCUS_IN_OUT),
            alt_screen: m.contains(TermMode::ALT_SCREEN),
            alternate_scroll: m.contains(TermMode::ALTERNATE_SCROLL),
            kitty: KittyFlags(kitty),
        }
    }
}

#[derive(Clone, Default)]
struct Listener(Arc<Mutex<Vec<Raw>>>);

impl EventListener for Listener {
    fn send_event(&self, event: Raw) {
        if let Ok(mut queue) = self.0.lock() {
            queue.push(event);
        }
    }
}

struct Size {
    cols: usize,
    rows: usize,
    history: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows + self.history
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

pub struct Emulator {
    term: Term<Listener>,
    parser: Processor,
    queue: Listener,
    scheme: ColorScheme,
    cell_px: (u16, u16),
    title: Option<String>,
    history: usize,
}

impl std::fmt::Debug for Emulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Emulator")
            .field("cols", &self.term.columns())
            .field("rows", &self.term.screen_lines())
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl Emulator {
    /// A blank screen `cols` by `rows`, keeping `scrollback` lines of history.
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let queue = Listener::default();
        let config = Config {
            scrolling_history: scrollback,
            kitty_keyboard: true,
            osc52: Osc52::OnlyCopy,
            ..Config::default()
        };
        let size = Size {
            cols: usize::from(cols.max(2)),
            rows: usize::from(rows.max(1)),
            history: 0,
        };
        Self {
            term: Term::new(config, &size, queue.clone()),
            parser: Processor::new(),
            queue,
            scheme: ColorScheme::default(),
            cell_px: DEFAULT_CELL_PX,
            title: None,
            history: scrollback,
        }
    }

    pub fn set_scheme(&mut self, scheme: ColorScheme) {
        self.scheme = scheme;
    }

    /// The size of one cell in pixels, for the child's size reports. Terminals that can't tell
    /// keep the 8 by 16 default.
    pub fn set_cell_pixels(&mut self, width: u16, height: u16) {
        self.cell_px = (width.max(1), height.max(1));
    }

    pub fn size(&self) -> (u16, u16) {
        (self.term.columns() as u16, self.term.screen_lines() as u16)
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn modes(&self) -> Modes {
        Modes::from_raw(*self.term.mode())
    }

    /// Feeds the child's output through the parser.
    pub fn feed(&mut self, bytes: &[u8]) -> Output {
        self.parser.advance(&mut self.term, bytes);
        self.drain()
    }

    fn drain(&mut self) -> Output {
        let raw: Vec<Raw> = match self.queue.0.lock() {
            Ok(mut queue) => std::mem::take(&mut *queue),
            Err(_) => Vec::new(),
        };
        let mut out = Output::default();
        for event in raw {
            match event {
                Raw::PtyWrite(text) => out.reply.extend_from_slice(text.as_bytes()),
                Raw::ColorRequest(index, format) => {
                    let (r, g, b) = self.scheme.lookup(index);
                    out.reply
                        .extend_from_slice(format(Rgb { r, g, b }).as_bytes());
                }
                Raw::TextAreaSizeRequest(format) => {
                    let (cols, rows) = self.size();
                    let window = WindowSize {
                        num_lines: rows,
                        num_cols: cols,
                        cell_width: self.cell_px.0,
                        cell_height: self.cell_px.1,
                    };
                    out.reply.extend_from_slice(format(window).as_bytes());
                }
                Raw::Title(title) => {
                    self.title = Some(title.clone());
                    out.events.push(Event::Title(title));
                }
                Raw::ResetTitle => {
                    self.title = None;
                    out.events.push(Event::ResetTitle);
                }
                Raw::ClipboardStore(_, text) => out.events.push(Event::Clipboard(text)),
                Raw::Bell => out.events.push(Event::Bell),
                _ => {}
            }
        }
        out
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let size = Size {
            cols: usize::from(cols.max(2)),
            rows: usize::from(rows.max(1)),
            history: 0,
        };
        self.term.resize(size);
        let _ = self.drain();
    }

    /// Scrolls the viewport into history by `lines` (negative scrolls back towards the live screen).
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    pub fn scroll_to_top(&mut self) {
        self.term.scroll_display(Scroll::Top);
    }

    /// The address of the OSC 8 hyperlink under a cell, if any.
    pub fn hyperlink_at(&self, col: u16, row: u16) -> Option<String> {
        let grid = self.term.grid();
        if usize::from(col) >= grid.columns() || usize::from(row) >= grid.screen_lines() {
            return None;
        }
        let line = Line(i32::from(row) - grid.display_offset() as i32);
        grid[line][Column(usize::from(col))]
            .hyperlink()
            .map(|link| link.uri().to_string())
    }

    /// A snapshot of the visible grid, scrolled to the viewport.
    pub fn screen(&self) -> Screen {
        let grid = self.term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let offset = grid.display_offset();
        let mut cells = Vec::with_capacity(cols * rows);
        for row in 0..rows {
            let line = Line(row as i32 - offset as i32);
            let source = &grid[line];
            for col in 0..cols {
                let cell = &source[Column(col)];
                let mut attrs = Attrs::empty();
                for (flag, attr) in [
                    (Flags::BOLD, Attrs::BOLD),
                    (Flags::DIM, Attrs::DIM),
                    (Flags::ITALIC, Attrs::ITALIC),
                    (Flags::ALL_UNDERLINES, Attrs::UNDERLINE),
                    (Flags::INVERSE, Attrs::INVERSE),
                    (Flags::HIDDEN, Attrs::HIDDEN),
                    (Flags::STRIKEOUT, Attrs::STRIKEOUT),
                    (Flags::WIDE_CHAR, Attrs::WIDE),
                    (Flags::WIDE_CHAR_SPACER, Attrs::SPACER),
                    (Flags::LEADING_WIDE_CHAR_SPACER, Attrs::SPACER),
                ] {
                    if cell.flags.intersects(flag) {
                        attrs.insert(attr);
                    }
                }
                cells.push(Cell {
                    ch: cell.c,
                    marks: cell.zerowidth().map(<[char]>::to_vec).unwrap_or_default(),
                    fg: colour(cell.fg),
                    bg: colour(cell.bg),
                    attrs,
                });
            }
        }
        let cursor_style = self.term.cursor_style();
        let point = grid.cursor.point;
        let shown = self.term.mode().contains(TermMode::SHOW_CURSOR)
            && offset == 0
            && cursor_style.shape != RawShape::Hidden
            && point.line.0 >= 0
            && (point.line.0 as usize) < rows;
        let cursor = shown.then(|| Cursor {
            col: point.column.0.min(cols.saturating_sub(1)) as u16,
            row: point.line.0 as u16,
            shape: match cursor_style.shape {
                RawShape::Underline => CursorShape::Underline,
                RawShape::Beam => CursorShape::Beam,
                RawShape::HollowBlock => CursorShape::HollowBlock,
                _ => CursorShape::Block,
            },
        });
        Screen {
            cols: cols as u16,
            rows: rows as u16,
            cells,
            cursor,
            scrolled: offset,
            history: self
                .term
                .total_lines()
                .saturating_sub(rows)
                .min(self.history),
        }
    }
}

fn colour(c: RawColor) -> Color {
    match c {
        RawColor::Spec(Rgb { r, g, b }) => Color::Rgb(r, g, b),
        RawColor::Indexed(i) => Color::Indexed(i),
        RawColor::Named(name) => match name {
            NamedColor::Foreground
            | NamedColor::Background
            | NamedColor::Cursor
            | NamedColor::BrightForeground
            | NamedColor::DimForeground => Color::Default,
            other => Color::Indexed(named_index(other)),
        },
    }
}

fn named_index(name: NamedColor) -> u8 {
    use NamedColor::*;
    match name {
        Black | DimBlack => 0,
        Red | DimRed => 1,
        Green | DimGreen => 2,
        Yellow | DimYellow => 3,
        Blue | DimBlue => 4,
        Magenta | DimMagenta => 5,
        Cyan | DimCyan => 6,
        White | DimWhite => 7,
        BrightBlack => 8,
        BrightRed => 9,
        BrightGreen => 10,
        BrightYellow => 11,
        BrightBlue => 12,
        BrightMagenta => 13,
        BrightCyan => 14,
        BrightWhite => 15,
        _ => 7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emu() -> Emulator {
        Emulator::new(20, 5, 100)
    }

    #[test]
    fn text_lands_on_the_grid() {
        let mut e = emu();
        e.feed(b"hello\r\nworld");
        let s = e.screen();
        assert_eq!(s.row_text(0), "hello");
        assert_eq!(s.row_text(1), "world");
        assert_eq!(s.cursor.map(|c| (c.col, c.row)), Some((5, 1)));
    }

    #[test]
    fn colours_and_attributes_come_through() {
        let mut e = emu();
        e.feed(b"\x1b[1;31mR\x1b[0m\x1b[38;2;1;2;3mT\x1b[48;5;200mI\x1b[7mV");
        let s = e.screen();
        let c = |i| s.cell(i, 0).unwrap();
        assert_eq!(c(0).fg, Color::Indexed(1));
        assert!(c(0).attrs.contains(Attrs::BOLD));
        assert_eq!(c(1).fg, Color::Rgb(1, 2, 3));
        assert_eq!(c(2).bg, Color::Indexed(200));
        assert!(c(3).attrs.contains(Attrs::INVERSE));
        assert_eq!(c(1).bg, Color::Default);
    }

    #[test]
    fn wide_characters_take_two_cells() {
        let mut e = emu();
        e.feed("a世b".as_bytes());
        let s = e.screen();
        assert!(s.cell(1, 0).unwrap().attrs.contains(Attrs::WIDE));
        assert!(s.cell(2, 0).unwrap().attrs.contains(Attrs::SPACER));
        assert_eq!(s.cell(3, 0).unwrap().ch, 'b');
        assert_eq!(s.row_text(0), "a世b");
    }

    #[test]
    fn combining_marks_stay_with_their_base() {
        let mut e = emu();
        e.feed("e\u{301}x".as_bytes());
        let s = e.screen();
        assert_eq!(s.cell(0, 0).unwrap().text(), "e\u{301}");
        assert_eq!(s.cell(1, 0).unwrap().ch, 'x');
    }

    #[test]
    fn scrollback_is_viewable_and_snaps_back() {
        let mut e = Emulator::new(10, 3, 50);
        for n in 0..10 {
            e.feed(format!("line{n}\r\n").as_bytes());
        }
        assert_eq!(e.screen().scrolled, 0);
        e.scroll(4);
        let s = e.screen();
        assert_eq!(s.scrolled, 4);
        assert!(s.history >= 4);
        assert!(s.cursor.is_none(), "no cursor while looking at history");
        assert!(s.row_text(0).starts_with("line"));
        e.scroll_to_bottom();
        assert_eq!(e.screen().scrolled, 0);
    }

    #[test]
    fn resize_keeps_the_grid_consistent() {
        let mut e = emu();
        e.feed(b"abc");
        e.resize(30, 8);
        assert_eq!(e.size(), (30, 8));
        let s = e.screen();
        assert_eq!(s.cells.len(), 30 * 8);
        assert_eq!(s.row_text(0), "abc");
    }

    #[test]
    fn title_and_bell_and_clipboard_are_events() {
        let mut e = emu();
        let out = e.feed(b"\x1b]0;my shell\x07\x07\x1b]52;c;aGk=\x07");
        assert_eq!(e.title(), Some("my shell"));
        assert!(out.events.contains(&Event::Title("my shell".into())));
        assert!(out.events.contains(&Event::Bell));
        assert!(out.events.contains(&Event::Clipboard("hi".into())));
        let out = e.feed(b"\x1b]2;\x07");
        assert!(out.reply.is_empty());
    }

    #[test]
    fn clipboard_reads_are_never_answered() {
        let mut e = emu();
        let out = e.feed(b"\x1b]52;c;?\x07");
        assert!(out.reply.is_empty());
    }

    #[test]
    fn primary_and_secondary_device_attributes_are_answered() {
        let mut e = emu();
        assert_eq!(e.feed(b"\x1b[c").reply, b"\x1b[?6c");
        let secondary = e.feed(b"\x1b[>c").reply;
        assert!(secondary.starts_with(b"\x1b[>0;"));
    }

    #[test]
    fn cursor_position_and_status_reports_are_answered() {
        let mut e = emu();
        e.feed(b"ab");
        assert_eq!(e.feed(b"\x1b[6n").reply, b"\x1b[1;3R");
        assert_eq!(e.feed(b"\x1b[5n").reply, b"\x1b[0n");
    }

    #[test]
    fn size_queries_are_answered() {
        let mut e = Emulator::new(40, 12, 10);
        assert_eq!(e.feed(b"\x1b[18t").reply, b"\x1b[8;12;40t");
        e.set_cell_pixels(10, 20);
        assert_eq!(e.feed(b"\x1b[14t").reply, b"\x1b[4;240;400t");
    }

    #[test]
    fn colour_queries_use_the_scheme() {
        let mut e = emu();
        e.set_scheme(ColorScheme {
            foreground: (0x11, 0x22, 0x33),
            background: (0xaa, 0xbb, 0xcc),
            ..ColorScheme::default()
        });
        let fg = String::from_utf8(e.feed(b"\x1b]10;?\x07").reply).unwrap();
        assert_eq!(fg, "\x1b]10;rgb:1111/2222/3333\x07");
        let bg = String::from_utf8(e.feed(b"\x1b]11;?\x1b\\").reply).unwrap();
        assert_eq!(bg, "\x1b]11;rgb:aaaa/bbbb/cccc\x1b\\");
        let ansi = String::from_utf8(e.feed(b"\x1b]4;196;?\x07").reply).unwrap();
        assert_eq!(ansi, "\x1b]4;196;rgb:ffff/0000/0000\x07");
    }

    #[test]
    fn the_kitty_keyboard_query_reports_and_tracks_flags() {
        let mut e = emu();
        assert_eq!(e.feed(b"\x1b[?u").reply, b"\x1b[?0u");
        e.feed(b"\x1b[>5u");
        assert_eq!(e.feed(b"\x1b[?u").reply, b"\x1b[?5u");
        let flags = e.modes().kitty;
        assert!(flags.has(KittyFlags::DISAMBIGUATE));
        assert!(flags.has(KittyFlags::ALTERNATE_KEYS));
        assert!(!flags.has(KittyFlags::ALL_KEYS));
        e.feed(b"\x1b[<u");
        assert!(!e.modes().kitty.any());
    }

    #[test]
    fn input_modes_follow_the_child() {
        let mut e = emu();
        let idle = Modes {
            alternate_scroll: true,
            ..Modes::default()
        };
        assert_eq!(e.modes(), idle);
        e.feed(b"\x1b[?2004h\x1b[?1h\x1b[?1002h\x1b[?1006h\x1b[?1004h");
        let m = e.modes();
        assert!(m.bracketed_paste && m.app_cursor && m.sgr_mouse && m.focus_reporting);
        assert_eq!(m.mouse, MouseMode::Drag);
        e.feed(b"\x1b[?1049h");
        assert!(e.modes().alt_screen);
        e.feed(b"\x1b[?1049l\x1b[?2004l\x1b[?1002l");
        assert!(!e.modes().bracketed_paste);
        assert_eq!(e.modes().mouse, MouseMode::Off);
    }

    #[test]
    fn cursor_shape_and_visibility_are_reported() {
        let mut e = emu();
        e.feed(b"\x1b[6 q");
        assert_eq!(e.screen().cursor.unwrap().shape, CursorShape::Beam);
        e.feed(b"\x1b[4 q");
        assert_eq!(e.screen().cursor.unwrap().shape, CursorShape::Underline);
        e.feed(b"\x1b[?25l");
        assert!(e.screen().cursor.is_none());
    }

    #[test]
    fn hyperlinks_are_readable_by_cell() {
        let mut e = emu();
        e.feed(b"\x1b]8;;https://example.test/a\x1b\\link\x1b]8;;\x1b\\ plain");
        assert_eq!(
            e.hyperlink_at(1, 0).as_deref(),
            Some("https://example.test/a")
        );
        assert_eq!(e.hyperlink_at(6, 0), None);
        assert_eq!(e.hyperlink_at(99, 0), None);
    }

    #[test]
    fn graphics_sequences_are_dropped() {
        let mut e = emu();
        e.feed(b"a\x1bPq#0;2;0;0;0~~~\x1b\\b");
        assert_eq!(e.screen().row_text(0), "ab");
    }

    #[test]
    fn the_scheme_fills_the_256_colour_cube() {
        let s = ColorScheme::default();
        assert_eq!(s.lookup(16), (0, 0, 0));
        assert_eq!(s.lookup(231), (255, 255, 255));
        assert_eq!(s.lookup(232), (8, 8, 8));
        assert_eq!(s.lookup(257), s.background);
    }
}
