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
    if cli.setup && !cli.global.demo {
        let terminal = cmd::context::Terminal::detect();
        if cli.plain || !terminal.stdout_tty || !terminal.stdin_tty {
            return match cmd::run_setup(&cli, terminal) {
                Ok(()) if terminal.stdout_tty && terminal.stdin_tty => {
                    let mut cli = cli;
                    cli.setup = false;
                    launch(&cli)
                }
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("{err}");
                    err.exit().into()
                }
            };
        }
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
            if let Some(problem) = ctx.config_problem().filter(|_| !cli.setup) {
                anyhow::bail!("{problem}\nFix the config file, or see review-buddy config paths.");
            }
            if cli.setup || review_buddy::setup::needs_first_run(&ctx.paths, &ctx.config) {
                let services = std::sync::Arc::new(review_buddy::setup::Services::system(
                    ctx.env.as_ref(),
                    &ctx.config,
                ));
                options.setup = Some((
                    review_buddy::setup::new_flow(&ctx.paths, &ctx.config),
                    services,
                ));
                let args = cli.global.clone();
                options.reload = Some(runtime::Reload(std::sync::Arc::new(move || {
                    let ctx = cmd::context::Context::build(
                        args.clone(),
                        cmd::context::Terminal::detect(),
                    )?;
                    if let Some(problem) = ctx.config_problem() {
                        anyhow::bail!("{problem}");
                    }
                    let live =
                        std::sync::Arc::new(review_buddy::providers::Live::from_context(&ctx)?);
                    Ok((launch_settings(&ctx), live))
                })));
            }
            let setup_services = std::sync::Arc::new(review_buddy::setup::Services::system(
                ctx.env.as_ref(),
                &ctx.config,
            ));
            options.settings_services = Some(std::sync::Arc::new(
                review_buddy::settings::Services::system(cli.global.clone(), setup_services),
            ));
            if options.reload.is_none() {
                let args = cli.global.clone();
                options.reload = Some(runtime::Reload(std::sync::Arc::new(move || {
                    let ctx = cmd::context::Context::build(
                        args.clone(),
                        cmd::context::Terminal::detect(),
                    )?;
                    if let Some(problem) = ctx.config_problem() {
                        anyhow::bail!("{problem}");
                    }
                    let live =
                        std::sync::Arc::new(review_buddy::providers::Live::from_context(&ctx)?);
                    Ok((launch_settings(&ctx), live))
                })));
            }
            options.settings = Some(launch_settings(&ctx));
            options.no_mouse = !ctx.config.ui.mouse;
            options.live = Some(std::sync::Arc::new(
                review_buddy::providers::Live::from_context(&ctx)?,
            ));
        }
    }
    Ok(options)
}

/// The settings for a live run: the config, then the remembered layout on top unless
/// `ui.remember_layout` is off.
#[cfg(feature = "live")]
fn launch_settings(ctx: &cmd::context::Context) -> runtime::Settings {
    let settings = runtime::Settings::from_config(&ctx.config)
        .with_write_target(ctx.paths.write_target.clone());
    if !ctx.config.ui.remember_layout {
        return settings;
    }
    let path = review_buddy::session::file_in(&ctx.paths.paths.state_dir);
    let loaded = review_buddy::session::load(&path);
    settings.with_session(path, loaded, ctx.env.as_ref())
}
