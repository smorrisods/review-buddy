//! A small multi-line text buffer for the composer. Pure and cursor-based: columns count
//! characters, never bytes, so editing is safe for any text.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    lines: Vec<String>,
    row: usize,
    col: usize,
    /// The column up/down try to return to after passing through a shorter line.
    want: Option<usize>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            want: None,
        }
    }
}

fn byte_at(line: &str, col: usize) -> usize {
    line.char_indices().nth(col).map_or(line.len(), |(i, _)| i)
}

fn chars(line: &str) -> usize {
    line.chars().count()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }

    /// A buffer holding `text`, with the cursor at the end.
    pub fn with_text(text: &str) -> Self {
        let mut editor = Self::new();
        editor.insert_str(text);
        editor
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Nothing but whitespace: there is nothing to lose by closing.
    pub fn is_blank(&self) -> bool {
        self.lines.iter().all(|l| l.trim().is_empty())
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// `(row, column)`, both zero-based, the column in characters.
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    fn line_len(&self) -> usize {
        chars(&self.lines[self.row])
    }

    pub fn insert_char(&mut self, c: char) {
        if c == '\n' {
            return self.newline();
        }
        self.want = None;
        let at = byte_at(&self.lines[self.row], self.col);
        self.lines[self.row].insert(at, c);
        self.col += 1;
    }

    /// Inserts pasted or prefilled text. Line endings (`\r\n`, `\r`) become newlines and
    /// control characters other than tab are dropped.
    pub fn insert_str(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        for c in text.chars() {
            match c {
                '\n' => self.newline(),
                '\t' => self.insert_char('\t'),
                c if c.is_control() => {}
                c => self.insert_char(c),
            }
        }
    }

    pub fn newline(&mut self) {
        self.want = None;
        let at = byte_at(&self.lines[self.row], self.col);
        let rest = self.lines[self.row].split_off(at);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        self.want = None;
        if self.col > 0 {
            let at = byte_at(&self.lines[self.row], self.col - 1);
            self.lines[self.row].remove(at);
            self.col -= 1;
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_len();
            self.lines[self.row].push_str(&line);
        }
    }

    pub fn delete(&mut self) {
        self.want = None;
        if self.col < self.line_len() {
            let at = byte_at(&self.lines[self.row], self.col);
            self.lines[self.row].remove(at);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    pub fn left(&mut self) {
        self.want = None;
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.line_len();
        }
    }

    pub fn right(&mut self) {
        self.want = None;
        if self.col < self.line_len() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn up(&mut self) {
        if self.row == 0 {
            self.col = 0;
            self.want = None;
            return;
        }
        let want = *self.want.get_or_insert(self.col);
        self.row -= 1;
        self.col = want.min(self.line_len());
    }

    pub fn down(&mut self) {
        if self.row + 1 >= self.lines.len() {
            self.col = self.line_len();
            self.want = None;
            return;
        }
        let want = *self.want.get_or_insert(self.col);
        self.row += 1;
        self.col = want.min(self.line_len());
    }

    pub fn home(&mut self) {
        self.want = None;
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.want = None;
        self.col = self.line_len();
    }

    pub fn start(&mut self) {
        self.want = None;
        self.row = 0;
        self.col = 0;
    }

    pub fn finish(&mut self) {
        self.want = None;
        self.row = self.lines.len() - 1;
        self.col = self.line_len();
    }

    /// Back to the start of the word before the cursor, crossing line breaks.
    pub fn word_left(&mut self) {
        self.want = None;
        while self.col == 0 && self.row > 0 {
            self.left();
        }
        let line: Vec<char> = self.lines[self.row].chars().collect();
        while self.col > 0 && !is_word(line[self.col - 1]) {
            self.col -= 1;
        }
        while self.col > 0 && is_word(line[self.col - 1]) {
            self.col -= 1;
        }
    }

    /// Forward to the end of the word after the cursor, crossing line breaks.
    pub fn word_right(&mut self) {
        self.want = None;
        while self.col == self.line_len() && self.row + 1 < self.lines.len() {
            self.right();
        }
        let line: Vec<char> = self.lines[self.row].chars().collect();
        while self.col < line.len() && !is_word(line[self.col]) {
            self.col += 1;
        }
        while self.col < line.len() && is_word(line[self.col]) {
            self.col += 1;
        }
    }

    /// Deletes back to the start of the previous word.
    pub fn delete_word(&mut self) {
        let (row, col) = self.cursor();
        self.word_left();
        if (self.row, self.col) == (row, col) {
            return;
        }
        let at = byte_at(&self.lines[row], col);
        let tail = self.lines[row].split_off(at);
        self.lines.drain(self.row + 1..=row);
        let head = &mut self.lines[self.row];
        let at = byte_at(head, self.col);
        head.truncate(at);
        head.push_str(&tail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Editor {
        let mut e = Editor::new();
        e.insert_str(text);
        e
    }

    #[test]
    fn typing_builds_text_and_moves_the_cursor() {
        let mut e = Editor::new();
        for c in "héllo".chars() {
            e.insert_char(c);
        }
        assert_eq!(e.text(), "héllo");
        assert_eq!(e.cursor(), (0, 5));
    }

    #[test]
    fn newline_splits_the_line_at_the_cursor() {
        let mut e = typed("abcd");
        e.left();
        e.left();
        e.newline();
        assert_eq!(e.lines(), ["ab", "cd"]);
        assert_eq!(e.cursor(), (1, 0));
        e.insert_char('\n');
        assert_eq!(e.line_count(), 3);
    }

    #[test]
    fn backspace_deletes_and_joins_lines() {
        let mut e = typed("ab\ncd");
        e.backspace();
        assert_eq!(e.text(), "ab\nc");
        e.home();
        e.backspace();
        assert_eq!(e.lines(), ["abc"]);
        assert_eq!(e.cursor(), (0, 2));
        e.start();
        e.backspace();
        assert_eq!(e.text(), "abc", "nothing before the start");
    }

    #[test]
    fn delete_removes_forward_and_joins_the_next_line() {
        let mut e = typed("ab\ncd");
        e.start();
        e.delete();
        assert_eq!(e.text(), "b\ncd");
        e.end();
        e.delete();
        assert_eq!(e.lines(), ["bcd"]);
        e.finish();
        e.delete();
        assert_eq!(e.text(), "bcd");
    }

    #[test]
    fn left_and_right_wrap_across_lines() {
        let mut e = typed("a\nb");
        e.home();
        e.left();
        assert_eq!(e.cursor(), (0, 1));
        e.right();
        assert_eq!(e.cursor(), (1, 0));
        e.finish();
        e.right();
        assert_eq!(e.cursor(), (1, 1));
    }

    #[test]
    fn up_and_down_remember_the_wanted_column() {
        let mut e = typed("long line\nx\nanother one");
        e.up();
        assert_eq!(e.cursor(), (1, 1));
        e.up();
        assert_eq!(e.cursor(), (0, 9), "back out to the remembered column");
        e.up();
        assert_eq!(e.cursor(), (0, 0), "up on the first line goes home");
        e.finish();
        e.down();
        assert_eq!(e.cursor(), (2, 11));
    }

    #[test]
    fn home_end_start_and_finish() {
        let mut e = typed("one\ntwo");
        e.home();
        assert_eq!(e.cursor(), (1, 0));
        e.end();
        assert_eq!(e.cursor(), (1, 3));
        e.start();
        assert_eq!(e.cursor(), (0, 0));
        e.finish();
        assert_eq!(e.cursor(), (1, 3));
    }

    #[test]
    fn word_jumps_skip_punctuation_and_cross_lines() {
        let mut e = typed("foo, bar_baz\nqux");
        e.word_left();
        assert_eq!(e.cursor(), (1, 0));
        e.word_left();
        assert_eq!(e.cursor(), (0, 5), "to the start of bar_baz");
        e.word_left();
        assert_eq!(e.cursor(), (0, 0));
        e.word_right();
        assert_eq!(e.cursor(), (0, 3));
        e.word_right();
        assert_eq!(e.cursor(), (0, 12));
        e.word_right();
        assert_eq!(e.cursor(), (1, 3));
    }

    #[test]
    fn paste_normalises_line_endings_and_drops_control_characters() {
        let mut e = typed("a");
        e.insert_str("b\r\nc\rd\x1b[31me\tf");
        assert_eq!(e.text(), "ab\nc\nd[31me\tf");
        assert_eq!(e.cursor(), (2, 8));
    }

    #[test]
    fn paste_in_the_middle_keeps_the_tail_after_the_cursor() {
        let mut e = typed("hello world");
        for _ in 0..5 {
            e.left();
        }
        e.insert_str("big\nnew ");
        assert_eq!(e.lines(), ["hello big", "new world"]);
        assert_eq!(e.cursor(), (1, 4));
    }

    #[test]
    fn delete_word_removes_back_to_the_word_start() {
        let mut e = typed("one two three");
        e.delete_word();
        assert_eq!(e.text(), "one two ");
        e.delete_word();
        assert_eq!(e.text(), "one ");
        let mut e = typed("a\nb");
        e.home();
        e.delete_word();
        assert_eq!(e.text(), "b");
    }

    #[test]
    fn blank_means_only_whitespace() {
        assert!(Editor::new().is_blank());
        assert!(typed(" \n\t").is_blank());
        assert!(!typed("x").is_blank());
    }

    #[test]
    fn with_text_puts_the_cursor_at_the_end() {
        let e = Editor::with_text("```suggestion\nfn x()\n```");
        assert_eq!(e.cursor(), (2, 3));
        assert_eq!(e.line_count(), 3);
    }
}
