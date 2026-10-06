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
    /// Picks a depth from `COLORTERM` and `TERM` values.
    pub fn detect(colorterm: Option<&str>, term: Option<&str>) -> Self {
        let colorterm = colorterm.unwrap_or_default().to_ascii_lowercase();
        if colorterm == "truecolor" || colorterm == "24bit" {
            ColourDepth::TrueColour
        } else if term.is_some_and(|t| t.contains("256color")) {
            ColourDepth::Ansi256
        } else {
            ColourDepth::Ansi16
        }
    }

    pub fn from_env() -> Self {
        Self::detect(
            std::env::var("COLORTERM").ok().as_deref(),
            std::env::var("TERM").ok().as_deref(),
        )
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
}

impl Palette {
    pub fn new(theme: Theme, depth: ColourDepth, no_color: bool) -> Self {
        Self {
            theme,
            depth,
            no_color,
        }
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
}
