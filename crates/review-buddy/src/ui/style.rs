//! Maps the framework-neutral `rb-theme` colours and styles onto ratatui.

use ratatui::style::{Color, Modifier, Style};
use rb_theme::{AnsiColour, Colour, Modifiers, Palette, Role};

pub fn colour(c: Colour) -> Color {
    match c {
        Colour::Reset => Color::Reset,
        Colour::Rgb(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        Colour::Indexed(i) => Color::Indexed(i),
        Colour::Ansi(a) => ansi(a),
    }
}

fn ansi(a: AnsiColour) -> Color {
    match a {
        AnsiColour::Black => Color::Black,
        AnsiColour::Red => Color::Red,
        AnsiColour::Green => Color::Green,
        AnsiColour::Yellow => Color::Yellow,
        AnsiColour::Blue => Color::Blue,
        AnsiColour::Magenta => Color::Magenta,
        AnsiColour::Cyan => Color::Cyan,
        AnsiColour::White => Color::Gray,
        AnsiColour::BrightBlack => Color::DarkGray,
        AnsiColour::BrightRed => Color::LightRed,
        AnsiColour::BrightGreen => Color::LightGreen,
        AnsiColour::BrightYellow => Color::LightYellow,
        AnsiColour::BrightBlue => Color::LightBlue,
        AnsiColour::BrightMagenta => Color::LightMagenta,
        AnsiColour::BrightCyan => Color::LightCyan,
        AnsiColour::BrightWhite => Color::White,
    }
}

fn modifiers(m: Modifiers) -> Modifier {
    let mut out = Modifier::empty();
    out.set(Modifier::BOLD, m.bold);
    out.set(Modifier::DIM, m.dim);
    out.set(Modifier::REVERSED, m.reverse);
    out
}

pub fn style(s: rb_theme::Style) -> Style {
    let mut out = Style::default().add_modifier(modifiers(s.modifiers));
    if let Some(fg) = s.fg {
        out = out.fg(colour(fg));
    }
    if let Some(bg) = s.bg {
        out = out.bg(colour(bg));
    }
    out
}

/// Foreground style for a role.
pub fn fg(palette: &Palette, role: Role) -> Style {
    style(palette.fg(role))
}

/// Background style for a role. Transparent roles add no colour.
pub fn bg(palette: &Palette, role: Role) -> Style {
    style(palette.bg(role))
}

/// The colour at `t` (0.0–1.0) along `stops`. Blends RGB neighbours; other colour
/// kinds step to the nearest stop.
pub fn gradient_at(stops: &[Colour], t: f32) -> Option<Colour> {
    match stops {
        [] => None,
        [only] => Some(*only),
        _ => {
            let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
            let i = (t.floor() as usize).min(stops.len() - 2);
            let local = t - i as f32;
            Some(match (stops[i], stops[i + 1]) {
                (Colour::Rgb(a), Colour::Rgb(b)) => {
                    let mix = |x: u8, y: u8| {
                        (f32::from(x) + (f32::from(y) - f32::from(x)) * local).round() as u8
                    };
                    Colour::rgb(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b))
                }
                (a, b) => {
                    if local < 0.5 {
                        a
                    } else {
                        b
                    }
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_theme::{ColourDepth, Theme};

    #[test]
    fn maps_every_colour_kind() {
        assert_eq!(colour(Colour::Reset), Color::Reset);
        assert_eq!(colour(Colour::rgb(1, 2, 3)), Color::Rgb(1, 2, 3));
        assert_eq!(colour(Colour::Indexed(237)), Color::Indexed(237));
        assert_eq!(
            colour(Colour::Ansi(AnsiColour::BrightBlack)),
            Color::DarkGray
        );
        assert_eq!(colour(Colour::Ansi(AnsiColour::White)), Color::Gray);
        assert_eq!(colour(Colour::Ansi(AnsiColour::BrightWhite)), Color::White);
    }

    #[test]
    fn maps_modifiers_and_colours() {
        let s = style(rb_theme::Style {
            fg: Some(Colour::rgb(9, 9, 9)),
            bg: None,
            modifiers: Modifiers {
                bold: true,
                dim: true,
                reverse: false,
            },
        });
        assert_eq!(s.fg, Some(Color::Rgb(9, 9, 9)));
        assert_eq!(s.bg, None);
        assert!(s.add_modifier.contains(Modifier::BOLD | Modifier::DIM));
        assert!(!s.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn transparent_background_adds_no_colour() {
        let p = Palette::new(Theme::default(), ColourDepth::TrueColour, false);
        assert_eq!(bg(&p, Role::Background).bg, None);
        assert!(fg(&p, Role::Accent).fg.is_some());
    }

    #[test]
    fn no_color_drops_colours_but_keeps_emphasis() {
        let p = Palette::new(Theme::default(), ColourDepth::TrueColour, true);
        let s = fg(&p, Role::Accent);
        assert_eq!(s.fg, None);
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn gradient_endpoints_and_midpoint() {
        let stops = [Colour::rgb(0, 0, 0), Colour::rgb(100, 200, 50)];
        assert_eq!(gradient_at(&stops, 0.0), Some(Colour::rgb(0, 0, 0)));
        assert_eq!(gradient_at(&stops, 1.0), Some(Colour::rgb(100, 200, 50)));
        assert_eq!(gradient_at(&stops, 0.5), Some(Colour::rgb(50, 100, 25)));
        assert_eq!(gradient_at(&stops, 9.0), Some(Colour::rgb(100, 200, 50)));
    }

    #[test]
    fn gradient_spans_three_stops_and_handles_degenerate_input() {
        let stops = [
            Colour::rgb(255, 0, 0),
            Colour::rgb(0, 255, 0),
            Colour::rgb(0, 0, 255),
        ];
        assert_eq!(gradient_at(&stops, 0.5), Some(Colour::rgb(0, 255, 0)));
        assert_eq!(gradient_at(&[], 0.5), None);
        assert_eq!(gradient_at(&stops[..1], 0.9), Some(stops[0]));
    }

    #[test]
    fn gradient_steps_between_non_rgb_stops() {
        let stops = [Colour::Indexed(1), Colour::Indexed(9)];
        assert_eq!(gradient_at(&stops, 0.2), Some(Colour::Indexed(1)));
        assert_eq!(gradient_at(&stops, 0.8), Some(Colour::Indexed(9)));
    }
}
