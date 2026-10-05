//! Tables for a terminal and tab-separated rows for a pipe. Both are pure string builders.

use rb_theme::Role;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::colour::Painter;

const GUTTER: &str = "  ";
const MIN_FLEX_WIDTH: usize = 8;

#[derive(Debug, Clone)]
pub struct Column {
    pub header: &'static str,
    /// The column that gives up width (with an ellipsis) when the terminal is narrow.
    pub flex: bool,
}

impl Column {
    pub const fn fixed(header: &'static str) -> Self {
        Self {
            header,
            flex: false,
        }
    }

    pub const fn flex(header: &'static str) -> Self {
        Self { header, flex: true }
    }
}

/// One table cell. `pipe` replaces `text` in TSV output (full title, ISO time, `pass` not `●`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub pipe: Option<String>,
    pub role: Option<Role>,
}

impl Cell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            pipe: None,
            role: None,
        }
    }

    pub fn styled(text: impl Into<String>, role: Role) -> Self {
        Self {
            role: Some(role),
            ..Self::plain(text)
        }
    }

    pub fn with_pipe(mut self, pipe: impl Into<String>) -> Self {
        self.pipe = Some(pipe.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct Table {
    columns: Vec<Column>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    pub fn new(columns: Vec<Column>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, row: Vec<Cell>) {
        debug_assert_eq!(row.len(), self.columns.len());
        self.rows.push(row);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Control characters would break the layout, so they become single spaces.
fn flatten(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Shortens `text` to at most `max` display columns, ending in `…` when anything was cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

fn pad(text: &str, width: usize) -> String {
    let fill = width.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(fill))
}

fn column_widths(table: &Table, cells: &[Vec<String>], width: usize) -> Vec<usize> {
    let mut widths: Vec<usize> = table.columns.iter().map(|c| c.header.width()).collect();
    for row in cells {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.width());
        }
    }
    let gutters = GUTTER.len() * widths.len().saturating_sub(1);
    let mut excess = (widths.iter().sum::<usize>() + gutters).saturating_sub(width);
    while excess > 0 {
        let widest = table
            .columns
            .iter()
            .enumerate()
            .filter(|(i, c)| c.flex && widths[*i] > MIN_FLEX_WIDTH)
            .max_by_key(|(i, _)| widths[*i])
            .map(|(i, _)| i);
        let Some(i) = widest else { break };
        let cut = excess.min(widths[i] - MIN_FLEX_WIDTH);
        widths[i] -= cut;
        excess -= cut;
    }
    widths
}

/// Aligned columns with a muted header row, fitted to `width` by truncating the flex column.
pub fn render_table(table: &Table, width: usize, painter: &Painter) -> String {
    let cells: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|row| row.iter().map(|c| flatten(&c.text)).collect())
        .collect();
    let widths = column_widths(table, &cells, width);
    let last = widths.len().saturating_sub(1);

    let mut out = String::new();
    let header: Vec<String> = table
        .columns
        .iter()
        .zip(&widths)
        .enumerate()
        .map(|(i, (c, w))| {
            let text = pad(c.header, *w);
            let text = if i == last {
                text.trim_end().to_string()
            } else {
                text
            };
            painter.paint(Role::Muted, &text)
        })
        .collect();
    out.push_str(&header.join(GUTTER));
    out.push('\n');

    for (row, texts) in table.rows.iter().zip(&cells) {
        let line: Vec<String> = row
            .iter()
            .zip(texts)
            .zip(&widths)
            .enumerate()
            .map(|(i, ((cell, text), w))| {
                let shown = truncate(text, *w);
                let padded = if i == last { shown } else { pad(&shown, *w) };
                match cell.role {
                    Some(role) => painter.paint(role, &padded),
                    None => padded,
                }
            })
            .collect();
        out.push_str(line.join(GUTTER).trim_end());
        out.push('\n');
    }
    out
}

/// Backslash-escapes the characters that would split a TSV row or cell.
pub fn escape_tsv(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// One line per row, tab-separated, no header, no colour, no truncation.
pub fn render_tsv(table: &Table) -> String {
    let mut out = String::new();
    for row in &table.rows {
        let line: Vec<String> = row
            .iter()
            .map(|c| escape_tsv(c.pipe.as_deref().unwrap_or(&c.text)))
            .collect();
        out.push_str(&line.join("\t"));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Painter {
        Painter::plain()
    }

    fn sample() -> Table {
        let mut t = Table::new(vec![
            Column::fixed("Id"),
            Column::flex("Title"),
            Column::fixed("Age"),
        ]);
        t.push(vec![
            Cell::plain("a#1"),
            Cell::plain("A fairly long pull request title"),
            Cell::plain("2h").with_pipe("2026-10-05T08:00:00Z"),
        ]);
        t.push(vec![
            Cell::plain("bb#22"),
            Cell::plain("Short"),
            Cell::plain("1d"),
        ]);
        t
    }

    #[test]
    fn truncation_counts_display_columns() {
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello", 4), "hel…");
        assert_eq!(truncate("hello", 1), "…");
        assert_eq!(truncate("hello", 0), "");
        assert_eq!(truncate("日本語です", 5), "日本…");
        assert_eq!(truncate("日本", 4), "日本");
    }

    #[test]
    fn wide_terminals_align_without_truncating() {
        let out = render_table(&sample(), 120, &plain());
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "Id     Title                             Age");
        assert_eq!(lines[1], "a#1    A fairly long pull request title  2h");
        assert_eq!(lines[2], "bb#22  Short                             1d");
    }

    #[test]
    fn narrow_terminals_truncate_the_flex_column() {
        let out = render_table(&sample(), 30, &plain());
        for line in out.lines() {
            assert!(line.width() <= 30, "{line:?}");
        }
        assert!(out.contains("A fairly long…") || out.contains('…'), "{out}");
        assert!(out.contains("2h") && out.contains("bb#22"));
    }

    #[test]
    fn flex_never_shrinks_below_its_floor() {
        let out = render_table(&sample(), 5, &plain());
        assert!(out.lines().nth(1).unwrap().contains("A fairl…"), "{out}");
    }

    #[test]
    fn control_characters_never_reach_the_terminal() {
        let mut t = Table::new(vec![Column::fixed("A"), Column::flex("B")]);
        t.push(vec![
            Cell::plain("x\ty"),
            Cell::plain("line\nbreak\u{1b}[31m"),
        ]);
        let out = render_table(&t, 80, &plain());
        assert_eq!(out.lines().count(), 2);
        assert!(!out.contains('\u{1b}') && !out.contains('\t'));
    }

    #[test]
    fn tsv_uses_pipe_text_with_no_header_or_truncation() {
        let out = render_tsv(&sample());
        assert_eq!(
            out,
            "a#1\tA fairly long pull request title\t2026-10-05T08:00:00Z\nbb#22\tShort\t1d\n"
        );
    }

    #[test]
    fn tsv_escapes_tabs_newlines_and_backslashes() {
        let mut t = Table::new(vec![Column::fixed("A"), Column::fixed("B")]);
        t.push(vec![Cell::plain("a\tb"), Cell::plain("c\nd\r\\e")]);
        assert_eq!(render_tsv(&t), "a\\tb\tc\\nd\\r\\\\e\n");
        assert_eq!(render_tsv(&t).lines().count(), 1);
    }

    #[test]
    fn empty_tables_render_headers_only_or_nothing() {
        let t = Table::new(vec![Column::fixed("A")]);
        assert!(t.is_empty());
        assert_eq!(render_tsv(&t), "");
        assert_eq!(render_table(&t, 40, &plain()), "A\n");
    }

    #[test]
    fn coloured_cells_pad_on_plain_text() {
        let palette = rb_theme::Palette::new(
            rb_theme::Theme::default(),
            rb_theme::ColourDepth::TrueColour,
            false,
        );
        let painter = Painter::new(palette, true);
        let mut t = Table::new(vec![Column::fixed("A"), Column::fixed("B")]);
        t.push(vec![Cell::styled("x", Role::Success), Cell::plain("y")]);
        let out = render_table(&t, 40, &painter);
        assert!(out.contains("\u{1b}["));
        let stripped = super::super::colour::strip_ansi(&out);
        assert_eq!(stripped.lines().nth(1).unwrap(), "x  y");
    }
}
