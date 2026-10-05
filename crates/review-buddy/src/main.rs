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
        None => return launch(&cli),
        Some(cli::Command::Open { .. }) => "The open command",
        Some(cli::Command::Theme { .. }) => "The theme command",
        Some(cli::Command::Doctor) => "The doctor command",
        Some(cli::Command::Config { .. }) => "The config command",
        Some(cli::Command::Triage { .. }) => "The triage command",
    };
    eprintln!("{what} isn't built yet. Run `review-buddy --help` to see what's available.");
    ExitCode::from(EXIT_UNAVAILABLE)
}

fn launch(cli: &cli::Cli) -> ExitCode {
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        eprintln!(
            "Review Buddy needs an interactive terminal to draw in. Run it from a terminal, or see `review-buddy --help` for the command line."
        );
        return ExitCode::from(EXIT_UNAVAILABLE);
    }
    let options = match run_options(cli) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{err:#}");
            return ExitCode::from(EXIT_UNAVAILABLE);
        }
    };
    match runtime::run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Review Buddy couldn't start the interface: {err:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "demo")]
fn run_options(cli: &cli::Cli) -> anyhow::Result<runtime::RunOptions> {
    let demo = if cli.demo {
        Some(review_buddy::demo::Demo::start(cli.frozen_time.as_deref())?)
    } else {
        None
    };
    Ok(runtime::RunOptions { demo })
}

#[cfg(not(feature = "demo"))]
fn run_options(cli: &cli::Cli) -> anyhow::Result<runtime::RunOptions> {
    if cli.demo {
        anyhow::bail!(
            "This build doesn't include demo mode. Install a release build, or rebuild with the `demo` feature."
        );
    }
    Ok(runtime::RunOptions::default())
}
