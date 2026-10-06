//! Review Buddy theme loading, role resolution, and colour-depth quantisation.
//!
//! This crate is framework-neutral: it hands out [`Colour`] values and [`Style`]s that the
//! TUI crate maps onto its rendering library.

mod colour;
mod palette;
mod theme;

pub use colour::{
    contrast_ratio, relative_luminance, AnsiColour, Colour, ColourParseError, Rgb, TagColour,
    TagColourError,
};
pub use palette::{
    indexed_rgb, nearest_ansi, no_color_requested, quantise_256, resolve_background,
    BackgroundMode, BackgroundSettings, ColourDepth, Modifiers, Palette, Style,
};
pub use theme::{Appearance, Role, SyntaxRole, Theme, ThemeError, BUILTIN_IDS, DEFAULT_THEME_ID};
