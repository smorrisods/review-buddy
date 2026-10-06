use std::str::FromStr;

use crate::colour::{AnsiColour, Colour, Rgb, TagColour};
use crate::theme::{Role, SyntaxRole, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourDepth {
    TrueColour,
    Ansi256,
    Ansi16,
}

impl ColourDepth {
    /// Detects the depth from the process environment. See [`ColourDepth::detect_with`].
    pub fn from_env() -> Self {
        Self::detect_with(|key| std::env::var(key).ok())
    }

    /// Picks a depth from environment variables read through `get`, so tests can inject them.
    ///
    /// In order: `REVIEW_BUDDY_COLOUR_DEPTH` (`truecolor`, `256` or `16`); `COLORTERM` of
    /// `truecolor` or `24bit`; a `TERM` that names 24-bit colour (`*-direct`, `*truecolor*`,
    /// kitty, alacritty, wezterm, ghostty); then terminals that are known to support 24-bit
    /// colour but don't always set `COLORTERM` (Windows Terminal through `WT_SESSION`, which
    /// WSL passes along, iTerm2, VS Code, WezTerm, Ghostty, kitty, and VTE 0.36 or newer).
    /// Those hints are ignored under tmux or screen, which only pass 24-bit colour through when
    /// told to. After that a `TERM` containing `256color` gives 256 colours, and anything else
    /// gives 16.
    pub fn detect_with(get: impl Fn(&str) -> Option<String>) -> Self {
        let var = |key: &str| get(key).filter(|v| !v.is_empty());
        if let Some(depth) = var("REVIEW_BUDDY_COLOUR_DEPTH").and_then(|v| v.parse().ok()) {
            return depth;
        }
        let colorterm = var("COLORTERM").unwrap_or_default().to_ascii_lowercase();
        if colorterm == "truecolor" || colorterm == "24bit" {
            return ColourDepth::TrueColour;
        }
        let term = var("TERM").unwrap_or_default().to_ascii_lowercase();
        let named_truecolour = term.ends_with("-direct")
            || term.contains("truecolor")
            || term.contains("24bit")
            || ["kitty", "alacritty", "wezterm", "ghostty"]
                .iter()
                .any(|n| term.contains(n));
        if named_truecolour {
            return ColourDepth::TrueColour;
        }
        let multiplexed =
            var("TMUX").is_some() || term.starts_with("screen") || term.starts_with("tmux");
        if !multiplexed {
            let program = var("TERM_PROGRAM").unwrap_or_default();
            let vte_ok = var("VTE_VERSION")
                .and_then(|v| v.parse::<u32>().ok())
                .is_some_and(|v| v >= 3600);
            let known = var("WT_SESSION").is_some()
                || var("KITTY_WINDOW_ID").is_some()
                || ["iTerm.app", "vscode", "WezTerm", "ghostty"].contains(&program.as_str())
                || vte_ok;
            if known {
                return ColourDepth::TrueColour;
            }
        }
        if term.contains("256color") {
            ColourDepth::Ansi256
        } else {
            ColourDepth::Ansi16
        }
    }

    /// Picks a depth from `COLORTERM` and `TERM` values alone.
    pub fn detect(colorterm: Option<&str>, term: Option<&str>) -> Self {
        Self::detect_with(|key| match key {
            "COLORTERM" => colorterm.map(str::to_string),
            "TERM" => term.map(str::to_string),
            _ => None,
        })
    }
}

/// Parses the `ui.colour_depth` config values: `truecolor`, `256`, or `16`.
impl FromStr for ColourDepth {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "truecolor" | "truecolour" | "24bit" => Ok(ColourDepth::TrueColour),
            "256" => Ok(ColourDepth::Ansi256),
            "16" => Ok(ColourDepth::Ansi16),
            other => Err(format!(
                "'{other}' isn't a colour depth; use truecolor, 256, or 16"
            )),
        }
    }
}

