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

mod auth;
mod changes;
mod config;
mod doctor;
mod markdown;
mod open;
mod pr_checks;
mod pr_diff;
mod pr_list;
mod pr_view;
mod queue;
mod source;
mod stub;
mod theme;

use std::process::ExitCode;

use crate::cli::{AuthAction, Cli, Command, ConfigAction, PrAction, SourceAction, ThemeAction};
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
        Command::Pr {
            action:
                PrAction::Diff {
                    selector,
                    name_only,
                    stat,
                    file,
                    patch,
                },
        } => pr_diff::run(
            &ctx,
            selector.as_deref(),
            &pr_diff::Options {
                name_only,
                stat,
                files: file,
                raw: patch,
            },
        ),
        Command::Queue(args) => queue::run(&ctx, &args),
        Command::Pr {
            action: PrAction::List(args),
        } => pr_list::run(&ctx, &args),
        Command::Pr {
            action:
                PrAction::Checks {
                    selector,
                    watch,
                    interval,
                    fail_fast,
                    required,
                },
        } => pr_checks::run(
            &ctx,
            selector.as_deref(),
            &pr_checks::Options {
                watch,
                interval,
                fail_fast,
                required,
            },
        ),
        Command::Auth {
            action: AuthAction::Status,
        } => auth::status(&ctx),
        Command::Source {
            action: SourceAction::List,
        } => source::list(&ctx),
        Command::Doctor => doctor::run(&ctx),
        Command::Open { selector } => open::run(&ctx, selector.as_deref(), terminal),
        Command::Pr {
            action: PrAction::View {
                selector, comments, ..
            },
        } => pr_view::run(
            &ctx,
            &pr_view::ViewOptions {
                selector,
                comments,
                web: false,
            },
        ),
        Command::Pr {
            action: PrAction::Open { selector },
        } => pr_view::run(
            &ctx,
            &pr_view::ViewOptions {
                selector,
                comments: false,
                web: true,
            },
        ),
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
            &["triage", "explain", "x"][..],
            &["config", "get", "ui.theme"],
        ] {
            let err = execute(parse(args), Terminal::detect()).unwrap_err();
            assert_eq!(err.exit(), Exit::Usage, "{args:?}");
            assert!(err.to_string().starts_with("Not built yet."));
        }
    }
}
