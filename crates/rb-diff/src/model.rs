use rb_core::{FilePatch, FileStatus, Side};

use crate::parse::parse_patch;

/// Patches larger than this (1 MB) are not parsed; the diff shows a fallback instead.
pub const MAX_PATCH_BYTES: usize = 1024 * 1024;

/// Patches with more lines than this should be rendered lazily by hunk.
pub const LAZY_LINE_THRESHOLD: usize = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// A hunk header: `@@ -old_start,old_len +new_start,new_len @@ section`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkHeader {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// Text after the closing `@@` (usually the enclosing function), trimmed. May be empty.
    pub section: String,
}

impl HunkHeader {
    /// The header as it appears in a patch, e.g. `@@ -1,3 +1,4 @@ fn main()`.
    pub fn render(&self) -> String {
        let mut out = format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_len, self.new_start, self.new_len
        );
        if !self.section.is_empty() {
            out.push(' ');
            out.push_str(&self.section);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line number in the old file; `None` for added lines.
    pub old_no: Option<u32>,
    /// Line number in the new file; `None` for removed lines.
    pub new_no: Option<u32>,
    /// Content without the leading `+`/`-`/space and without a trailing `\r`.
    pub text: String,
    /// Followed by `\ No newline at end of file`.
    pub no_newline: bool,
    /// GitHub-style position: 1-based, counted from the line after the first `@@` header,
    /// with later hunk headers and `\ No newline` markers each occupying one position.
    pub position: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: HunkHeader,
    /// Position of the hunk header itself (0 for the first hunk, which is never addressable).
    pub header_position: u32,
    pub lines: Vec<DiffLine>,
}

/// Identifies a line within one [`ParsedPatch`]: stable for as long as that patch is kept, and
/// ordered in document order. Not stable across re-fetches; persist an [`Anchor`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LineId {
    pub hunk: u32,
    pub line: u32,
}

/// Where a comment attaches: a side of the diff plus a line number on that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Anchor {
    pub side: Side,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedPatch {
    pub hunks: Vec<Hunk>,
}

impl ParsedPatch {
    pub fn hunk(&self, index: usize) -> Option<&Hunk> {
        self.hunks.get(index)
    }

    pub fn line(&self, id: LineId) -> Option<&DiffLine> {
        self.hunks
            .get(id.hunk as usize)?
            .lines
            .get(id.line as usize)
    }

    /// Number of diff lines, excluding hunk headers.
    pub fn line_count(&self) -> usize {
        self.hunks.iter().map(|h| h.lines.len()).sum()
    }

    /// Whether the diff is big enough that the UI should render lazily by hunk.
    pub fn is_large(&self) -> bool {
        self.line_count() > LAZY_LINE_THRESHOLD
    }

    pub fn adds(&self) -> usize {
        self.count(LineKind::Added)
    }

    pub fn dels(&self) -> usize {
        self.count(LineKind::Removed)
    }

    fn count(&self, kind: LineKind) -> usize {
        self.iter().filter(|(_, l)| l.kind == kind).count()
    }

    /// Every line in document order with its id.
    pub fn iter(&self) -> impl Iterator<Item = (LineId, &DiffLine)> {
        self.hunks.iter().enumerate().flat_map(|(h, hunk)| {
            hunk.lines.iter().enumerate().map(move |(l, line)| {
                (
                    LineId {
                        hunk: h as u32,
                        line: l as u32,
                    },
                    line,
                )
            })
        })
    }

    /// The comment anchor for a line: removed lines anchor to the old side, everything else to
    /// the new side.
    pub fn anchor(&self, id: LineId) -> Option<Anchor> {
        let line = self.line(id)?;
        match (line.kind, line.old_no, line.new_no) {
            (LineKind::Removed, Some(n), _) => Some(Anchor {
                side: Side::Old,
                line: n,
            }),
            (_, _, Some(n)) => Some(Anchor {
                side: Side::New,
                line: n,
            }),
            _ => None,
        }
    }