/// Whether `NO_COLOR` asks for colourless output (set and non-empty).
pub fn no_color_requested() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub bold: bool,
    pub dim: bool,
    pub reverse: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        bold: false,
        dim: false,
        reverse: false,
    };
    pub const BOLD: Modifiers = Modifiers {
        bold: true,
        ..Modifiers::NONE
    };
    pub const DIM: Modifiers = Modifiers {
        dim: true,
        ..Modifiers::NONE
    };
    pub const REVERSE: Modifiers = Modifiers {
        reverse: true,
        ..Modifiers::NONE
    };
}

/// A framework-neutral style. `None` colours mean "leave the terminal's own colour".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Option<Colour>,
    pub bg: Option<Colour>,
    pub modifiers: Modifiers,
}

/// A theme prepared for a particular terminal: colours quantised to its depth, or
/// collapsed to modifiers when `NO_COLOR` is set.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    theme: Theme,
    depth: ColourDepth,
    no_color: bool,
    background_mode: BackgroundMode,
}

/// Whether the interface paints a background of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackgroundMode {
    /// Follow the theme: paint when it has an opaque background.
    #[default]
    Theme,
    /// Paint, using the theme's effective background when it is transparent.
    Yes,
    /// Never paint.
    No,
}

impl BackgroundMode {
    pub const ALL: [BackgroundMode; 3] = [Self::Theme, Self::Yes, Self::No];

    pub fn key(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Yes => "yes",
            Self::No => "no",
        }
    }

    /// The next mode in the session cycle: theme, yes, no, then theme again.
    pub fn next(self) -> Self {
        match self {
            Self::Theme => Self::Yes,
            Self::Yes => Self::No,
            Self::No => Self::Theme,
        }
    }
}

impl FromStr for BackgroundMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "theme" => Ok(Self::Theme),
            "yes" => Ok(Self::Yes),
            "no" => Ok(Self::No),
            other => Err(format!(
                "'{other}' isn't a background setting; use theme, yes, or no"
            )),
        }
    }
}

/// Where each layer of the background setting stands. Precedence, highest first: the session
/// key, `REVIEW_BUDDY_BACKGROUND`, the per-theme setting, the global setting, then the theme's
/// own default (which is what `theme` means).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BackgroundSettings {
    pub global: BackgroundMode,
    pub per_theme: std::collections::BTreeMap<String, BackgroundMode>,
    pub env: Option<BackgroundMode>,
    pub session: Option<BackgroundMode>,
}

impl BackgroundSettings {
    /// The mode in force for a theme.
    pub fn mode_for(&self, theme_id: &str) -> BackgroundMode {
        self.session
            .or(self.env)
            .or_else(|| self.per_theme.get(theme_id).copied())
            .unwrap_or(self.global)
    }
}

/// The background colour to paint, quantised for `depth`, or `None` to leave the terminal's own.
///
/// `NO_COLOR` never paints. A per-theme setting beats the global one. At 16 colours nothing is
/// painted unless forced with `yes`. `theme` follows the theme (see
/// [`Theme::paints_background`]); `yes` paints a transparent theme with its effective
/// background; `no` never paints.
pub fn resolve_background(
    theme: &Theme,
    global: BackgroundMode,
    per_theme: Option<BackgroundMode>,
    depth: ColourDepth,
    no_color: bool,
) -> Option<Colour> {
    if no_color {
        return None;
    }
    let paint = match per_theme.unwrap_or(global) {
        BackgroundMode::No => false,
        BackgroundMode::Yes => true,
        BackgroundMode::Theme => depth != ColourDepth::Ansi16 && theme.paints_background(),
    };
    paint.then(|| quantise(Colour::Rgb(theme.effective_background()), depth))
}

fn quantise(colour: Colour, depth: ColourDepth) -> Colour {
    match (colour, depth) {
        (Colour::Rgb(rgb), ColourDepth::Ansi256) => Colour::Indexed(quantise_256(rgb)),
        (Colour::Rgb(_) | Colour::Indexed(_), ColourDepth::Ansi16) => {
            Colour::Ansi(nearest_ansi(colour))
        }
        (c, _) => c,
    }
}

