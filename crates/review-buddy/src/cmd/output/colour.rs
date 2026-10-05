//! ANSI colour for command output, driven by `rb-theme` roles.

use rb_paths::Env;
use rb_theme::{AnsiColour, Colour, ColourDepth, Palette, Role, Style, Theme};

use crate::cli::ColorWhen;

/// Whether to colour output. `--color`/`--no-color` win; then `NO_COLOR`, `CLICOLOR_FORCE`,
/// and finally whether stdout is a terminal that isn't `TERM=dumb`.
pub fn colour_enabled(
    choice: Option<ColorWhen>,
    no_color_flag: bool,
    env: &dyn Env,
    stdout_is_tty: bool,
) -> bool {
    if no_color_flag {
        return false;
    }
    match choice {
        Some(ColorWhen::Always) => return true,
        Some(ColorWhen::Never) => return false,
        Some(ColorWhen::Auto) | None => {}
    }
    let set = |key: &str| env.var(key).is_some_and(|v| !v.is_empty());
    if set("NO_COLOR") {
        return false;
    }
    if env
        .var("CLICOLOR_FORCE")
        .is_some_and(|v| !v.is_empty() && v != "0")
    {
        return true;
    }
    stdout_is_tty && env.var("TERM").as_deref() != Some("dumb")
}

/// Wraps text in the escape sequences for a theme role, or leaves it alone when colour is off.
#[derive(Debug, Clone)]
pub struct Painter {
    palette: Palette,
    enabled: bool,
}

impl Painter {
    pub fn new(palette: Palette, enabled: bool) -> Self {
        Self { palette, enabled }
    }

    /// A painter that never emits escapes.
    pub fn plain() -> Self {
        Self::new(
            Palette::new(Theme::default(), ColourDepth::Ansi16, true),
            false,
        )
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    pub fn paint(&self, role: Role, text: &str) -> String {
        if !self.enabled || text.is_empty() {
            return text.to_string();
        }
        let params = sgr(&self.palette.fg(role));
        if params.is_empty() {
            return text.to_string();
        }
        format!("\u{1b}[{params}m{text}\u{1b}[0m")
    }
}

fn sgr(style: &Style) -> String {
    let mut parts: Vec<String> = Vec::new();
    if style.modifiers.bold {
        parts.push("1".into());
    }
    if style.modifiers.dim {
        parts.push("2".into());
    }
    if style.modifiers.reverse {
        parts.push("7".into());
    }
    if let Some(fg) = style.fg {
        parts.push(colour_code(fg, 30));
    }
    if let Some(bg) = style.bg {
        parts.push(colour_code(bg, 40));
    }
    parts.join(";")
}

fn colour_code(colour: Colour, base: u8) -> String {
    match colour {
        Colour::Reset => (base + 9).to_string(),
        Colour::Rgb(rgb) => format!("{};2;{};{};{}", base + 8, rgb.r, rgb.g, rgb.b),
        Colour::Indexed(i) => format!("{};5;{i}", base + 8),
        Colour::Ansi(a) => {
            let index = AnsiColour::ALL.iter().position(|c| *c == a).unwrap_or(7) as u8;
            if index < 8 {
                (base + index).to_string()
            } else {
                (base + 60 + index - 8).to_string()
            }
        }
    }
}

/// Removes CSI escape sequences, for comparing coloured output with plain text.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_paths::MapEnv;

    fn env(vars: &[(&str, &str)]) -> MapEnv {
        vars.iter()
            .fold(MapEnv::new("/home/a"), |e, (k, v)| e.with_var(k, v))
    }

    #[test]
    fn flags_beat_the_environment() {
        let noisy = env(&[("CLICOLOR_FORCE", "1")]);
        assert!(!colour_enabled(None, true, &noisy, true));
        assert!(!colour_enabled(Some(ColorWhen::Never), false, &noisy, true));
        let quiet = env(&[("NO_COLOR", "1")]);
        assert!(colour_enabled(
            Some(ColorWhen::Always),
            false,
            &quiet,
            false
        ));
    }

    #[test]
    fn auto_follows_tty_no_color_and_clicolor_force() {
        let none = env(&[]);
        assert!(colour_enabled(None, false, &none, true));
        assert!(!colour_enabled(None, false, &none, false));
        assert!(!colour_enabled(
            Some(ColorWhen::Auto),
            false,
            &env(&[("NO_COLOR", "1")]),
            true
        ));
        assert!(colour_enabled(
            None,
            false,
            &env(&[("CLICOLOR_FORCE", "1")]),
            false
        ));
        assert!(!colour_enabled(
            None,
            false,
            &env(&[("CLICOLOR_FORCE", "0")]),
            false
        ));
        assert!(colour_enabled(None, false, &env(&[("NO_COLOR", "")]), true));
        assert!(!colour_enabled(
            None,
            false,
            &env(&[("TERM", "dumb")]),
            true
        ));
    }

    #[test]
    fn plain_painters_leave_text_alone() {
        assert_eq!(Painter::plain().paint(Role::Success, "ok"), "ok");
    }

    #[test]
    fn enabled_painters_wrap_in_sgr_and_reset() {
        let palette = Palette::new(Theme::default(), ColourDepth::TrueColour, false);
        let painter = Painter::new(palette, true);
        let out = painter.paint(Role::Success, "ok");
        assert!(
            out.starts_with("\u{1b}[38;2;") && out.ends_with("ok\u{1b}[0m"),
            "{out:?}"
        );
        assert_eq!(strip_ansi(&out), "ok");
        assert_eq!(painter.paint(Role::Success, ""), "");
    }

    #[test]
    fn depth_changes_the_code_shape() {
        let p256 = Painter::new(
            Palette::new(Theme::default(), ColourDepth::Ansi256, false),
            true,
        );
        assert!(p256.paint(Role::Danger, "x").starts_with("\u{1b}[38;5;"));
        let p16 = Painter::new(
            Palette::new(Theme::default(), ColourDepth::Ansi16, false),
            true,
        );
        let out = p16.paint(Role::Danger, "x");
        assert!(!out.contains(";2;") && !out.contains(";5;"), "{out:?}");
    }

    #[test]
    fn colour_codes() {
        assert_eq!(colour_code(Colour::Ansi(AnsiColour::Red), 30), "31");
        assert_eq!(colour_code(Colour::Ansi(AnsiColour::BrightRed), 30), "91");
        assert_eq!(colour_code(Colour::Ansi(AnsiColour::Green), 40), "42");
        assert_eq!(colour_code(Colour::Indexed(200), 30), "38;5;200");
    }

    #[test]
    fn strip_ansi_removes_only_escapes() {
        assert_eq!(strip_ansi("\u{1b}[1;31mhi\u{1b}[0m there"), "hi there");
    }
}
