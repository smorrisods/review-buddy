//! Settings → Sources: seeing the connected accounts, checking their tokens, and adding,
//! editing, switching off or removing them.
//!
//! - [`state`] is the pure state machine the screen drives. Keys, clicks and the results of
//!   effects go in; effects to run come out.
//! - [`effects`] runs them: reading the layered config, testing a token, and writing changes to
//!   the write target through [`edit`] (comments preserved, atomic, `0600`). Programs, the
//!   keyring and the filesystem are injected through [`Services`], so tests use fakes.
//! - [`edit`] holds the `[[source]]` edits themselves.
//!
//! Where a source is defined decides whether it can be changed here. The config layers merge
//! with later files replacing earlier `[[source]]` lists wholesale, so a source from a
//! `config.d` drop-in or `$XDG_CONFIG_DIRS` can't be overridden from the user file, and writing
//! a new list there would hide the other one. Those sources are shown read-only with where they
//! come from, and nothing here writes to a file that isn't the write target.

pub mod edit;
pub mod effects;
pub mod state;

pub use effects::{run_effect, unavailable, Loaded, Services};
pub use state::{
    Change, Check, Click, Effect, Field, Form, Hidden, Input, Modal, Origin, Out, Pick, Remove,
    RemoveChoice, Saved, Snapshot, SourceRow, State, TestInfo,
};
