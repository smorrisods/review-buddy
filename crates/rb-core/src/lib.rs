//! Review Buddy domain types, built-in triage and the `Provider` trait.
//!
//! This crate is deliberately free of I/O: no network, no filesystem, no clock. Callers pass the
//! current time in wherever it matters, which keeps everything here unit-testable.

mod error;
mod model;
mod provider;
pub mod triage;

pub use error::{Error, Result};
pub use model::*;
pub use provider::{Capabilities, FeatureAction, Provider};
pub use triage::{Bucket, TriageConfig, TriageOutcome, TriageReason};