    pub fn find_by_anchor(&self, anchor: Anchor) -> Option<LineId> {
        match anchor.side {
            Side::New => self.find_by_new(anchor.line),
            Side::Old => self.find_by_old(anchor.line),
        }
    }

    /// The line carrying this new-file line number (an added or context line).
    pub fn find_by_new(&self, n: u32) -> Option<LineId> {
        self.iter()
            .find(|(_, l)| l.new_no == Some(n))
            .map(|(id, _)| id)
    }

    /// The line carrying this old-file line number (a removed or context line).
    pub fn find_by_old(&self, n: u32) -> Option<LineId> {
        self.iter()
            .find(|(_, l)| l.old_no == Some(n))
            .map(|(id, _)| id)
    }

    /// The line at a GitHub-style diff position.
    pub fn find_by_position(&self, position: u32) -> Option<LineId> {
        self.iter()
            .find(|(_, l)| l.position == position)
            .map(|(id, _)| id)
    }

    /// The first line of every non-empty hunk.
    pub fn hunk_starts(&self) -> Vec<LineId> {
        self.hunks
            .iter()
            .enumerate()
            .filter(|(_, h)| !h.lines.is_empty())
            .map(|(i, _)| LineId {
                hunk: i as u32,
                line: 0,
            })
            .collect()
    }

    /// The start of the next hunk after `from`, or the first hunk when `from` is `None`.
    pub fn next_hunk_start(&self, from: Option<LineId>) -> Option<LineId> {
        self.hunk_starts()
            .into_iter()
            .find(|s| from.is_none_or(|f| s.hunk > f.hunk))
    }

    /// The start of the current hunk when `from` is inside it, otherwise of the previous one.
    pub fn prev_hunk_start(&self, from: LineId) -> Option<LineId> {
        self.hunk_starts()
            .into_iter()
            .rev()
            .find(|s| (s.hunk == from.hunk && from.line > 0) || s.hunk < from.hunk)
    }
}

/// The index of the file after (`forward`) or before `current`, or `None` at either end.
pub fn adjacent_file(current: usize, len: usize, forward: bool) -> Option<usize> {
    if forward {
        (current + 1 < len).then_some(current + 1)
    } else {
        current.checked_sub(1).filter(|_| current < len)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// The forge sent no patch: the file is binary or beyond its size limit.
    Unavailable,
    Binary,
    TooLarge,
    /// A text patch with no hunks, such as a pure rename or a mode change.
    NoTextChanges,
}

impl FallbackReason {
    /// Calm one-line copy for the diff pane.
    pub fn message(self) -> &'static str {
        match self {
            FallbackReason::Unavailable | FallbackReason::Binary | FallbackReason::TooLarge => {
                "binary or very large · o to open in browser"
            }
            FallbackReason::NoTextChanges => "no line changes in this file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffBody {
    Text(ParsedPatch),
    Fallback(FallbackReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub body: DiffBody,
}

impl FileDiff {
    pub fn from_file_patch(file: &FilePatch) -> Self {
        let body = match file.patch.as_deref() {
            None => DiffBody::Fallback(FallbackReason::Unavailable),
            Some(p) if p.len() > MAX_PATCH_BYTES => DiffBody::Fallback(FallbackReason::TooLarge),
            Some(p) => {
                let parsed = parse_patch(p);
                if !parsed.hunks.is_empty() {
                    DiffBody::Text(parsed)
                } else if is_binary_patch(p) {
                    DiffBody::Fallback(FallbackReason::Binary)
                } else {
                    DiffBody::Fallback(FallbackReason::NoTextChanges)
                }
            }
        };
        FileDiff {
            path: file.path.clone(),
            old_path: file.old_path.clone(),
            status: file.status,
            body,
        }
    }

    pub fn parsed(&self) -> Option<&ParsedPatch> {
        match &self.body {
            DiffBody::Text(p) => Some(p),
            DiffBody::Fallback(_) => None,
        }
    }
}

fn is_binary_patch(patch: &str) -> bool {
    patch.lines().any(|l| {
        l.starts_with("GIT binary patch")
            || (l.starts_with("Binary files ") && l.ends_with(" differ"))
    })
}
