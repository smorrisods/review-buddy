//! The `review-buddy` terminal interface: app state, rendering, config, and the runtime loop.

pub mod app;
pub mod cli;
pub mod cmd;
pub mod config;
#[cfg(feature = "demo")]
pub mod demo;
pub mod drafts;
pub mod images;
pub mod load;
#[cfg(feature = "live")]
pub mod providers;
pub mod runtime;
pub mod session;
pub mod settings;
pub mod setup;
pub mod ui;
