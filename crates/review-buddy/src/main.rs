use std::io::IsTerminal;
use std::process::ExitCode;

use clap::Parser;
use review_buddy::{cli, cmd, runtime};

/// Exit code for an interface that was started without a terminal.
const EXIT_UNAVAILABLE: u8 = 2;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    if cli.command.is_some() {
        return cmd::run(cli);
    }
    launch(&cli)
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
    let demo = if cli.global.demo {
        Some(review_buddy::demo::Demo::start(
            cli.global.frozen_time.as_deref(),
        )?)
    } else {
        None
    };
    Ok(runtime::RunOptions { demo })
}

#[cfg(not(feature = "demo"))]
fn run_options(cli: &cli::Cli) -> anyhow::Result<runtime::RunOptions> {
    if cli.global.demo {
        anyhow::bail!(
            "This build doesn't include demo mode. Install a release build, or rebuild with the `demo` feature."
        );
    }
    Ok(runtime::RunOptions::default())
}
