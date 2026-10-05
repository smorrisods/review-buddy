/// Expands tabs to the next multiple of `tab_width` columns. A width of zero is treated as one.
pub fn expand_tabs(text: &str, tab_width: u8) -> String {
    if !text.contains('\t') {
        return text.to_string();
    }
    let width = usize::from(tab_width.max(1));
    let mut out = String::with_capacity(text.len() + 8);
    let mut col = 0usize;
    for ch in text.chars() {
        if ch == '\t' {
            let n = width - col % width;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_to_next_stop() {
        assert_eq!(expand_tabs("\tx", 4), "    x");
        assert_eq!(expand_tabs("ab\tx", 4), "ab  x");
        assert_eq!(expand_tabs("abcd\tx", 4), "abcd    x");
    }

    #[test]
    fn zero_width_is_one_and_plain_text_is_untouched() {
        assert_eq!(expand_tabs("a\tb", 0), "a b");
        assert_eq!(expand_tabs("plain", 8), "plain");
    }
}
