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

fn run_options(cli: &cli::Cli) -> anyhow::Result<runtime::RunOptions> {
    #[allow(unused_mut)]
    let mut options = runtime::RunOptions::default();
    if cli.global.demo {
        #[cfg(feature = "demo")]
        {
            options.demo = Some(review_buddy::demo::Demo::start(
                cli.global.frozen_time.as_deref(),
            )?);
        }
        #[cfg(not(feature = "demo"))]
        anyhow::bail!(
            "This build doesn't include demo mode. Install a release build, or rebuild with the `demo` feature."
        );
    } else {
        #[cfg(feature = "live")]
        {
            let ctx =
                cmd::context::Context::build(cli.global.clone(), cmd::context::Terminal::detect())?;
            if let Some(problem) = ctx.config_problem() {
                anyhow::bail!("{problem}\nFix the config file, or see review-buddy config paths.");
            }
            options.settings = Some(runtime::Settings::from_config(&ctx.config));
            options.live = Some(std::sync::Arc::new(
                review_buddy::providers::Live::from_context(&ctx)?,
            ));
        }
    }
    Ok(options)
}
