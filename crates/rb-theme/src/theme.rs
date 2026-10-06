use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use crate::colour::{AnsiColour, Colour, ColourSpec, Rgb};

pub const DEFAULT_THEME_ID: &str = "liminal-hq";
pub const BUILTIN_IDS: [&str; 4] = ["liminal-hq", "dusk", "afterglow-dark", "afterglow-light"];

const DARK_FALLBACK_BG: Rgb = Rgb::new(0x05, 0x05, 0x07);
const LIGHT_FALLBACK_BG: Rgb = Rgb::new(0xfb, 0xfa, 0xf6);

fn builtin_source(id: &str) -> Option<&'static str> {
    Some(match id {
        "liminal-hq" => include_str!("../../../themes/liminal-hq.toml"),
        "dusk" => include_str!("../../../themes/dusk.toml"),
        "afterglow-dark" => include_str!("../../../themes/afterglow-dark.toml"),
        "afterglow-light" => include_str!("../../../themes/afterglow-light.toml"),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

macro_rules! names_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($var),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$var),+];

            pub fn key(self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }

            pub fn from_key(key: &str) -> Option<Self> {
                match key { $($s => Some($name::$var),)+ _ => None }
            }
        }
    };
}

names_enum! {
    /// A colour role. Widgets ask for roles, never raw colours.
    Role {
        Background => "background",
        Raised => "raised",
        Selection => "selection",
        Line => "line",
        Text => "text",
        TextBright => "text_bright",
        TextSecondary => "text_secondary",
        Muted => "muted",
        Accent => "accent",
        Interactive => "interactive",
        Cyan => "cyan",
        Success => "success",
        Warning => "warning",
        Danger => "danger",
        AddedBg => "added_bg",
        RemovedBg => "removed_bg",
        Github => "github",
        Gitlab => "gitlab",
        JaxBody => "jax_body",
        JaxBox => "jax_box",
    }
}

names_enum! {
    /// A syntax-highlighting role from the `[syntax]` table.
    SyntaxRole {
        Keyword => "keyword",
        String => "string",
        Number => "number",
        Type => "type",
        Function => "function",
        Comment => "comment",
        Punctuation => "punctuation",
        Macro => "macro",
    }
}

impl SyntaxRole {
    /// The role used when a theme omits this syntax key.
    pub fn fallback(self) -> Role {
        match self {
            SyntaxRole::Keyword => Role::Interactive,
            SyntaxRole::String => Role::Success,
            SyntaxRole::Type => Role::Cyan,
            SyntaxRole::Comment => Role::Muted,
            _ => Role::Text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    NotFound(String),
    Cycle(String),
    Parse { id: String, message: String },
    Invalid { id: String, message: String },
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::NotFound(id) => write!(f, "no theme named '{id}' was found"),
            ThemeError::Cycle(id) => write!(f, "theme '{id}' extends itself; remove the loop"),
            ThemeError::Parse { id, message } => {
                write!(f, "theme '{id}' isn't valid TOML: {message}")
            }
            ThemeError::Invalid { id, message } => write!(f, "theme '{id}': {message}"),
        }
    }
}

impl std::error::Error for ThemeError {}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, Default)]
struct RawMeta {
    name: Option<String>,
    extends: Option<String>,
    appearance: Option<Appearance>,
    paint_background: Option<bool>,
}

#[derive(Deserialize, Default)]
struct RawTheme {
    #[serde(default)]
    theme: RawMeta,
    #[serde(default)]
    colours: BTreeMap<String, RawValue>,
    #[serde(default)]
    syntax: BTreeMap<String, String>,
    #[serde(default)]
    ansi: BTreeMap<String, String>,
}

/// A fully resolved theme: every role has a concrete, alpha-blended colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub appearance: Appearance,
    paint_background: Option<bool>,
    roles: BTreeMap<Role, Colour>,
    syntax: BTreeMap<SyntaxRole, Colour>,
    wordmark: Vec<Colour>,
    ansi: BTreeMap<Role, AnsiColour>,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::builtin(DEFAULT_THEME_ID).expect("the default theme is embedded and valid")
    }
}

impl Theme {
    /// Loads an embedded theme by id.
    pub fn builtin(id: &str) -> Result<Theme, ThemeError> {
        Theme::resolve(id, &|_| None)
    }

