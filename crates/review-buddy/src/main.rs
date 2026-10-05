use std::io::IsTerminal;
use std::process::ExitCode;

use clap::Parser;
use review_buddy::{cli, runtime};

/// Exit code for a command that parsed fine but cannot run here (not built yet,
/// or the interface was started without a terminal).
const EXIT_UNAVAILABLE: u8 = 2;

fn main() -> ExitCode {
    run(cli::Cli::parse())
}

fn run(cli: cli::Cli) -> ExitCode {
    let what = match &cli.command {
        None => return launch(),
        Some(cli::Command::Open { .. }) => "The open command",
        Some(cli::Command::Theme { .. }) => "The theme command",
        Some(cli::Command::Doctor) => "The doctor command",
        Some(cli::Command::Config { .. }) => "The config command",
        Some(cli::Command::Triage { .. }) => "The triage command",
    };
    eprintln!("{what} isn't built yet. Run `review-buddy --help` to see what's available.");
    ExitCode::from(EXIT_UNAVAILABLE)
}

fn launch() -> ExitCode {
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        eprintln!(
            "Review Buddy needs an interactive terminal to draw in. Run it from a terminal, or see `review-buddy --help` for the command line."
        );
        return ExitCode::from(EXIT_UNAVAILABLE);
    }
    match runtime::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Review Buddy couldn't start the interface: {err:#}");
            ExitCode::FAILURE
        }
    }
}