impl Palette {
    pub fn new(theme: Theme, depth: ColourDepth, no_color: bool) -> Self {
        Self {
            theme,
            depth,
            no_color,
            background_mode: BackgroundMode::Theme,
        }
    }

    /// Sets the mode (already merged across config, environment and session) that
    /// [`Palette::background`] resolves with.
    pub fn with_background_mode(mut self, mode: BackgroundMode) -> Self {
        self.background_mode = mode;
        self
    }

    pub fn background_mode(&self) -> BackgroundMode {
        self.background_mode
    }

    /// The background to paint across the frame, quantised for this depth, or `None` to leave
    /// the terminal's own.
    pub fn background(&self) -> Option<Colour> {
        resolve_background(
            &self.theme,
            self.background_mode,
            None,
            self.depth,
            self.no_color,
        )
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn depth(&self) -> ColourDepth {
        self.depth
    }

    pub fn no_color(&self) -> bool {
        self.no_color
    }

    /// The colour for a role at this depth, or `None` when it should not be painted
    /// (`NO_COLOR`, or a transparent role).
    pub fn colour(&self, role: Role) -> Option<Colour> {
        if self.no_color {
            return None;
        }
        let colour = self.theme.colour(role);
        if colour == Colour::Reset {
            return None;
        }
        Some(match (self.depth, colour) {
            (ColourDepth::Ansi16, Colour::Rgb(_) | Colour::Indexed(_)) => {
                Colour::Ansi(self.ansi16(role, colour))
            }
            (ColourDepth::Ansi256, Colour::Rgb(rgb)) => Colour::Indexed(quantise_256(rgb)),
            _ => colour,
        })
    }

    pub fn syntax(&self, role: SyntaxRole) -> Option<Colour> {
        if self.no_color {
            return None;
        }
        let colour = self.theme.syntax(role);
        match (colour, self.depth) {
            (Colour::Reset, _) => None,
            (Colour::Rgb(_) | Colour::Indexed(_), ColourDepth::Ansi16) => {
                Some(Colour::Ansi(nearest_ansi(colour)))
            }
            (Colour::Rgb(rgb), ColourDepth::Ansi256) => Some(Colour::Indexed(quantise_256(rgb))),
            _ => Some(colour),
        }
    }

    pub fn wordmark(&self) -> Vec<Colour> {
        if self.no_color {
            return Vec::new();
        }
        self.theme
            .wordmark()
            .iter()
            .filter_map(|c| match (*c, self.depth) {
                (Colour::Reset, _) => None,
                (Colour::Rgb(_) | Colour::Indexed(_), ColourDepth::Ansi16) => {
                    Some(Colour::Ansi(nearest_ansi(*c)))
                }
                (Colour::Rgb(rgb), ColourDepth::Ansi256) => {
                    Some(Colour::Indexed(quantise_256(rgb)))
                }
                (c, _) => Some(c),
            })
            .collect()
    }

    /// The theme's wordmark stops at full precision, for blending. Quantising the stops first
    /// would leave nothing to blend between at 256 colours, so a gradient drawn from
    /// [`Palette::wordmark`] steps between three colours; blend these, then call
    /// [`Palette::at_depth`] on each result. Empty under `NO_COLOR`.
    pub fn wordmark_blend_stops(&self) -> Vec<Colour> {
        if self.no_color {
            return Vec::new();
        }
        self.theme
            .wordmark()
            .iter()
            .copied()
            .filter(|c| !matches!(c, Colour::Reset))
            .collect()
    }

    /// Maps a colour computed at full precision (a blend, say) onto this palette's depth.
    pub fn at_depth(&self, colour: Colour) -> Colour {
        match (colour, self.depth) {
            (Colour::Rgb(rgb), ColourDepth::Ansi256) => Colour::Indexed(quantise_256(rgb)),
            (Colour::Rgb(_) | Colour::Indexed(_), ColourDepth::Ansi16) => {
                Colour::Ansi(nearest_ansi(colour))
            }
            (c, _) => c,
        }
    }

    /// A style using the role as foreground colour.
    pub fn fg(&self, role: Role) -> Style {
        if self.no_color {
            return Style {
                modifiers: no_color_modifiers(role),
                ..Style::default()
            };
        }
        Style {
            fg: self.colour(role),
            ..Style::default()
        }
    }

    /// A style for a source's tag colour: a role resolves as [`Palette::fg`] does, and a
    /// fixed colour is quantised to this depth. `NO_COLOR` leaves it unpainted.
    pub fn tag_fg(&self, tag: &TagColour) -> Style {
        match tag {
            TagColour::Role(role) => self.fg(*role),
            TagColour::Fixed(_) if self.no_color => Style::default(),
            TagColour::Fixed(rgb) => Style {
                fg: Some(match self.depth {
                    ColourDepth::TrueColour => Colour::Rgb(*rgb),
                    ColourDepth::Ansi256 => Colour::Indexed(quantise_256(*rgb)),
                    ColourDepth::Ansi16 => Colour::Ansi(nearest_ansi(Colour::Rgb(*rgb))),
                }),
                ..Style::default()
            },
        }
    }

    /// A style using the role as background colour.
    pub fn bg(&self, role: Role) -> Style {
        if self.no_color {
            let modifiers = if role == Role::Selection {
                Modifiers::REVERSE
            } else {
                Modifiers::NONE
            };
            return Style {
                modifiers,
                ..Style::default()
            };
        }
        Style {
            bg: self.colour(role),
            ..Style::default()
        }
    }

    fn ansi16(&self, role: Role, colour: Colour) -> AnsiColour {
        self.theme
            .ansi_override(role)
            .or_else(|| default_ansi(role))
            .unwrap_or_else(|| nearest_ansi(colour))
    }
}

/// Emphasis standing in for colour when `NO_COLOR` is set.
fn no_color_modifiers(role: Role) -> Modifiers {
    match role {
        Role::TextBright | Role::Accent | Role::Danger => Modifiers::BOLD,
        Role::TextSecondary | Role::Muted | Role::Line => Modifiers::DIM,
        Role::Selection => Modifiers::REVERSE,
        _ => Modifiers::NONE,
    }
}

/// The built-in 16-colour mapping, used when a theme has no `[ansi]` entry.
fn default_ansi(role: Role) -> Option<AnsiColour> {
    Some(match role {
        Role::Accent => AnsiColour::Yellow,
        Role::Interactive => AnsiColour::Magenta,
        Role::Cyan => AnsiColour::Cyan,
        Role::Success => AnsiColour::Green,
        Role::Warning => AnsiColour::BrightYellow,
        Role::Danger => AnsiColour::Red,
        Role::Muted | Role::Line | Role::Selection => AnsiColour::BrightBlack,
        Role::Github => AnsiColour::Blue,
        Role::Gitlab => AnsiColour::Magenta,
        _ => return None,
    })
}

/// Nearest ANSI colour in OKLab.
pub fn nearest_ansi(colour: Colour) -> AnsiColour {
    let rgb = match colour {
        Colour::Rgb(rgb) => rgb,
        Colour::Indexed(i) => indexed_rgb(i),
        Colour::Ansi(a) => return a,
        Colour::Reset => return AnsiColour::White,
    };
    AnsiColour::ALL
        .into_iter()
        .min_by(|a, b| {
            rgb.distance(a.approx_rgb())
                .total_cmp(&rgb.distance(b.approx_rgb()))
        })
        .unwrap_or(AnsiColour::White)
}

const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// RGB value of an xterm-256 palette entry (0–15 use typical ANSI values).
pub fn indexed_rgb(i: u8) -> Rgb {
    match i {
        0..=15 => AnsiColour::ALL[usize::from(i)].approx_rgb(),
        16..=231 => {
            let n = usize::from(i - 16);
            Rgb::new(CUBE[n / 36], CUBE[(n / 6) % 6], CUBE[n % 6])
        }
        232..=255 => {
            let v = 8 + 10 * (i - 232);
            Rgb::new(v, v, v)
        }
    }
}

/// Nearest xterm-256 colour in OKLab. Entries 0–15 are skipped because terminals
/// let users redefine them.
pub fn quantise_256(rgb: Rgb) -> u8 {
    (16..=255u8)
        .min_by(|a, b| {
            rgb.distance(indexed_rgb(*a))
                .total_cmp(&rgb.distance(indexed_rgb(*b)))
        })
        .unwrap_or(16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::BUILTIN_IDS;

    fn palette(id: &str, depth: ColourDepth, no_color: bool) -> Palette {
        Palette::new(Theme::builtin(id).unwrap(), depth, no_color)
    }

    #[test]
    fn detects_depth() {
        use ColourDepth::*;
        assert_eq!(ColourDepth::detect(Some("truecolor"), None), TrueColour);
        assert_eq!(
            ColourDepth::detect(Some("24bit"), Some("xterm")),
            TrueColour
        );
        assert_eq!(ColourDepth::detect(None, Some("xterm-256color")), Ansi256);
        assert_eq!(ColourDepth::detect(None, Some("xterm")), Ansi16);
        assert_eq!(ColourDepth::detect(None, None), Ansi16);
        assert_eq!("256".parse(), Ok(Ansi256));
        assert_eq!("truecolor".parse(), Ok(TrueColour));
        assert_eq!("16".parse(), Ok(Ansi16));
        assert!("8".parse::<ColourDepth>().is_err());
    }

    fn env(pairs: &[(&str, &str)]) -> ColourDepth {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ColourDepth::detect_with(|key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
    }

    #[test]
    fn windows_terminal_in_wsl_is_truecolour_without_colorterm() {
        use ColourDepth::*;
        // WSL passes WT_SESSION through but not COLORTERM.
        assert_eq!(
            env(&[("TERM", "xterm-256color"), ("WT_SESSION", "abc")]),
            TrueColour
        );
        assert_eq!(env(&[("TERM", "xterm-256color")]), Ansi256);
        assert_eq!(
            env(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "iTerm.app")]),
            TrueColour
        );
        assert_eq!(
            env(&[("TERM", "xterm-256color"), ("VTE_VERSION", "7603")]),
            TrueColour
        );
        assert_eq!(
            env(&[("TERM", "xterm-256color"), ("VTE_VERSION", "3000")]),
            Ansi256
        );
        assert_eq!(env(&[("TERM", "xterm-kitty")]), TrueColour);
        assert_eq!(env(&[("TERM", "xterm-direct")]), TrueColour);
    }

    #[test]
    fn multiplexers_do_not_inherit_the_outer_terminal_hints() {
        use ColourDepth::*;
        assert_eq!(
            env(&[("TERM", "tmux-256color"), ("WT_SESSION", "abc")]),
            Ansi256
        );
        assert_eq!(
            env(&[
                ("TERM", "xterm-256color"),
                ("TMUX", "/tmp/x"),
                ("WT_SESSION", "abc")
            ]),
            Ansi256
        );
        assert_eq!(
            env(&[
                ("TERM", "screen"),
                ("COLORTERM", "truecolor"),
                ("WT_SESSION", "abc")
            ]),
            TrueColour
        );
    }

    #[test]
    fn the_override_wins_over_every_hint() {
        use ColourDepth::*;
        assert_eq!(
            env(&[
                ("REVIEW_BUDDY_COLOUR_DEPTH", "256"),
                ("COLORTERM", "truecolor")
            ]),
            Ansi256
        );
        assert_eq!(
            env(&[
                ("REVIEW_BUDDY_COLOUR_DEPTH", "truecolor"),
                ("TERM", "xterm")
            ]),
            TrueColour
        );
        assert_eq!(
            env(&[("REVIEW_BUDDY_COLOUR_DEPTH", "nonsense"), ("TERM", "xterm")]),
            Ansi16
        );
    }

    #[test]
    fn quantise_256_hits_exact_entries() {
        assert_eq!(quantise_256(Rgb::new(0, 0, 0)), 16);
        assert_eq!(quantise_256(Rgb::new(255, 255, 255)), 231);
        assert_eq!(quantise_256(Rgb::new(255, 0, 0)), 196);
        assert_eq!(quantise_256(Rgb::new(95, 135, 175)), 16 + 36 + 2 * 6 + 3);
        assert_eq!(quantise_256(Rgb::new(128, 128, 128)), 244);
    }

    #[test]
    fn nearest_ansi_picks_obvious_colours() {
        assert_eq!(
            nearest_ansi(Colour::rgb(250, 10, 10)),
            AnsiColour::BrightRed
        );
        assert_eq!(nearest_ansi(Colour::rgb(0, 0, 0)), AnsiColour::Black);
        assert_eq!(nearest_ansi(Colour::Indexed(196)), AnsiColour::BrightRed);
    }

    #[test]
    fn truecolour_passes_through() {
        let p = palette("liminal-hq", ColourDepth::TrueColour, false);
        assert_eq!(p.colour(Role::Accent), Some(Colour::rgb(0xff, 0xaa, 0x40)));
        assert_eq!(p.colour(Role::Background), None);
    }

    #[test]
    fn depth_256_quantises_every_role() {
        let p = palette("dusk", ColourDepth::Ansi256, false);
        for role in Role::ALL {
            assert!(!matches!(p.colour(*role), Some(Colour::Rgb(_))), "{role:?}");
        }
        assert!(p.wordmark().iter().all(|c| matches!(c, Colour::Indexed(_))));
    }

    #[test]
    fn a_blended_wordmark_keeps_its_gradient_at_256_colours() {
        use std::collections::HashSet;
        let p = palette("liminal-hq", ColourDepth::Ansi256, false);
        let stops = p.wordmark_blend_stops();
        assert!(stops.iter().all(|c| matches!(c, Colour::Rgb(_))));
        let blend = |t: f32| {
            let t = t * (stops.len() - 1) as f32;
            let i = (t.floor() as usize).min(stops.len() - 2);
            let (Colour::Rgb(a), Colour::Rgb(b)) = (stops[i], stops[i + 1]) else {
                unreachable!()
            };
            let l = t - i as f32;
            let mix =
                |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * l).round() as u8;
            Colour::rgb(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b))
        };
        let steps: HashSet<Colour> = (0..12)
            .map(|i| p.at_depth(blend(i as f32 / 11.0)))
            .collect();
        assert!(
            steps.len() > stops.len() + 2,
            "{} distinct colours",
            steps.len()
        );
        assert!(steps.iter().all(|c| matches!(c, Colour::Indexed(_))));
        let none = palette("liminal-hq", ColourDepth::Ansi256, true);
        assert!(none.wordmark_blend_stops().is_empty());
    }

    #[test]
    fn depth_16_honours_ansi_table_then_defaults() {
        let p = palette("liminal-hq", ColourDepth::Ansi16, false);
        assert_eq!(
            p.colour(Role::Accent),
            Some(Colour::Ansi(AnsiColour::Yellow))
        );
        assert_eq!(
            p.colour(Role::Warning),
            Some(Colour::Ansi(AnsiColour::BrightYellow))
        );
        // dusk has no [ansi] of its own but inherits liminal-hq's table.
        let d = palette("dusk", ColourDepth::Ansi16, false);
        assert_eq!(d.colour(Role::Danger), Some(Colour::Ansi(AnsiColour::Red)));

        let src = "[ansi]\naccent = \"bright-red\"\n";
        let t = Theme::from_toml("x", src, &|_| None).unwrap();
        let p = Palette::new(t, ColourDepth::Ansi16, false);
        assert_eq!(
            p.colour(Role::Accent),
            Some(Colour::Ansi(AnsiColour::BrightRed))
        );
        for role in Role::ALL {
            assert!(!matches!(
                p.colour(*role),
                Some(Colour::Rgb(_) | Colour::Indexed(_))
            ));
        }
        assert!(p.wordmark().iter().all(|c| matches!(c, Colour::Ansi(_))));
    }

    #[test]
    fn tag_colours_follow_depth_and_no_color() {
        let fixed = TagColour::Fixed(Rgb::new(250, 10, 10));
        let role = TagColour::Role(Role::Github);
        let p = palette("dusk", ColourDepth::TrueColour, false);
        assert_eq!(p.tag_fg(&fixed).fg, Some(Colour::rgb(250, 10, 10)));
        assert_eq!(p.tag_fg(&role), p.fg(Role::Github));
        let p = palette("dusk", ColourDepth::Ansi256, false);
        assert_eq!(p.tag_fg(&fixed).fg, Some(Colour::Indexed(196)));
        let p = palette("dusk", ColourDepth::Ansi16, false);
        assert_eq!(
            p.tag_fg(&fixed).fg,
            Some(Colour::Ansi(AnsiColour::BrightRed))
        );
        assert_eq!(p.tag_fg(&role).fg, Some(Colour::Ansi(AnsiColour::Blue)));
        let p = palette("dusk", ColourDepth::TrueColour, true);
        assert_eq!(p.tag_fg(&fixed), Style::default());
        assert_eq!(p.tag_fg(&role).fg, None);
    }

    #[test]
    fn no_color_collapses_to_modifiers() {
        let p = palette("dusk", ColourDepth::TrueColour, true);
        for role in Role::ALL {
            assert_eq!(p.fg(*role).fg, None);
            assert_eq!(p.fg(*role).bg, None);
            assert_eq!(p.bg(*role).bg, None);
        }
        assert_eq!(p.fg(Role::Accent).modifiers, Modifiers::BOLD);
        assert_eq!(p.fg(Role::Muted).modifiers, Modifiers::DIM);
        assert_eq!(p.bg(Role::Selection).modifiers, Modifiers::REVERSE);
        assert_eq!(p.fg(Role::Text).modifiers, Modifiers::NONE);
        assert!(p.wordmark().is_empty());
        assert_eq!(p.syntax(SyntaxRole::Keyword), None);
    }

    fn bg(
        id: &str,
        global: BackgroundMode,
        per: Option<BackgroundMode>,
        depth: ColourDepth,
        no_color: bool,
    ) -> Option<Colour> {
        resolve_background(&Theme::builtin(id).unwrap(), global, per, depth, no_color)
    }

    const DEPTHS: [ColourDepth; 3] = [
        ColourDepth::TrueColour,
        ColourDepth::Ansi256,
        ColourDepth::Ansi16,
    ];

    #[test]
    fn theme_mode_follows_the_theme() {
        use BackgroundMode::*;
        let t = ColourDepth::TrueColour;
        assert_eq!(bg("liminal-hq", Theme, None, t, false), None);
        assert_eq!(bg("dusk", Theme, None, t, false), None);
        assert_eq!(
            bg("afterglow-dark", Theme, None, t, false),
            Some(Colour::rgb(0x0f, 0x0e, 0x1a))
        );
        assert_eq!(
            bg("afterglow-light", Theme, None, t, false),
            Some(Colour::rgb(0xfb, 0xfa, 0xf6))
        );
    }

    #[test]
    fn yes_paints_a_transparent_theme_with_its_fallback() {
        use BackgroundMode::*;
        let t = ColourDepth::TrueColour;
        assert_eq!(
            bg("liminal-hq", Yes, None, t, false),
            Some(Colour::rgb(0x05, 0x05, 0x07))
        );
        assert_eq!(
            bg("dusk", Yes, None, t, false),
            Some(Colour::rgb(0x05, 0x05, 0x07))
        );
        assert_eq!(
            bg("afterglow-dark", Yes, None, t, false),
            Some(Colour::rgb(0x0f, 0x0e, 0x1a))
        );
    }

    #[test]
    fn no_never_paints() {
        for id in BUILTIN_IDS {
            for depth in DEPTHS {
                assert_eq!(bg(id, BackgroundMode::No, None, depth, false), None);
            }
        }
    }

    #[test]
    fn no_color_never_paints_in_any_mode() {
        for id in BUILTIN_IDS {
            for mode in BackgroundMode::ALL {
                for depth in DEPTHS {
                    assert_eq!(bg(id, mode, Some(mode), depth, true), None);
                }
            }
        }
    }

    #[test]
    fn per_theme_beats_global() {
        use BackgroundMode::*;
        let t = ColourDepth::TrueColour;
        assert!(bg("dusk", No, Some(Yes), t, false).is_some());
        assert_eq!(bg("afterglow-dark", Yes, Some(No), t, false), None);
        assert!(bg("afterglow-dark", No, Some(Theme), t, false).is_some());
        assert_eq!(bg("dusk", Yes, Some(Theme), t, false), None);
    }

    #[test]
    fn sixteen_colours_paint_only_when_forced() {
        use BackgroundMode::*;
        let d = ColourDepth::Ansi16;
        assert_eq!(bg("afterglow-dark", Theme, None, d, false), None);
        assert_eq!(bg("afterglow-light", Theme, None, d, false), None);
        assert!(matches!(
            bg("afterglow-dark", Yes, None, d, false),
            Some(Colour::Ansi(_))
        ));
        assert!(matches!(
            bg("dusk", Yes, None, d, false),
            Some(Colour::Ansi(_))
        ));
        assert!(bg("afterglow-dark", Theme, Some(Yes), d, false).is_some());
    }

    #[test]
    fn backgrounds_quantise_to_the_depth() {
        let d = ColourDepth::Ansi256;
        assert!(matches!(
            bg("afterglow-dark", BackgroundMode::Theme, None, d, false),
            Some(Colour::Indexed(_))
        ));
    }

    #[test]
    fn palette_background_uses_its_mode() {
        let p = palette("dusk", ColourDepth::TrueColour, false);
        assert_eq!(p.background(), None);
        let p = p.with_background_mode(BackgroundMode::Yes);
        assert!(p.background().is_some());
        assert_eq!(p.background_mode(), BackgroundMode::Yes);
    }

    #[test]
    fn settings_precedence_is_session_env_theme_global() {
        use BackgroundMode::*;
        let mut s = BackgroundSettings {
            global: Yes,
            ..Default::default()
        };
        assert_eq!(s.mode_for("dusk"), Yes);
        s.per_theme.insert("dusk".into(), No);
        assert_eq!(s.mode_for("dusk"), No);
        assert_eq!(s.mode_for("other"), Yes);
        s.env = Some(Theme);
        assert_eq!(s.mode_for("dusk"), Theme);
        s.session = Some(Yes);
        assert_eq!(s.mode_for("dusk"), Yes);
    }

    #[test]
    fn modes_parse_and_cycle() {
        for m in BackgroundMode::ALL {
            assert_eq!(m.key().parse(), Ok(m));
        }
        assert_eq!(" YES ".parse(), Ok(BackgroundMode::Yes));
        assert!("maybe".parse::<BackgroundMode>().is_err());
        assert_eq!(BackgroundMode::Theme.next(), BackgroundMode::Yes);
        assert_eq!(BackgroundMode::Yes.next(), BackgroundMode::No);
        assert_eq!(BackgroundMode::No.next(), BackgroundMode::Theme);
    }
}
