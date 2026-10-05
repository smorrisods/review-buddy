use crate::model::{DiffLine, Hunk, HunkHeader, LineKind, ParsedPatch};

/// Parses the unified-diff body a forge returns for one file.
///
/// Lines before the first hunk header (`diff --git`, `index`, `---`, `+++`, rename metadata)
/// are skipped. Hunk lengths from the header decide where a hunk ends; anything after that
/// until the next header is ignored. Malformed headers are skipped, and a hunk with no lines
/// is kept. Never panics.
pub fn parse_patch(patch: &str) -> ParsedPatch {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut state: Option<Cursor> = None;
    let mut position = 0u32;

    for raw in patch.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);

        if line.starts_with("@@") {
            state = None;
            if let Some(header) = parse_header(line) {
                let header_position = if hunks.is_empty() {
                    0
                } else {
                    position = position.saturating_add(1);
                    position
                };
                state = Some(Cursor {
                    old: header.old_start,
                    new: header.new_start,
                    old_left: header.old_len,
                    new_left: header.new_len,
                });
                hunks.push(Hunk {
                    header,
                    header_position,
                    lines: Vec::new(),
                });
            }
            continue;
        }

        let (Some(cur), Some(hunk)) = (state.as_mut(), hunks.last_mut()) else {
            continue;
        };

        if line.starts_with('\\') {
            if let Some(last) = hunk.lines.last_mut() {
                last.no_newline = true;
                position = position.saturating_add(1);
            }
            continue;
        }

        if cur.old_left == 0 && cur.new_left == 0 {
            continue;
        }

        let (kind, text) = match line.chars().next() {
            Some('+') => (LineKind::Added, &line[1..]),
            Some('-') => (LineKind::Removed, &line[1..]),
            Some(' ') => (LineKind::Context, &line[1..]),
            // Some APIs strip the lone space from empty context lines.
            None => (LineKind::Context, ""),
            Some(_) => continue,
        };

        let (old_no, new_no) = match kind {
            LineKind::Context => (Some(cur.take_old()), Some(cur.take_new())),
            LineKind::Added => (None, Some(cur.take_new())),
            LineKind::Removed => (Some(cur.take_old()), None),
        };
        position = position.saturating_add(1);
        hunk.lines.push(DiffLine {
            kind,
            old_no,
            new_no,
            text: text.to_string(),
            no_newline: false,
            position,
        });
    }

    ParsedPatch { hunks }
}

struct Cursor {
    old: u32,
    new: u32,
    old_left: u32,
    new_left: u32,
}

impl Cursor {
    fn take_old(&mut self) -> u32 {
        let n = self.old;
        self.old = self.old.saturating_add(1);
        self.old_left = self.old_left.saturating_sub(1);
        n
    }

    fn take_new(&mut self) -> u32 {
        let n = self.new;
        self.new = self.new.saturating_add(1);
        self.new_left = self.new_left.saturating_sub(1);
        n
    }
}

fn parse_header(line: &str) -> Option<HunkHeader> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, section) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let (old_start, old_len) = parse_range(old)?;
    let (new_start, new_len) = parse_range(new)?;
    Some(HunkHeader {
        old_start,
        old_len,
        new_start,
        new_len,
        section: section.trim().to_string(),
    })
}

fn parse_range(s: &str) -> Option<(u32, u32)> {
    match s.split_once(',') {
        Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_forms() {
        let h = parse_header("@@ -10,7 +12,9 @@ impl Foo {").unwrap();
        assert_eq!(
            (h.old_start, h.old_len, h.new_start, h.new_len),
            (10, 7, 12, 9)
        );
        assert_eq!(h.section, "impl Foo {");
        let h = parse_header("@@ -5 +5 @@").unwrap();
        assert_eq!((h.old_len, h.new_len), (1, 1));
        assert_eq!(h.render(), "@@ -5,1 +5,1 @@");
        assert!(parse_header("@@ nonsense @@").is_none());
        assert!(parse_header("@@ -1,2 +x @@").is_none());
    }

    #[test]
    fn numbers_and_positions() {
        let p = parse_patch("@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n@@ -10,2 +10,2 @@\n x\n-y\n+Y\n");
        assert_eq!(p.hunks.len(), 2);
        let pos: Vec<u32> = p.iter().map(|(_, l)| l.position).collect();
        assert_eq!(pos, [1, 2, 3, 4, 6, 7, 8]);
        assert_eq!(p.hunks[1].header_position, 5);
        let nums: Vec<_> = p.iter().map(|(_, l)| (l.old_no, l.new_no)).collect();
        assert_eq!(
            nums[..4],
            [
                (Some(1), Some(1)),
                (Some(2), None),
                (None, Some(2)),
                (Some(3), Some(3))
            ]
        );
    }

    #[test]
    fn crlf_empty_context_and_no_newline_marker() {
        let p = parse_patch(
            "@@ -1,3 +1,3 @@\r\n a\r\n\r\n-b\r\n\\ No newline at end of file\r\n+c\r\n",
        );
        let lines = &p.hunks[0].lines;
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].text, "a");
        assert_eq!(lines[1].kind, LineKind::Context);
        assert_eq!(lines[1].text, "");
        assert!(lines[2].no_newline);
        assert_eq!(lines[3].position, 5);
    }

    #[test]
    fn skips_metadata_and_trailing_junk() {
        let p = parse_patch(
            "diff --git a/x b/y\nrename from x\nrename to y\n--- a/x\n+++ b/y\n@@ -1 +1 @@\n-a\n+b\n--- trailing\n",
        );
        assert_eq!(p.hunks[0].lines.len(), 2);
    }

    #[test]
    fn empty_hunk_and_bad_header() {
        let p = parse_patch("@@ -0,0 +0,0 @@\n@@ bad\n+ignored\n");
        assert_eq!(p.hunks.len(), 1);
        assert!(p.hunks[0].lines.is_empty());
        assert!(parse_patch("").hunks.is_empty());
    }

    #[test]
    fn tabs_are_preserved_raw() {
        let p = parse_patch("@@ -1 +1 @@\n-\tfoo\n+\t\tfoo\n");
        assert_eq!(p.hunks[0].lines[1].text, "\t\tfoo");
    }
}
