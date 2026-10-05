//! Small text helpers for the panes: word wrapping, markdown flattening and column joins.

use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn cells(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Word-wraps `text` to `width` cells. Words longer than a line are split.
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
