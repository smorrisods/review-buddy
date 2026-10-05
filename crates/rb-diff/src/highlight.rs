use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rb_theme::{indexed_rgb, Colour, Palette, Rgb, Style, SyntaxRole, Theme};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme as SynTheme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::model::{LineId, LineKind, ParsedPatch};
use crate::tabs::expand_tabs;

/// Parse state is carried from one hunk into the next only when the unseen gap between them is
/// at most this many lines on that side; across a bigger gap the state is reset.
pub const MAX_CARRY_GAP: u32 = 50;

/// Patches with more lines than this are returned unhighlighted.
pub const MAX_HIGHLIGHT_LINES: usize = 20_000;

/// Lines longer than this (in bytes) are left unhighlighted, which also guards against
/// minified files.
pub const MAX_HIGHLIGHT_LINE_LEN: usize = 2000;

/// Marks the syntect colour of a role: the alpha channel is `ROLE_TAG_BASE + role index`, so a
/// span's role can be recovered without ambiguity when two roles share one RGB value.
const ROLE_TAG_BASE: u8 = 0xE0;
const DEFAULT_ALPHA: u8 = 0xFF;

/// A run of text with one syntax role. `None` means ordinary text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Tab-expanded text.
    pub text: String,
    pub role: Option<SyntaxRole>,
}

impl Span {
    /// The foreground style for this span at the palette's colour depth. Plain text has no
    /// foreground so it inherits the pane's own text colour.
    pub fn style(&self, palette: &Palette) -> Style {
        Style {
            fg: self.role.and_then(|r| palette.syntax(r)),
            ..Style::default()
        }
    }
}

/// Highlighted spans for every line of one file, addressed by [`LineId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedFile {
    hunks: Vec<Vec<Vec<Span>>>,
}

impl HighlightedFile {
    pub fn line(&self, id: LineId) -> Option<&[Span]> {
        self.hunks
            .get(id.hunk as usize)?
            .get(id.line as usize)
            .map(Vec::as_slice)
    }

    fn matches(&self, patch: &ParsedPatch) -> bool {
        self.hunks.len() == patch.hunks.len()
            && self
                .hunks
                .iter()
                .zip(&patch.hunks)
                .all(|(a, b)| a.len() == b.lines.len())
    }
}

type CacheKey = (String, String, u8);

/// Syntax highlighting with a cache keyed by `(file, theme id, tab width)`.
///
/// `file` is the path used both for language detection and as the cache key; callers that keep
/// several revisions of one file should call [`Highlighter::invalidate_file`] when the patch
/// changes (the cache also rejects an entry whose hunk and line counts no longer match).
pub struct Highlighter {
    syntaxes: SyntaxSet,
    cache: HashMap<CacheKey, Arc<HighlightedFile>>,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            cache: HashMap::new(),
        }
    }

    /// The name of the language detected for `path`, if any.
    pub fn language(&self, path: &str) -> Option<&str> {
        self.syntax_for(path).map(|s| s.name.as_str())
    }

    pub fn cached_entries(&self) -> usize {
        self.cache.len()
    }

    pub fn invalidate_file(&mut self, file: &str) {
        self.cache.retain(|(f, _, _), _| f != file);
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn highlight(
        &mut self,
        file: &str,
        patch: &ParsedPatch,
        theme: &Theme,
        tab_width: u8,
    ) -> Arc<HighlightedFile> {
        let key = (file.to_string(), theme.id.clone(), tab_width);
        if let Some(hit) = self.cache.get(&key) {
            if hit.matches(patch) {
                return Arc::clone(hit);
            }
        }
        let syn_theme = syntect_theme(theme);
        let syntax = self.syntax_for(file);
        let done = Arc::new(highlight_patch(
            &self.syntaxes,
            syntax,
            &syn_theme,
            patch,
            tab_width,
        ));
        self.cache.insert(key, Arc::clone(&done));
        done
    }

    fn syntax_for(&self, path: &str) -> Option<&SyntaxReference> {
        let name = Path::new(path).file_name()?.to_str()?;
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        let set = &self.syntaxes;
        let found = set
            .find_syntax_by_extension(name)
            .or_else(|| ext.as_deref().and_then(|e| set.find_syntax_by_extension(e)))
            .or_else(|| {
                ext.as_deref()
                    .and_then(alias_extension)
                    .and_then(|e| set.find_syntax_by_extension(e))
            })?;
        (found.name != "Plain Text").then_some(found)
    }
}

/// Languages the bundled grammars cover under another extension.
fn alias_extension(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "ts" | "tsx" | "jsx" | "mjs" | "cjs" | "mts" | "cts" => "js",
        "jsonc" | "json5" => "json",
        "pyi" => "py",
        "vue" | "svelte" => "html",
        _ => return None,
    })
}

fn role_index(role: SyntaxRole) -> u8 {
    SyntaxRole::ALL
        .iter()
        .position(|r| *r == role)
        .map_or(0, |i| i as u8)
}

fn role_from_alpha(alpha: u8) -> Option<SyntaxRole> {
    let idx = alpha.checked_sub(ROLE_TAG_BASE)?;
    SyntaxRole::ALL.get(usize::from(idx)).copied()
}

