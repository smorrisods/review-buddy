//! Images in change descriptions: finding them, fetching them within strict limits, caching the
//! bytes, and drawing them where the terminal can. See `docs/configuration.md` for `ui.images`.

pub mod cache;
pub mod detect;
pub mod extract;
pub mod fetch;
pub mod hosts;
pub mod layout;
pub mod state;

pub use extract::{ImageRef, Segment};
pub use fetch::Failure;
pub use state::{Slot, State, View};

/// The most images drawn for one description; the rest are counted in a note.
pub const MAX_IMAGES: usize = 8;
