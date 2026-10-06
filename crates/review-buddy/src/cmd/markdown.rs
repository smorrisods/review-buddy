//! Markdown to readable terminal text: wrapped paragraphs, bullets, quotes and indented code.
//! Pure: no I/O, and colour only through the [`Painter`] it is given.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use rb_theme::Role;
use unicode_width::UnicodeWidthStr;

use super::output::Painter;

const MIN_WIDTH: usize = 20;

/// Renders `markdown` for a terminal `width` columns wide. Trailing blank lines are trimmed.
pub fn render(markdown: &str, width: usize, painter: &Painter) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = Renderer::new(width.max(MIN_WIDTH), painter);
    for event in Parser::new_ext(markdown, options) {
        r.event(event);
    }
    r.finish()
}

struct Word {
    text: String,
    role: Option<Role>,
    /// A hard line break comes before this word.
    break_before: bool,
}

struct Container {
    cont: String,
    marker: Option<String>,
}

struct Renderer<'a> {
    width: usize,
    painter: &'a Painter,
    out: Vec<String>,
    words: Vec<Word>,
    glue: bool,
    containers: Vec<Container>,
    lists: Vec<Option<u64>>,
    roles: Vec<Role>,
    links: Vec<String>,
    link_text: Vec<String>,
    code: Option<Vec<String>>,
    heading: bool,
}

