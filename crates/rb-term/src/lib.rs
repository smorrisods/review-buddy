//! A terminal pane for Review Buddy.
//!
//! The crate keeps three things apart so the app only ever sees bytes in and a screen plus events
//! out:
//!
//! - [`Emulator`] is a thin boundary around the `alacritty_terminal` core. Nothing outside
//!   `emulator.rs` names that crate, so a version bump touches one file.
//! - [`Pty`] spawns a command in a pseudo-terminal (ConPTY on Windows) through `portable-pty` and
//!   reports its output and exit as [`PtyEvent`]s.
//! - [`Pane`] ties an emulator to either a real child (the host writes [`Pane::feed`]ed bytes and
//!   sends [`Output::reply`] back) or a scripted, in-process transcript used by demo mode.
//!
//! Around those sit the pure helpers the app needs: [`keys`] and [`mouse`] encoders, the [`focus`]
//! model for leaving the pane, and [`worktree`] planning for a managed checkout. The ratatui
//! [`widget`] is behind the `widget` feature; no other module uses ratatui types.
//!
//! Images inside the pane (sixel, kitty graphics, iTerm2) are out of scope. The emulator drops
//! them, so a child that draws one shows nothing in its place.

pub mod emulator;
pub mod focus;
pub mod keys;
pub mod mouse;
pub mod pane;
pub mod pty;
pub mod screen;
pub mod script;
pub mod worktree;

#[cfg(feature = "widget")]
pub mod widget;

pub use emulator::{ColorScheme, Emulator, Event, KittyFlags, Modes, MouseMode, Output};
pub use focus::{Chord, ChordError, EscapeAction, EscapeState};
pub use keys::{encode_key, host_supports_kitty, KeyContext};
pub use mouse::{encode_mouse, Wheel};
pub use pane::{Pane, PaneKind};
pub use pty::{default_shell, host_cleanup, Pty, PtyEvent, SpawnError, SpawnSpec};
pub use screen::{Attrs, Cell, Color, Cursor, CursorShape, Screen};
pub use script::ScriptContext;
