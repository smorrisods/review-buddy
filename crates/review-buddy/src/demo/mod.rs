//! Demo mode: offline fixtures, an in-memory provider, and throwaway directories.
//!
//! Nothing here reads or writes the real config, cache or state, and nothing opens a socket.

mod env;
mod fixtures;
mod provider;
mod time;

pub use env::DemoEnv;
pub use fixtures::{count_changes, load as load_fixtures, DemoChange, Fixtures};
pub use provider::{DemoProvider, DemoWorld, DEMO_SUFFIX};
pub use time::{parse_iso, DEFAULT_FROZEN};

use anyhow::{anyhow, Result};
use rb_core::Timestamp;

/// Everything a demo session holds: the throwaway directories and the in-memory world.
/// The directories are removed when this is dropped.
#[derive(Debug)]
pub struct Demo {
    pub env: DemoEnv,
    pub world: DemoWorld,
}

impl Demo {
    /// Starts a demo session. `frozen` is an ISO 8601 time (`--frozen-time`); without one the
    /// fixtures are dated relative to the real clock.
    pub fn start(frozen: Option<&str>) -> Result<Self> {
        let now = match frozen {
            Some(text) => parse_iso(text).ok_or_else(|| {
                anyhow!("couldn't read `{text}` as a time. Try a form like 2026-10-05T10:00.")
            })?,
            None => real_now(),
        };
        Ok(Self {
            env: DemoEnv::new()?,
            world: DemoWorld::new(now)?,
        })
    }
}

fn real_now() -> Timestamp {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Timestamp(i64::try_from(secs).unwrap_or(i64::MAX))
}
