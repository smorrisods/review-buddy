//! Small text helpers for the panes: word wrapping, markdown flattening and column joins.

use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn cells(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Word-wraps `text` to `width` cells. Words longer than a line are split.
/// Shortens `text` to at most `max` cells by replacing the middle with `…`, keeping both ends
/// (so `git.ontariogovernment.ca` stays recognisable as `git.onta…ent.ca`).
pub fn elide_middle(text: &str, max: usize) -> String {
    if cells(text) <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let keep = max - 1;
    let tail_cells = keep / 2;
    let head_cells = keep - tail_cells;
    let take = |chars: &mut dyn Iterator<Item = char>, limit: usize| {
        let mut out = Vec::new();
        let mut used = 0;
        for ch in chars {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + w > limit {
                break;
            }
            used += w;
            out.push(ch);
        }
        out
    };
    let head: String = take(&mut text.chars(), head_cells).into_iter().collect();
    let mut tail = take(&mut text.chars().rev(), tail_cells);
    tail.reverse();
    format!("{head}…{}", tail.into_iter().collect::<String>())
}

pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let mut word = word.to_string();
            while cells(&word) > width {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                let (head, tail) = split_at_width(&word, width);
                lines.push(head);
                word = tail;
            }
            let joined = if line.is_empty() { 0 } else { cells(&line) + 1 };
            if !line.is_empty() && joined + cells(&word) > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&word);
        }
        lines.push(line);
    }
    lines
}

fn split_at_width(word: &str, width: usize) -> (String, String) {
    let mut used = 0;
    let mut cut = word.len();
    for (at, ch) in word.char_indices() {
        let w = ch.width().unwrap_or(0);
        if used + w > width {
            cut = at;
            break;
        }
        used += w;
    }
    if cut == 0 {
        cut = word.chars().next().map_or(0, char::len_utf8);
    }
    (word[..cut].to_string(), word[cut..].to_string())
}

/// Walks `raw` (a source line, tabs not yet expanded) and calls `on_break` with the cell offset,
/// in the tab-expanded text, at which each continuation row of a `width`-cell row begins.
/// Breaks fall between grapheme clusters, so a wide character, an emoji sequence or a base
/// letter with its combining marks never splits, and a tab's expansion moves to the next row as
/// one unit (unless it is wider than the row, when it has to be cut).
fn walk_wrap(raw: &str, tab_width: u8, width: usize, mut on_break: impl FnMut(usize)) {
    let width = width.max(1);
    let tab = usize::from(tab_width.max(1));
    let (mut pos, mut row_start, mut col) = (0usize, 0usize, 0usize);
    let mut place = |w: usize, pos: &mut usize, row_start: &mut usize| {
        if w > 0 && *pos > *row_start && *pos - *row_start + w > width {
            on_break(*pos);
            *row_start = *pos;
        }
        *pos += w;
    };
    for g in raw.graphemes(true) {
        if g == "\t" {
            let n = tab - col % tab;
            col += n;
            if n > width {
                for _ in 0..n {
                    place(1, &mut pos, &mut row_start);
                }
            } else {
                place(n, &mut pos, &mut row_start);
            }
        } else {
            col += g.chars().count();
            place(UnicodeWidthStr::width(g), &mut pos, &mut row_start);
        }
    }
}

/// The cell offsets where the continuation rows of `raw` start when it is wrapped to `width`
/// cells; see [`walk_wrap`]. Empty when the line fits on one row.
pub fn wrap_points(raw: &str, tab_width: u8, width: usize) -> Vec<usize> {
    let mut points = Vec::new();
    walk_wrap(raw, tab_width, width, |at| points.push(at));
    points
}

/// The cell offset at which row `sub` of a wrapped line starts, given its [`wrap_points`].
pub fn row_start(points: &[usize], sub: usize) -> usize {
    sub.checked_sub(1)
        .and_then(|i| points.get(i))
        .copied()
        .unwrap_or(0)
}

/// How many rows `raw` takes when wrapped to `width` cells (at least one).
pub fn wrap_rows(raw: &str, tab_width: u8, width: usize) -> usize {
    if raw.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return raw.len().div_ceil(width.max(1)).max(1);
    }
    let mut rows = 1;
    walk_wrap(raw, tab_width, width, |_| rows += 1);
    rows
}