    /// Parses a theme from TOML source, resolving `extends` through `lookup` then the built-ins.
    pub fn from_toml(
        id: &str,
        source: &str,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Theme, ThemeError> {
        let mut chain = vec![parse_raw(id, source)?];
        let mut seen = vec![id.to_string()];
        while let Some(parent) = next_parent(&chain, &seen) {
            if seen.contains(&parent) {
                return Err(ThemeError::Cycle(parent));
            }
            let src = lookup(&parent)
                .or_else(|| builtin_source(&parent).map(str::to_string))
                .ok_or_else(|| ThemeError::NotFound(parent.clone()))?;
            chain.push(parse_raw(&parent, &src)?);
            seen.push(parent);
        }
        build(id, chain)
    }

    /// Resolves a theme by id. `lookup` supplies user-installed theme source first
    /// (config home, data dirs); embedded themes are the last resort.
    pub fn resolve(id: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Theme, ThemeError> {
        let src = lookup(id)
            .or_else(|| builtin_source(id).map(str::to_string))
            .ok_or_else(|| ThemeError::NotFound(id.to_string()))?;
        Theme::from_toml(id, &src, lookup)
    }

    pub fn colour(&self, role: Role) -> Colour {
        self.roles[&role]
    }

    pub fn syntax(&self, role: SyntaxRole) -> Colour {
        self.syntax
            .get(&role)
            .copied()
            .unwrap_or_else(|| self.colour(role.fallback()))
    }

    /// The `wordmark` gradient stops (two to four colours).
    pub fn wordmark(&self) -> &[Colour] {
        &self.wordmark
    }

    /// The `[ansi]` override for a role, if the theme sets one.
    pub fn ansi_override(&self, role: Role) -> Option<AnsiColour> {
        self.ansi.get(&role).copied()
    }

    /// Whether this theme paints its background when the setting is `theme`: the file's
    /// `[theme] paint_background`, or else whether `background` is an opaque colour.
    pub fn paints_background(&self) -> bool {
        self.paint_background
            .unwrap_or_else(|| matches!(self.colour(Role::Background), Colour::Rgb(_)))
    }

    /// The background to measure contrast against: the theme's own, or a dark/light
    /// stand-in when the background is transparent.
    pub fn effective_background(&self) -> Rgb {
        match self.colour(Role::Background) {
            Colour::Rgb(rgb) => rgb,
            _ => fallback_bg(self.appearance),
        }
    }
}

fn fallback_bg(appearance: Appearance) -> Rgb {
    match appearance {
        Appearance::Dark => DARK_FALLBACK_BG,
        Appearance::Light => LIGHT_FALLBACK_BG,
    }
}

fn parse_raw(id: &str, source: &str) -> Result<RawTheme, ThemeError> {
    toml::from_str(source).map_err(|e| ThemeError::Parse {
        id: id.to_string(),
        message: e.message().to_string(),
    })
}

/// The parent of the last theme in the chain; the default theme is the implicit
/// parent of everything except itself.
fn next_parent(chain: &[RawTheme], seen: &[String]) -> Option<String> {
    let last = chain.last()?;
    if let Some(parent) = &last.theme.extends {
        return Some(parent.clone());
    }
    let last_id = seen.last()?;
    (last_id != DEFAULT_THEME_ID).then(|| DEFAULT_THEME_ID.to_string())
}

fn build(id: &str, chain: Vec<RawTheme>) -> Result<Theme, ThemeError> {
    let invalid = |message: String| ThemeError::Invalid {
        id: id.to_string(),
        message,
    };

    let mut name = None;
    let mut appearance = None;
    let mut paint_background = None;
    let mut colours: BTreeMap<String, RawValue> = BTreeMap::new();
    let mut syntax = BTreeMap::new();
    let mut ansi = BTreeMap::new();
    // The chain runs child → ancestors; the child's values win.
    for raw in chain {
        if name.is_none() {
            name = Some(raw.theme.name.unwrap_or_else(|| id.to_string()));
        }
        appearance = appearance.or(raw.theme.appearance);
        paint_background = paint_background.or(raw.theme.paint_background);
        for (k, v) in raw.colours {
            colours.entry(k).or_insert(v);
        }
        for (k, v) in raw.syntax {
            syntax.entry(k).or_insert(v);
        }
        for (k, v) in raw.ansi {
            ansi.entry(k).or_insert(v);
        }
    }
    let appearance = appearance.unwrap_or_default();

    let parse = |key: &str, value: &str| -> Result<ColourSpec, ThemeError> {
        value
            .parse::<ColourSpec>()
            .map_err(|e| invalid(format!("{key}: {e}")))
    };

    for key in colours.keys() {
        if key != "wordmark" && Role::from_key(key).is_none() {
            return Err(invalid(format!("unknown colour role '{key}'")));
        }
    }
    for key in syntax.keys() {
        if SyntaxRole::from_key(key).is_none() {
            return Err(invalid(format!("unknown syntax role '{key}'")));
        }
    }

    let single = |key: &str| -> Result<ColourSpec, ThemeError> {
        match colours.get(key) {
            Some(RawValue::One(s)) => parse(key, s),
            Some(RawValue::Many(_)) => Err(invalid(format!("{key} must be a single colour"))),
            None => Err(invalid(format!("missing colour role '{key}'"))),
        }
    };

    let fallback = fallback_bg(appearance);
    let background = flatten(single("background")?, fallback);
    let raised = flatten(single("raised")?, solid_rgb(background).unwrap_or(fallback));
    // Alpha blends against the background, or `raised` when the background is transparent.
    let base = solid_rgb(background)
        .or_else(|| solid_rgb(raised))
        .unwrap_or(fallback);

    let mut roles = BTreeMap::new();
    roles.insert(Role::Background, background);
    roles.insert(Role::Raised, raised);
    for role in Role::ALL {
        if !roles.contains_key(role) {
            roles.insert(*role, flatten(single(role.key())?, base));
        }
    }

    let mut syntax_out = BTreeMap::new();
    for (key, value) in &syntax {
        if let Some(role) = SyntaxRole::from_key(key) {
            syntax_out.insert(role, flatten(parse(key, value)?, base));
        }
    }

    let wordmark = match colours.get("wordmark") {
        Some(RawValue::Many(list)) if (2..=4).contains(&list.len()) => list
            .iter()
            .map(|s| parse("wordmark", s).map(|c| flatten(c, base)))
            .collect::<Result<Vec<_>, _>>()?,
        Some(RawValue::One(s)) => vec![flatten(parse("wordmark", s)?, base)],
        Some(RawValue::Many(_)) => {
            return Err(invalid("wordmark needs two to four colours".into()))
        }
        None => return Err(invalid("missing colour role 'wordmark'".into())),
    };

    let mut ansi_out = BTreeMap::new();
    for (key, value) in &ansi {
        let role = Role::from_key(key)
            .ok_or_else(|| invalid(format!("unknown role '{key}' in [ansi]")))?;
        let colour = value
            .parse::<AnsiColour>()
            .map_err(|e| invalid(format!("[ansi] {key}: {e}")))?;
        ansi_out.insert(role, colour);
    }

    Ok(Theme {
        id: id.to_string(),
        name: name.unwrap_or_else(|| id.to_string()),
        appearance,
        paint_background,
        roles,
        syntax: syntax_out,
        wordmark,
        ansi: ansi_out,
    })
}

fn solid_rgb(c: Colour) -> Option<Rgb> {
    match c {
        Colour::Rgb(rgb) => Some(rgb),
        _ => None,
    }
}

fn flatten(spec: ColourSpec, base: Rgb) -> Colour {
    match spec {
        ColourSpec::Solid(c) => c,
        ColourSpec::Rgba(rgb, a) => Colour::Rgb(rgb.blend_over(base, a)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_resolves_all_roles() {
        for id in BUILTIN_IDS {
            let theme = Theme::builtin(id).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(theme.id, id);
            for role in Role::ALL {
                let _ = theme.colour(*role);
            }
            for role in SyntaxRole::ALL {
                let _ = theme.syntax(*role);
            }
            assert!((2..=4).contains(&theme.wordmark().len()), "{id}");
        }
    }

    #[test]
    fn liminal_is_default_and_transparent() {
        let t = Theme::default();
        assert_eq!(t.id, "liminal-hq");
        assert_eq!(t.colour(Role::Background), Colour::Reset);
        assert_eq!(t.colour(Role::Accent), Colour::rgb(0xff, 0xaa, 0x40));
        assert_eq!(t.effective_background(), Rgb::new(5, 5, 7));
    }

    #[test]
    fn light_theme_is_light() {
        let t = Theme::builtin("afterglow-light").unwrap();
        assert_eq!(t.appearance, Appearance::Light);
        assert_eq!(t.colour(Role::Background), Colour::rgb(0xfb, 0xfa, 0xf6));
    }

    #[test]
    fn alpha_blends_against_raised_when_background_transparent() {
        let t = Theme::default();
        let expected = Rgb::new(0xff, 0xaa, 0x40).blend_over(Rgb::new(0x11, 0x11, 0x16), 0x1f);
        assert_eq!(t.colour(Role::Selection), Colour::Rgb(expected));
    }

    #[test]
    fn alpha_blends_against_background_when_opaque() {
        let t = Theme::builtin("afterglow-light").unwrap();
        let expected = Rgb::new(0xb4, 0x53, 0x09).blend_over(Rgb::new(0xfb, 0xfa, 0xf6), 0x1a);
        assert_eq!(t.colour(Role::Selection), Colour::Rgb(expected));
    }

    #[test]
    fn extends_inherits_and_overrides() {
        let child = "[theme]\nname = \"Harbour\"\nextends = \"dusk\"\n[colours]\naccent = \"#112233\"\n[syntax]\nkeyword = \"#445566\"\n";
        let t = Theme::from_toml("harbour", child, &|_| None).unwrap();
        let dusk = Theme::builtin("dusk").unwrap();
        assert_eq!(t.name, "Harbour");
        assert_eq!(t.colour(Role::Accent), Colour::rgb(0x11, 0x22, 0x33));
        assert_eq!(t.colour(Role::Danger), dusk.colour(Role::Danger));
        assert_eq!(t.wordmark(), dusk.wordmark());
        assert_eq!(t.syntax(SyntaxRole::Keyword), Colour::rgb(0x44, 0x55, 0x66));
        assert_eq!(
            t.syntax(SyntaxRole::String),
            dusk.syntax(SyntaxRole::String)
        );
    }

    #[test]
    fn extends_defaults_to_liminal_hq() {
        let t = Theme::from_toml("tiny", "[colours]\naccent = \"#010203\"\n", &|_| None).unwrap();
        let base = Theme::default();
        assert_eq!(t.colour(Role::Accent), Colour::rgb(1, 2, 3));
        assert_eq!(t.colour(Role::Text), base.colour(Role::Text));
        assert_eq!(t.name, "tiny");
    }

    #[test]
    fn user_themes_chain_and_shadow() {
        let lookup = |id: &str| match id {
            "a" => Some("[theme]\nextends = \"b\"\n[colours]\ntext = \"#000001\"\n".to_string()),
            "b" => Some("[colours]\nmuted = \"#000002\"\n".to_string()),
            _ => None,
        };
        let t = Theme::resolve("a", &lookup).unwrap();
        assert_eq!(t.colour(Role::Text), Colour::rgb(0, 0, 1));
        assert_eq!(t.colour(Role::Muted), Colour::rgb(0, 0, 2));
        assert_eq!(
            t.colour(Role::Accent),
            Theme::default().colour(Role::Accent)
        );

        let shadow =
            |id: &str| (id == "dusk").then(|| "[colours]\ntext = \"#0a0b0c\"\n".to_string());
        let t = Theme::resolve("dusk", &shadow).unwrap();
        assert_eq!(t.colour(Role::Text), Colour::rgb(10, 11, 12));
    }

    #[test]
    fn errors_are_reported() {
        let none = |_: &str| None;
        assert!(matches!(
            Theme::builtin("nope"),
            Err(ThemeError::NotFound(_))
        ));
        let cyc = |id: &str| match id {
            "a" => Some("[theme]\nextends = \"b\"\n".to_string()),
            "b" => Some("[theme]\nextends = \"a\"\n".to_string()),
            _ => None,
        };
        assert!(matches!(
            Theme::resolve("a", &cyc),
            Err(ThemeError::Cycle(_))
        ));
        assert!(matches!(
            Theme::from_toml("x", "[colours", &none),
            Err(ThemeError::Parse { .. })
        ));
        for bad in [
            "[colours]\naccent = \"nope\"\n",
            "[colours]\nbogus = \"#000000\"\n",
            "[colours]\nwordmark = [\"#000000\"]\n",
            "[ansi]\naccent = \"#000000\"\n",
            "[syntax]\nbogus = \"#000000\"\n",
        ] {
            assert!(
                matches!(
                    Theme::from_toml("x", bad, &none),
                    Err(ThemeError::Invalid { .. })
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn paint_background_is_inferred_or_declared() {
        let inferred = |id| Theme::builtin(id).unwrap().paints_background();
        assert!(!inferred("liminal-hq"));
        assert!(!inferred("dusk"));
        assert!(inferred("afterglow-dark"));
        assert!(inferred("afterglow-light"));

        let load = |extra: &str, parent: &str| {
            let src = format!("[theme]\nextends = \"{parent}\"\n{extra}\n");
            Theme::from_toml("mine", &src, &|_| None).unwrap()
        };
        assert!(load("paint_background = true", "dusk").paints_background());
        assert!(!load("paint_background = false", "afterglow-dark").paints_background());
        assert!(load("", "afterglow-dark").paints_background());
    }

    #[test]
    fn paint_background_is_inherited_and_validated() {
        let parent = "[theme]\nextends = \"dusk\"\npaint_background = true\n";
        let lookup = |id: &str| (id == "base").then(|| parent.to_string());
        let child = Theme::from_toml("kid", "[theme]\nextends = \"base\"\n", &lookup).unwrap();
        assert!(child.paints_background());
        let bad = Theme::from_toml(
            "bad",
            "[theme]\npaint_background = \"sometimes\"\n",
            &|_| None,
        );
        assert!(matches!(bad, Err(ThemeError::Parse { .. })));
    }
}
