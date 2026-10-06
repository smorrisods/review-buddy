//! First run: finding the accounts someone already uses, connecting them, and writing a
//! commented `config.toml`.
//!
//! The pieces, from the inside out:
//!
//! - [`detect`] finds hosts from `gh`/`glab` sign-ins, git config and local repositories.
//! - [`flow`] is the state machine both front ends drive. Its transitions are pure.
//! - [`effects`] runs what the flow asks for: token checks, org listing, the config write.
//! - [`write`] renders and writes the file; [`source`] holds [`add_source`] for anything that
//!   wants to append one account to a config.
//! - [`plain`] is the line-based front end. The full-screen one lives in `app::setup` and
//!   `ui::first_run`.

pub mod detect;
pub mod effects;
pub mod flow;
pub mod plain;
pub mod source;
pub mod write;

use rb_paths::PathsReport;
use rb_theme::DEFAULT_THEME_ID;

pub use detect::{Detection, Evidence, FoundHost, Roots};
pub use effects::{run_effect, Services};
pub use flow::{Click, Conn, Credential, Effect, Flow, Input, Outcome, Plan, ProbeInfo, Step};
pub use source::{add_source, AddError, AuthKind, SourceSpec};
pub use write::{render, write, WriteError};

use crate::config::Config;

/// Whether a plain launch should open first run: no config file in any location and no sources.
pub fn needs_first_run(paths: &PathsReport, config: &Config) -> bool {
    !paths.config_files.iter().any(|f| f.exists) && config.sources.is_empty()
}

/// A fresh flow aimed at the layered write target. A config that is already there is kept
/// unless the person confirms replacing it.
pub fn new_flow(paths: &PathsReport, config: &Config) -> Flow {
    let existing = paths.write_target.exists();
    let theme = if existing {
        config.ui.theme.as_str()
    } else {
        DEFAULT_THEME_ID
    };
    Flow::new(paths.write_target.clone(), existing, theme, config.ui.jax)
        .with_api_urls(effects::api_urls(config).into_iter().collect())
}
