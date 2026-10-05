use rb_diff::{parse_patch, Highlighter, LineId, Span};
use rb_theme::{SyntaxRole, Theme};

const RUST: &str = include_str!("fixtures/menus.patch");

fn join(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

fn role_of(spans: &[Span], text: &str) -> Option<Option<SyntaxRole>> {
    spans.iter().find(|s| s.text.trim() == text).map(|s| s.role)
}

#[test]
fn rust_patch_gets_roles_and_keeps_text() {
    let mut hl = Highlighter::new();
    let theme = Theme::default();
    let patch = parse_patch(RUST);
    let out = hl.highlight("src/menus.rs", &patch, &theme, 4);
    for (id, line) in patch.iter() {
        assert_eq!(join(out.line(id).unwrap()), line.text);
    }
    let sig = patch.find_by_new(42).unwrap();
    assert_eq!(
        patch.line(sig).unwrap().text.trim(),
        "/// Moves the cursor, clamping to the last item."
    );
    assert_eq!(
        role_of(
            out.line(sig).unwrap(),
            "/// Moves the cursor, clamping to the last item."
        ),
        Some(Some(SyntaxRole::Comment))
    );
    let select = out.line(patch.find_by_new(43).unwrap()).unwrap();
    assert_eq!(role_of(select, "pub"), Some(Some(SyntaxRole::Keyword)));
    assert_eq!(role_of(select, "select"), Some(Some(SyntaxRole::Function)));
    let title = out.line(patch.find_by_new(50).unwrap()).unwrap();
    assert!(title.iter().any(|s| s.role == Some(SyntaxRole::String)));
}

#[test]
fn language_detection() {
    let hl = Highlighter::new();
    assert_eq!(hl.language("crates/x/src/menus.rs"), Some("Rust"));
    assert_eq!(hl.language("app/index.TSX"), Some("JavaScript"));
    assert_eq!(hl.language("Makefile"), Some("Makefile"));
    assert_eq!(hl.language("notes.unknownext"), None);
    assert_eq!(hl.language("LICENSE"), None);
}

#[test]
fn unknown_language_is_plain_and_tabs_expand() {
    let mut hl = Highlighter::new();
    let patch = parse_patch("@@ -1 +1 @@\n-\ta\n+\tb\tc\n");
    let out = hl.highlight("data.unknownext", &patch, &Theme::default(), 4);
    let spans = out.line(LineId { hunk: 0, line: 1 }).unwrap();
    assert_eq!(
        spans,
        [Span {
            text: "    b   c".into(),
            role: None
        }]
    );
}

#[test]
fn state_carries_across_nearby_hunks_but_not_far_ones() {
    let near = "@@ -1,2 +1,2 @@\n /* start\n-a\n+b\n@@ -5,2 +5,2 @@\n still comment\n-c\n+d\n";
    let far = near.replace("@@ -5,2 +5,2 @@", "@@ -500,2 +500,2 @@");
    let mut hl = Highlighter::new();
    let theme = Theme::default();
    let id = LineId { hunk: 1, line: 0 };
    let carried = hl.highlight("a.c", &parse_patch(near), &theme, 4);
    assert_eq!(carried.line(id).unwrap()[0].role, Some(SyntaxRole::Comment));
    let reset = hl.highlight("b.c", &parse_patch(&far), &theme, 4);
    assert_ne!(reset.line(id).unwrap()[0].role, Some(SyntaxRole::Comment));
}

#[test]
fn cache_is_keyed_by_file_theme_and_tab_width() {
    let mut hl = Highlighter::new();
    let patch = parse_patch(RUST);
    let liminal = Theme::default();
    let dusk = Theme::builtin("dusk").unwrap();
    let a = hl.highlight("a.rs", &patch, &liminal, 4);
    let b = hl.highlight("a.rs", &patch, &liminal, 4);
    assert!(std::sync::Arc::ptr_eq(&a, &b));
    hl.highlight("a.rs", &patch, &dusk, 4);
    hl.highlight("a.rs", &patch, &liminal, 8);
    hl.highlight("b.rs", &patch, &liminal, 4);
    assert_eq!(hl.cached_entries(), 4);
    hl.invalidate_file("a.rs");
    assert_eq!(hl.cached_entries(), 1);
}

#[test]
fn syntect_theme_follows_the_app_theme() {
    use rb_theme::{ColourDepth, Palette};
    let mut hl = Highlighter::new();
    let patch = parse_patch(RUST);
    for id in ["liminal-hq", "afterglow-light"] {
        let theme = Theme::builtin(id).unwrap();
        let out = hl.highlight("m.rs", &patch, &theme, 4);
        let select = out.line(patch.find_by_new(43).unwrap()).unwrap();
        let kw = select
            .iter()
            .find(|s| s.role == Some(SyntaxRole::Keyword))
            .unwrap();
        let palette = Palette::new(theme.clone(), ColourDepth::TrueColour, false);
        assert_eq!(
            kw.style(&palette).fg,
            Some(theme.syntax(SyntaxRole::Keyword))
        );
    }
}