impl<'a> Renderer<'a> {
    fn new(width: usize, painter: &'a Painter) -> Self {
        Self {
            width,
            painter,
            out: Vec::new(),
            words: Vec::new(),
            glue: false,
            containers: Vec::new(),
            lists: Vec::new(),
            roles: Vec::new(),
            links: Vec::new(),
            link_text: Vec::new(),
            code: None,
            heading: false,
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                self.push_text(&format!("`{code}`"), Some(Role::Cyan));
            }
            Event::SoftBreak => self.glue = false,
            Event::HardBreak => self.hard_break(),
            Event::Rule => {
                self.flush();
                self.blank_if_top();
                self.out.push("---".to_string());
                self.blank_if_top();
            }
            Event::TaskListMarker(done) => {
                self.push_text(if done { "[x]" } else { "[ ]" }, None);
                self.glue = false;
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = true;
                self.roles.push(Role::TextBright);
                if level == HeadingLevel::H1 {
                    self.roles.push(Role::TextBright);
                }
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.containers.push(Container {
                    cont: "> ".into(),
                    marker: None,
                });
            }
            Tag::CodeBlock(_) => {
                self.flush();
                self.code = Some(Vec::new());
            }
            Tag::List(start) => {
                self.flush();
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "- ".to_string(),
                };
                self.containers.push(Container {
                    cont: " ".repeat(marker.width()),
                    marker: Some(marker),
                });
            }
            Tag::Emphasis | Tag::Strikethrough => {}
            Tag::Strong => self.roles.push(Role::TextBright),
            Tag::Link { dest_url, .. } => {
                self.links.push(dest_url.to_string());
                self.link_text.push(String::new());
            }
            Tag::Image { dest_url, .. } => {
                self.links.push(dest_url.to_string());
                self.link_text.push(String::new());
                self.push_text("[image:", None);
                self.glue = false;
            }
            Tag::TableCell => {}
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush();
                self.blank_if_top();
            }
            TagEnd::Heading(level) => {
                self.flush();
                self.heading = false;
                self.roles.pop();
                if level == HeadingLevel::H1 {
                    self.roles.pop();
                }
                self.blank_if_top();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.containers.pop();
                self.blank_if_top();
            }
            TagEnd::CodeBlock => {
                if let Some(lines) = self.code.take() {
                    for line in lines {
                        let text = if line.is_empty() {
                            String::new()
                        } else {
                            self.painter.paint(Role::Muted, &format!("    {line}"))
                        };
                        let prefix = self.prefix(false);
                        self.out
                            .push(format!("{prefix}{text}").trim_end().to_string());
                    }
                }
                self.blank_if_top();
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                self.blank_if_top();
            }
            TagEnd::Item => {
                self.flush();
                self.containers.pop();
            }
            TagEnd::Strong => {
                self.roles.pop();
            }
            TagEnd::Link => {
                let url = self.links.pop().unwrap_or_default();
                let label = self.link_text.pop().unwrap_or_default();
                if !url.is_empty() && label != url {
                    self.glue = false;
                    self.push_text(&format!("({url})"), Some(Role::Interactive));
                }
            }
            TagEnd::Image => {
                let url = self.links.pop().unwrap_or_default();
                self.link_text.pop();
                self.glue = false;
                self.push_text(&format!("{url}]"), Some(Role::Interactive));
            }
            TagEnd::TableCell => {
                self.glue = false;
                self.push_text("|", Some(Role::Muted));
                self.glue = false;
            }
            TagEnd::TableHead | TagEnd::TableRow => self.flush(),
            TagEnd::Table => self.blank_if_top(),
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if let Some(lines) = &mut self.code {
            let trimmed = text.strip_suffix('\n').unwrap_or(text);
            lines.extend(trimmed.split('\n').map(str::to_string));
            return;
        }
        if let Some(label) = self.link_text.last_mut() {
            label.push_str(text);
        }
        self.push_text(text, None);
    }

    /// Splits on whitespace so wrapping can happen between words. Text that starts or ends
    /// without a space stays glued to its neighbour (for example the `.` after inline code).
    fn push_text(&mut self, text: &str, role: Option<Role>) {
        let role = role.or_else(|| self.roles.last().copied());
        let leading = text.starts_with(char::is_whitespace);
        let trailing = text.ends_with(char::is_whitespace);
        let mut first = true;
        for piece in text.split_whitespace() {
            let space = if first { leading || !self.glue } else { true };
            let space = space && !self.words.is_empty();
            let text = if space {
                format!(" {piece}")
            } else {
                piece.to_string()
            };
            self.words.push(Word {
                text,
                role,
                break_before: false,
            });
            first = false;
        }
        if first {
            self.glue = !trailing && self.glue;
            if leading || trailing {
                self.glue = false;
            }
        } else {
            self.glue = !trailing;
        }
    }

    fn hard_break(&mut self) {
        self.glue = false;
        self.words.push(Word {
            text: String::new(),
            role: None,
            break_before: true,
        });
    }

    fn prefix(&mut self, first: bool) -> String {
        self.containers
            .iter_mut()
            .map(|c| {
                if first {
                    c.marker.take().unwrap_or_else(|| c.cont.clone())
                } else {
                    c.cont.clone()
                }
            })
            .collect()
    }

    fn flush(&mut self) {
        self.glue = false;
        if self
            .words
            .iter()
            .all(|w| w.text.is_empty() && !w.break_before)
        {
            self.words.clear();
            return;
        }
        let words = std::mem::take(&mut self.words);
        let first_prefix = self.prefix(true);
        let cont_prefix: String = self.containers.iter().map(|c| c.cont.clone()).collect();
        let budget = self.width.saturating_sub(cont_prefix.width()).max(10);

        let mut lines: Vec<Vec<Word>> = vec![Vec::new()];
        let mut used = 0;
        for word in words {
            if word.break_before {
                lines.push(Vec::new());
                used = 0;
                continue;
            }
            let mut text = word.text;
            if used > 0 && used + text.width() > budget {
                lines.push(Vec::new());
                used = 0;
            }
            if used == 0 {
                text = text.trim_start().to_string();
            }
            used += text.width();
            lines.last_mut().expect("at least one line").push(Word {
                text,
                role: word.role,
                break_before: false,
            });
        }
        for (i, line) in lines.into_iter().enumerate() {
            let body: String = line
                .iter()
                .map(|w| match w.role {
                    Some(role) => self.painter.paint(role, &w.text),
                    None => w.text.clone(),
                })
                .collect();
            let prefix = if i == 0 { &first_prefix } else { &cont_prefix };
            self.out
                .push(format!("{prefix}{body}").trim_end().to_string());
        }
    }

    fn blank_if_top(&mut self) {
        if self.lists.is_empty() && self.out.last().is_some_and(|l| !l.is_empty()) {
            self.out.push(String::new());
        }
    }

    fn finish(mut self) -> String {
        self.flush();
        while self.out.last().is_some_and(String::is_empty) {
            self.out.pop();
        }
        let mut text = self.out.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::output::colour::strip_ansi;

    fn plain(md: &str, width: usize) -> String {
        render(md, width, &Painter::plain())
    }

    #[test]
    fn paragraphs_wrap_and_separate() {
        let text = plain("one two three four five six seven\n\nnext", 20);
        assert_eq!(text, "one two three four\nfive six seven\n\nnext\n");
    }

    #[test]
    fn soft_breaks_join_lines() {
        assert_eq!(plain("a\nb", 40), "a b\n");
    }

    #[test]
    fn hard_breaks_start_a_new_line() {
        assert_eq!(plain("a  \nb", 40), "a\nb\n");
    }

    #[test]
    fn headings_stay_on_their_own_line() {
        assert_eq!(plain("# Title\n\ntext", 40), "Title\n\ntext\n");
    }

    #[test]
    fn bullets_and_numbers_indent_their_wrapped_lines() {
        let text = plain("- alpha beta gamma delta\n- two\n\n1. one\n2. two", 20);
        assert_eq!(
            text,
            "- alpha beta gamma\n  delta\n- two\n\n1. one\n2. two\n"
        );
    }

    #[test]
    fn nested_lists_indent_further() {
        let text = plain("- a\n  - b\n  - c\n- d", 40);
        assert_eq!(text, "- a\n  - b\n  - c\n- d\n");
    }

    #[test]
    fn task_lists_keep_their_boxes() {
        assert_eq!(
            plain("- [x] done\n- [ ] todo", 40),
            "- [x] done\n- [ ] todo\n"
        );
    }

    #[test]
    fn quotes_are_prefixed() {
        assert_eq!(plain("> quoted words", 40), "> quoted words\n");
    }

    #[test]
    fn code_blocks_are_indented_and_not_wrapped() {
        let text = plain("```rust\nlet a = 1;\n\nlet b = 2;\n```\n\nafter", 20);
        assert_eq!(text, "    let a = 1;\n\n    let b = 2;\n\nafter\n");
    }

    #[test]
    fn inline_code_keeps_backticks_and_glues_punctuation() {
        assert_eq!(plain("Use `Menu::new` now.", 40), "Use `Menu::new` now.\n");
        assert_eq!(plain("See `x`.", 40), "See `x`.\n");
    }

    #[test]
    fn links_show_their_address_unless_it_is_the_text() {
        assert_eq!(
            plain("[docs](https://example.com/a)", 80),
            "docs (https://example.com/a)\n"
        );
        assert_eq!(plain("<https://example.com>", 80), "https://example.com\n");
    }

    #[test]
    fn images_become_a_note() {
        assert_eq!(plain("![shot](a.png)", 80), "[image: shot a.png]\n");
    }

    #[test]
    fn html_comments_are_dropped() {
        assert_eq!(plain("<!-- template -->\n\nHello", 40), "Hello\n");
    }

    #[test]
    fn rules_and_tables_are_readable() {
        assert_eq!(plain("a\n\n---\n\nb", 40), "a\n\n---\n\nb\n");
        let table = plain("| a | b |\n|---|---|\n| 1 | 2 |", 40);
        assert!(
            table.contains("a | b |") && table.contains("1 | 2 |"),
            "{table}"
        );
    }

    #[test]
    fn empty_input_is_empty_output() {
        assert_eq!(plain("", 40), "");
        assert_eq!(plain("  \n\n", 40), "");
    }

    #[test]
    fn colour_only_wraps_text_and_strips_back_to_plain() {
        let (_, painter) = coloured();
        let md = "# Hi\n\nSome **bold** and `code`.";
        let coloured = render(md, 40, &painter);
        assert!(coloured.contains('\u{1b}'));
        assert_eq!(strip_ansi(&coloured), plain(md, 40));
    }

    fn coloured() -> (rb_theme::Theme, Painter) {
        use rb_theme::{ColourDepth, Palette, Theme};
        let theme = Theme::default();
        let painter = Painter::new(
            Palette::new(theme.clone(), ColourDepth::TrueColour, false),
            true,
        );
        (theme, painter)
    }
}
