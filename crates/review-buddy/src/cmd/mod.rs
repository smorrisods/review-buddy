//! Commands: the non-interactive face of `review-buddy`. A command never enters the TUI.
//!
//! One module per noun. Each command is resolve, call a `Provider`, format; the shared plumbing
//! is in [`context`] and [`output`].

pub mod context;
pub mod error;
pub mod git;
pub mod output;
pub mod prompt;
pub mod selector;

mod config;
mod stub;
mod theme;

use std::process::ExitCode;

use crate::cli::{Cli, Command, ConfigAction, ThemeAction};
use context::{Context, Terminal};
use error::{CmdError, Exit};

/// Marks output that came from demo mode.
pub const DEMO_LABEL: &str = "(demo)";

/// Runs a command and returns its exit code. Failures print one or two calm lines to stderr.
pub fn run(cli: Cli) -> ExitCode {
    match execute(cli, Terminal::detect()) {
        Ok(()) => Exit::Success.into(),
        Err(err) => {
            eprintln!("{err}");
            err.exit().into()
        }
    }
}

fn execute(cli: Cli, terminal: Terminal) -> Result<(), CmdError> {
    let Some(command) = cli.command else {
        return Err(CmdError::usage("No command given."));
    };
    if let Some(milestone) = stub::milestone(&command) {
        return Err(CmdError::NotBuilt { milestone });
    }
    let ctx = Context::build(cli.global, terminal)?;
    match command {
        Command::Config {
            action: ConfigAction::Paths,
        } => config::paths(&ctx),
        Command::Theme {
            action: ThemeAction::List,
        } => theme::list(&ctx),
        other => unreachable!("{other:?} has no milestone but no implementation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("review-buddy").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn unbuilt_commands_exit_2_before_touching_anything() {
        for args in [
            &["queue"][..],
            &["pr", "list"],
            &["mr", "view", "!1"],
            &["doctor"],
        ] {
            let err = execute(parse(args), Terminal::detect()).unwrap_err();
            assert_eq!(err.exit(), Exit::Usage, "{args:?}");
            assert!(err.to_string().starts_with("Not built yet."));
        }
    }
}
