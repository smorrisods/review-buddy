mod cli;

use std::process::ExitCode;

use clap::Parser;

/// Exit code for a command that parsed fine but is not built yet.
const EXIT_NOT_BUILT: u8 = 2;

fn main() -> ExitCode {
    run(cli::Cli::parse())
}

fn run(cli: cli::Cli) -> ExitCode {
    let what = match &cli.command {
        None => "The interface",
        Some(cli::Command::Open { .. }) => "The open command",
        Some(cli::Command::Theme { .. }) => "The theme command",
        Some(cli::Command::Doctor) => "The doctor command",
        Some(cli::Command::Config { .. }) => "The config command",
        Some(cli::Command::Triage { .. }) => "The triage command",
    };
    eprintln!("{what} isn't built yet. Run `review-buddy --help` to see what's available.");
    ExitCode::from(EXIT_NOT_BUILT)
}