/// Cuts styled `spans` into rows at the cell offsets in `points`, keeping each piece's style so
/// highlighting carries across a break. Always returns at least one row.
pub fn split_spans(spans: Vec<Span<'static>>, points: &[usize]) -> Vec<Vec<Span<'static>>> {
    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let (mut pos, mut next) = (0usize, 0usize);
    for span in spans {
        let mut piece = String::new();
        for g in span.content.graphemes(true) {
            let w = UnicodeWidthStr::width(g);
            if w > 0 && points.get(next).is_some_and(|&p| pos >= p) {
                if !piece.is_empty() {
                    let done = std::mem::take(&mut piece);
                    rows.last_mut()
                        .expect("rows starts non-empty")
                        .push(Span::styled(done, span.style));
                }
                rows.push(Vec::new());
                next += 1;
            }
            piece.push_str(g);
            pos += w;
        }
        if !piece.is_empty() {
            rows.last_mut()
                .expect("rows starts non-empty")
                .push(Span::styled(piece, span.style));
        }
    }
    rows
}

/// Flattens the markdown a description is likely to use into plain, readable lines.
pub fn plain_markdown(body: &str) -> String {
    let mut out = Vec::new();
    let mut in_fence = false;
    for raw in body.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            out.push(format!("  {line}"));
            continue;
        }
        let trimmed = line.trim_start();
        let text = if let Some(rest) = trimmed.strip_prefix(['-', '*']) {
            match rest.strip_prefix(' ') {
                Some(item) => format!("• {item}"),
                None => trimmed.to_string(),
            }
        } else {
            trimmed.trim_start_matches('#').trim_start().to_string()
        };
        out.push(text.replace("**", "").replace('`', ""));
    }
    out.join("\n")
}

pub fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| cells(&s.content)).sum()
}

/// `left` and `right` as one row, with `right` pushed against the right edge of `width`.
pub fn justify(
    mut left: Vec<Span<'static>>,
    right: Vec<Span<'static>>,
    width: usize,
) -> Line<'static> {
    let used = spans_width(&left) + spans_width(&right);
    let gap = width.saturating_sub(used).max(1);
    left.push(Span::raw(" ".repeat(gap)));
    left.extend(right);
    Line::from(left)
}

/// Two columns side by side; `left_width` is the first column's width in cells.
pub fn columns(
    left: Vec<Line<'static>>,
    right: Vec<Line<'static>>,
    left_width: usize,
) -> Vec<Line<'static>> {
    let rows = left.len().max(right.len());
    (0..rows)
        .map(|i| {
            let mut spans = left.get(i).map(|l| l.spans.clone()).unwrap_or_default();
            let pad = left_width.saturating_sub(spans_width(&spans));
            spans.push(Span::raw(" ".repeat(pad)));
            spans.extend(right.get(i).map(|l| l.spans.clone()).unwrap_or_default());
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_on_words_and_keeps_blank_lines() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("a\n\nb", 10), ["a", "", "b"]);
        assert_eq!(wrap("", 10), Vec::<String>::new());
    }

    #[test]
    fn long_words_are_split_and_wide_chars_counted() {
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("日本語です", 4), ["日本", "語で", "す"]);
        assert_eq!(wrap("x", 0), ["x"]);
    }

    #[test]
    fn markdown_is_flattened() {
        let md = "# Title\n\nSome **bold** `code`.\n\n- one\n* two\n```\nlet x = 1;\n```";
        assert_eq!(
            plain_markdown(md),
            "Title\n\nSome bold code.\n\n• one\n• two\n  let x = 1;"
        );
    }

    #[test]
    fn justify_pushes_right_text_to_the_edge() {
        let line = justify(vec![Span::raw("ab")], vec![Span::raw("cd")], 10);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "ab      cd");
        let tight = justify(vec![Span::raw("abcd")], vec![Span::raw("ef")], 3);
        assert_eq!(spans_width(&tight.spans), 7);
    }

    #[test]
    fn columns_pad_the_left_side() {
        let out = columns(
            vec![Line::from("a"), Line::from("b")],
            vec![Line::from("x")],
            4,
        );
        let text: Vec<String> = out
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(text, ["a   x", "b   "]);
    }
}

#[cfg(test)]
mod elide_tests {
    use super::*;