fn colour_to_rgb(colour: Colour, fallback: Rgb) -> Rgb {
    match colour {
        Colour::Rgb(rgb) => rgb,
        Colour::Indexed(i) => indexed_rgb(i),
        Colour::Ansi(a) => a.approx_rgb(),
        Colour::Reset => fallback,
    }
}

/// Builds a syntect theme whose colours come from the app theme's syntax roles.
pub(crate) fn syntect_theme(theme: &Theme) -> SynTheme {
    let text = colour_to_rgb(
        theme.colour(rb_theme::Role::Text),
        Rgb::new(0xe0, 0xe0, 0xe0),
    );
    let role_colour = |role: SyntaxRole| {
        let rgb = colour_to_rgb(theme.syntax(role), text);
        Color {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
            a: ROLE_TAG_BASE + role_index(role),
        }
    };
    let rule = |scopes: &str, role: SyntaxRole| ThemeItem {
        scope: scopes
            .parse::<ScopeSelectors>()
            .expect("built-in scope selectors are valid"),
        style: StyleModifier {
            foreground: Some(role_colour(role)),
            background: None,
            font_style: Some(FontStyle::empty()),
        },
    };
    use SyntaxRole::*;
    SynTheme {
        name: Some(format!("review-buddy:{}", theme.id)),
        author: None,
        settings: ThemeSettings {
            foreground: Some(Color {
                r: text.r,
                g: text.g,
                b: text.b,
                a: DEFAULT_ALPHA,
            }),
            ..ThemeSettings::default()
        },
        scopes: vec![
            rule(
                "keyword, storage, constant.language, entity.name.tag",
                Keyword,
            ),
            rule("string, punctuation.definition.string", String),
            rule("constant.numeric", Number),
            rule(
                "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, \
                 entity.name.trait, entity.other.inherited-class, support.type, \
                 support.class, storage.type, entity.other.attribute-name",
                Type,
            ),
            rule(
                "entity.name.function, support.function, variable.function, meta.function-call",
                Function,
            ),
            rule(
                "entity.name.function.macro, support.macro, variable.macro",
                Macro,
            ),
            rule("comment, punctuation.definition.comment", Comment),
            rule(
                "punctuation.separator, punctuation.terminator, punctuation.accessor",
                Punctuation,
            ),
        ],
    }
}

fn highlight_patch(
    syntaxes: &SyntaxSet,
    syntax: Option<&SyntaxReference>,
    theme: &SynTheme,
    patch: &ParsedPatch,
    tab_width: u8,
) -> HighlightedFile {
    let syntax = syntax.filter(|_| patch.line_count() <= MAX_HIGHLIGHT_LINES);
    let mut old_stream: Option<HighlightLines> = None;
    let mut new_stream: Option<HighlightLines> = None;
    let mut prev_end: Option<(u32, u32)> = None;
    let mut hunks = Vec::with_capacity(patch.hunks.len());

    for hunk in &patch.hunks {
        let h = &hunk.header;
        let carry = |end: Option<u32>, start: u32| {
            end.is_some_and(|e| start.saturating_sub(e) <= MAX_CARRY_GAP)
        };
        let fresh = || syntax.map(|s| HighlightLines::new(s, theme));
        if !carry(prev_end.map(|e| e.0), h.old_start) || old_stream.is_none() {
            old_stream = fresh();
        }
        if !carry(prev_end.map(|e| e.1), h.new_start) || new_stream.is_none() {
            new_stream = fresh();
        }
        prev_end = Some((
            h.old_start.saturating_add(h.old_len),
            h.new_start.saturating_add(h.new_len),
        ));

        let mut lines = Vec::with_capacity(hunk.lines.len());
        for line in &hunk.lines {
            let text = expand_tabs(&line.text, tab_width);
            let (use_old, use_new) = match line.kind {
                LineKind::Context => (true, true),
                LineKind::Removed => (true, false),
                LineKind::Added => (false, true),
            };
            let long = text.len() > MAX_HIGHLIGHT_LINE_LEN;
            let from_old = run(&mut old_stream, syntaxes, &text, use_old && !long);
            let from_new = run(&mut new_stream, syntaxes, &text, use_new && !long);
            let spans = match line.kind {
                LineKind::Removed => from_old,
                _ => from_new,
            };
            lines.push(spans.unwrap_or_else(|| plain(text)));
        }
        hunks.push(lines);
    }
    HighlightedFile { hunks }
}

fn plain(text: String) -> Vec<Span> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Span { text, role: None }]
    }
}

fn run(
    stream: &mut Option<HighlightLines>,
    syntaxes: &SyntaxSet,
    text: &str,
    enabled: bool,
) -> Option<Vec<Span>> {
    if !enabled {
        return None;
    }
    let lines = stream.as_mut()?;
    let mut with_nl = String::with_capacity(text.len() + 1);
    with_nl.push_str(text);
    with_nl.push('\n');
    let Ok(ranges) = lines.highlight_line(&with_nl, syntaxes) else {
        *stream = None;
        return None;
    };
    let mut spans: Vec<Span> = Vec::new();
    for (style, seg) in ranges {
        let seg = seg.trim_end_matches('\n');
        if seg.is_empty() {
            continue;
        }
        let role = role_from_alpha(style.foreground.a);
        match spans.last_mut() {
            Some(last) if last.role == role => last.text.push_str(seg),
            _ => spans.push(Span {
                text: seg.to_string(),
                role,
            }),
        }
    }
    Some(spans)
}
