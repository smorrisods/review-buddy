use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Blends `self` at `alpha` (0–255) over `base`, rounding to the nearest channel value.
    pub fn blend_over(self, base: Rgb, alpha: u8) -> Rgb {
        let mix = |fg: u8, bg: u8| {
            let a = u32::from(alpha);
            ((u32::from(fg) * a + u32::from(bg) * (255 - a) + 127) / 255) as u8
        };
        Rgb::new(
            mix(self.r, base.r),
            mix(self.g, base.g),
            mix(self.b, base.b),
        )
    }

    pub(crate) fn oklab(self) -> [f64; 3] {
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (lin(self.r), lin(self.g), lin(self.b));
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        [
            0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
            1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
            0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
        ]
    }

    pub(crate) fn distance(self, other: Rgb) -> f64 {
        let (a, b) = (self.oklab(), other.oklab());
        (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
    }
}

/// The 16 ANSI colours, in standard palette order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnsiColour {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl AnsiColour {
    pub const ALL: [AnsiColour; 16] = [
        AnsiColour::Black,
        AnsiColour::Red,
        AnsiColour::Green,
        AnsiColour::Yellow,
        AnsiColour::Blue,
        AnsiColour::Magenta,
        AnsiColour::Cyan,
        AnsiColour::White,
        AnsiColour::BrightBlack,
        AnsiColour::BrightRed,
        AnsiColour::BrightGreen,
        AnsiColour::BrightYellow,
        AnsiColour::BrightBlue,
        AnsiColour::BrightMagenta,
        AnsiColour::BrightCyan,
        AnsiColour::BrightWhite,
    ];

    const NAMES: [&'static str; 16] = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "bright-black",
        "bright-red",
        "bright-green",
        "bright-yellow",
        "bright-blue",
        "bright-magenta",
        "bright-cyan",
        "bright-white",
    ];

    /// Index in the standard 16-colour palette (0–15).
    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0) as u8
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[usize::from(self.index())]
    }

    /// Typical xterm RGB value, used only to find the nearest ANSI colour.
    pub fn approx_rgb(self) -> Rgb {
        const TABLE: [(u8, u8, u8); 16] = [
            (0, 0, 0),
            (205, 0, 0),
            (0, 205, 0),
            (205, 205, 0),
            (0, 0, 238),
            (205, 0, 205),
            (0, 205, 205),
            (229, 229, 229),
            (127, 127, 127),
            (255, 0, 0),
            (0, 255, 0),
            (255, 255, 0),
            (92, 92, 255),
            (255, 0, 255),
            (0, 255, 255),
            (255, 255, 255),
        ];
        let (r, g, b) = TABLE[usize::from(self.index())];
        Rgb::new(r, g, b)
    }
}

impl FromStr for AnsiColour {
    type Err = ColourParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let norm = s.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        let norm = match norm.as_str() {
            "gray" | "grey" | "dark-gray" | "dark-grey" => "bright-black".to_string(),
            "light-gray" | "light-grey" => "white".to_string(),
            _ => norm,
        };
        Self::NAMES
            .iter()
            .position(|n| *n == norm)
            .map(|i| Self::ALL[i])
            .ok_or_else(|| ColourParseError(s.to_string()))
    }
}

/// A framework-neutral resolved colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Colour {
    /// The terminal's own colour; never painted.
    Reset,
    Rgb(Rgb),
    /// An xterm 256-colour index.
    Indexed(u8),
    Ansi(AnsiColour),
}

impl Colour {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Colour::Rgb(Rgb::new(r, g, b))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColourParseError(pub String);

impl fmt::Display for ColourParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "'{}' is not a colour; use #rrggbb, #rrggbbaa, a 0–255 index, an ANSI name, or transparent",
            self.0
        )
    }
}

impl std::error::Error for ColourParseError {}

/// A colour as written in a theme file, before alpha blending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColourSpec {
    Solid(Colour),
    Rgba(Rgb, u8),
}

impl FromStr for ColourSpec {
    type Err = ColourParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        let err = || ColourParseError(s.to_string());
        if let Some(hex) = t.strip_prefix('#') {
            if !hex.is_ascii() || !(hex.len() == 6 || hex.len() == 8) {
                return Err(err());
            }
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| err());
            let rgb = Rgb::new(byte(0)?, byte(2)?, byte(4)?);
            return Ok(if hex.len() == 8 {
                ColourSpec::Rgba(rgb, byte(6)?)
            } else {
                ColourSpec::Solid(Colour::Rgb(rgb))
            });
        }
        let lower = t.to_ascii_lowercase();
        if lower == "transparent" || lower == "reset" {
            return Ok(ColourSpec::Solid(Colour::Reset));
        }
        if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) {
            return t
                .parse::<u8>()
                .map(|i| ColourSpec::Solid(Colour::Indexed(i)))
                .map_err(|_| err());
        }
        t.parse::<AnsiColour>()
            .map(|a| ColourSpec::Solid(Colour::Ansi(a)))
    }
}

/// WCAG relative luminance of an sRGB colour.
pub fn relative_luminance(c: Rgb) -> f64 {
    let lin = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio between two colours, from 1.0 to 21.0.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(s: &str) -> Result<ColourSpec, ColourParseError> {
        s.parse()
    }

    #[test]
    fn parses_every_form() {
        assert_eq!(
            spec("#ffaa40"),
            Ok(ColourSpec::Solid(Colour::rgb(255, 170, 64)))
        );
        assert_eq!(
            spec("#ffaa401f"),
            Ok(ColourSpec::Rgba(Rgb::new(255, 170, 64), 0x1f))
        );
        assert_eq!(spec("237"), Ok(ColourSpec::Solid(Colour::Indexed(237))));
        assert_eq!(
            spec("bright-black"),
            Ok(ColourSpec::Solid(Colour::Ansi(AnsiColour::BrightBlack)))
        );
        assert_eq!(spec("Transparent"), Ok(ColourSpec::Solid(Colour::Reset)));
        assert_eq!(spec("reset"), Ok(ColourSpec::Solid(Colour::Reset)));
    }

    #[test]
    fn rejects_bad_colours() {
        for bad in ["", "#fff", "#gggggg", "256", "chartreuse", "#ffaa40é"] {
            assert!(spec(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn blend_maths() {
        let white = Rgb::new(255, 255, 255);
        let black = Rgb::new(0, 0, 0);
        assert_eq!(white.blend_over(black, 0), black);
        assert_eq!(white.blend_over(black, 255), white);
        assert_eq!(white.blend_over(black, 128), Rgb::new(128, 128, 128));
    }

    #[test]
    fn wcag_contrast() {
        let (w, b) = (Rgb::new(255, 255, 255), Rgb::new(0, 0, 0));
        assert!((contrast_ratio(w, b) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(w, w) - 1.0).abs() < 1e-9);
        assert_eq!(contrast_ratio(w, b), contrast_ratio(b, w));
    }
}