    #[test]
    fn elides_the_middle_and_keeps_both_ends() {
        assert_eq!(elide_middle("github.com", 12), "github.com");
        let e = elide_middle("git.ontariogovernment.ca", 14);
        assert_eq!(cells(&e), 14);
        assert!(
            e.starts_with("git.on") && e.ends_with("ent.ca") || e.ends_with(".ca"),
            "{e}"
        );
        assert!(e.contains('…'));
        assert_eq!(elide_middle("abcdef", 1), "…");
        assert_eq!(elide_middle("abcdef", 0), "");
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::*;

    fn rows(raw: &str, tab: u8, width: usize) -> Vec<String> {
        let expanded = rb_diff::expand_tabs(raw, tab);
        let points = wrap_points(raw, tab, width);
        split_spans(vec![Span::raw(expanded)], &points)
            .into_iter()
            .map(|r| r.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn short_lines_stay_whole() {
        assert_eq!(rows("hello", 4, 10), ["hello"]);
        assert_eq!(rows("", 4, 10), [""]);
        assert_eq!(wrap_rows("hello", 4, 5), 1);
    }

    #[test]
    fn ascii_breaks_at_the_width_with_no_word_logic() {
        assert_eq!(rows("abcdefghij", 4, 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap_rows("abcdefghij", 4, 4), 3);
        assert_eq!(rows("aaaa bbbb", 4, 5), ["aaaa ", "bbbb"]);
    }

    #[test]
    fn a_long_unbroken_token_is_cut_at_the_edge() {
        let token = "x".repeat(250);
        let out = rows(&token, 4, 80);
        assert_eq!(
            out.iter().map(|r| r.len()).collect::<Vec<_>>(),
            [80, 80, 80, 10]
        );
    }

    #[test]
    fn wide_characters_never_split() {
        assert_eq!(rows("日本語です", 4, 5), ["日本", "語で", "す"]);
        assert_eq!(rows("a日本", 4, 2), ["a", "日", "本"]);
        assert_eq!(wrap_rows("日本語です", 4, 5), 3);
    }

    #[test]
    fn a_wide_character_wider_than_the_row_still_makes_progress() {
        assert_eq!(rows("日日", 4, 1), ["日", "日"]);
    }

    #[test]
    fn emoji_sequences_and_combining_marks_stay_together() {
        let family = "👨\u{200d}👩\u{200d}👧";
        let out = rows(&format!("ab{family}cd"), 4, 3);
        assert!(
            out.iter().any(|r| r == family || r.contains(family)),
            "{out:?}"
        );
        for row in &out {
            assert!(!row.starts_with('\u{200d}'));
        }
        let accent = "e\u{301}";
        let out = rows(&accent.repeat(5), 4, 2);
        assert_eq!(
            out,
            [accent.repeat(2), accent.repeat(2), accent.to_string()]
        );
    }

    #[test]
    fn a_tab_expansion_moves_whole_to_the_next_row() {
        // "ab" then a tab to column 4 (two spaces): 2 + 2 > 3 so all of it moves down.
        assert_eq!(rows("ab\tc", 4, 3), ["ab", "  c"]);
        // A tab that cannot fit any row is cut at the edge.
        assert_eq!(rows("\tx", 8, 3), ["   ", "   ", "  x"]);
        assert_eq!(wrap_rows("ab\tc", 4, 3), 2);
    }

    #[test]
    fn styles_follow_the_text_across_a_break() {
        use ratatui::style::{Color, Style};
        let red = Style::default().fg(Color::Red);
        let blue = Style::default().fg(Color::Blue);
        let out = split_spans(
            vec![Span::styled("abc", red), Span::styled("def", blue)],
            &wrap_points("abcdef", 4, 4),
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0][0].style, red);
        assert_eq!(out[0][1].content, "d");
        assert_eq!(out[0][1].style, blue);
        assert_eq!(out[1][0].content, "ef");
        assert_eq!(out[1][0].style, blue);
    }

    #[test]
    fn row_counts_agree_with_the_points() {
        for raw in [
            "",
            "plain",
            "日本語 mixed ascii",
            "a\tb\tc\td",
            "e\u{301}e\u{301}x",
        ] {
            for width in 1..9 {
                assert_eq!(
                    wrap_rows(raw, 4, width),
                    wrap_points(raw, 4, width).len() + 1,
                    "{raw:?} at {width}"
                );
            }
        }
    }
}
