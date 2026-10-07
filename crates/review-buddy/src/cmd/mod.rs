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

pub(crate) mod auth;
mod auth_login;
mod auth_logout;
mod auth_token;
mod changes;
mod completion;
mod config;
mod config_get;
mod config_reset;
mod doctor;
mod drafts;
pub(crate) mod host;
mod markdown;
mod open;
mod pr_checks;
mod pr_diff;
mod pr_list;
mod pr_view;
pub(crate) mod probe;
mod queue;
mod setup;
mod source;
mod source_add;
mod source_test;
mod stub;
mod theme;

use std::process::ExitCode;

use crate::cli::{
    AuthAction, Cli, Command, ConfigAction, DraftsAction, PrAction, SourceAction, ThemeAction,
};
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

/// Runs `--setup` as plain prompts. `Ok` means a config was written and the sign-ins were checked.
pub fn run_setup(cli: &Cli, terminal: Terminal) -> Result<(), CmdError> {
    setup::run_plain(&cli.global, terminal)
}

fn execute(cli: Cli, terminal: Terminal) -> Result<(), CmdError> {
    let Some(command) = cli.command else {
        return Err(CmdError::usage("No command given."));
    };
    if let Some(milestone) = stub::milestone(&command) {
        return Err(CmdError::NotBuilt { milestone });
    }
    if let Command::Completion { shell } = &command {
        return completion::run(*shell);
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
        Command::Auth {
            action: AuthAction::Login { host, with_token },
        } => auth_login::run(&ctx, host.as_deref(), with_token),
        Command::Auth {
            action: AuthAction::Logout { host },
        } => auth_logout::run(&ctx, host.as_deref()),
        Command::Auth {
            action: AuthAction::Token { host, show },
        } => auth_token::run(&ctx, host.as_deref(), show),
        Command::Config {
            action: ConfigAction::Get { key },
        } => config_get::get(&ctx, &key),
        Command::Config {
            action: ConfigAction::List,
        } => config_get::list(&ctx),
        Command::Config {
            action: ConfigAction::ResetLayout,
        } => config_reset::run(&ctx),
        Command::Drafts {
            action: DraftsAction::List,
        } => drafts::list(&ctx),
        Command::Drafts {
            action: DraftsAction::Discard { selector },
        } => drafts::discard(&ctx, &selector),
        Command::Drafts {
            action: DraftsAction::Clear,
        } => drafts::clear(&ctx),
        Command::Source {
            action: SourceAction::List,
        } => source::list(&ctx),
        Command::Source {
            action: SourceAction::Test { name, require },
        } => source_test::run(&ctx, name.as_deref(), &require),
        Command::Source {
            action: SourceAction::Add(args),
        } => source_add::run(&ctx, &args),
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
        let err = execute(parse(&["triage", "explain", "x"]), Terminal::detect()).unwrap_err();
        assert_eq!(err.exit(), Exit::Usage);
        assert!(err.to_string().starts_with("Not built yet."));
    }
}
