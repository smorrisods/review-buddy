//! Patch parsing, the hunk model, and syntax highlighting for the diff screen.
//!
//! Forge APIs return one unified-diff body per file. [`FileDiff::from_file_patch`] turns a
//! [`rb_core::FilePatch`] into a [`ParsedPatch`] of hunks and lines carrying old/new line
//! numbers, or into an explicit fallback ([`DiffBody`]) for binary, oversized, or empty files.
//!
//! Comments anchor to a [`LineId`] (stable within one parsed patch), which maps to a
//! [`Anchor`] (side plus line number) and to a GitHub-style [`DiffLine::position`].
//!
//! [`Highlighter`] produces framework-neutral [`Span`]s per line. Each span names a
//! [`rb_theme::SyntaxRole`], and the UI resolves it through its `Palette`, so colour depth and
//! `NO_COLOR` are honoured in one place.
//!
//! # Side-by-side extension point
//!
//! Side-by-side is not implemented in 0.1. It will be a pure function over a [`Hunk`] that
//! walks `lines` and pairs each run of consecutive `Removed` lines with the run of `Added`
//! lines that directly follows it (leaving the shorter side blank), while `Context` lines pair
//! with themselves. Because the model already keeps `old_no`/`new_no` on every line and
//! highlighting is keyed by [`LineId`], the pairing can live in a new module without changing
//! the parser, anchors, or highlight cache.

mod highlight;
mod model;
mod parse;
mod tabs;

pub use highlight::{
    HighlightedFile, Highlighter, Span, MAX_CARRY_GAP, MAX_HIGHLIGHT_LINES, MAX_HIGHLIGHT_LINE_LEN,
};
pub use model::{
    adjacent_file, Anchor, DiffBody, DiffLine, FallbackReason, FileDiff, Hunk, HunkHeader, LineId,
    LineKind, ParsedPatch, LAZY_LINE_THRESHOLD, MAX_PATCH_BYTES,
};
pub use parse::parse_patch;
pub use tabs::expand_tabs;
