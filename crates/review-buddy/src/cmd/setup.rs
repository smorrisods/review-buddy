//! `review-buddy --setup` as plain prompts: the same first-run flow without the full screen.
//!
//! Used when stdin or stdout isn't a terminal, and for `--setup --plain`. After the config is
//! written it checks each new source's sign-in and says how it went.

use std::io::Write;
use std::sync::Arc;

use super::context::{Context, Terminal};
use super::error::CmdError;
use super::{auth, DEMO_LABEL};
use crate::cli::GlobalArgs;
use crate::setup::{self, plain, Outcome, Services};

pub fn run_plain(args: &GlobalArgs, terminal: Terminal) -> Result<(), CmdError> {
    if args.demo {
        return Err(CmdError::usage(format!(
            "Demo mode doesn't connect real accounts {DEMO_LABEL}.\nRun review-buddy --setup without --demo."
        )));
    }
    let ctx = Context::build(args.clone(), terminal)?;
    let services = Arc::new(Services::system(ctx.env.as_ref(), &ctx.config));
    let flow = setup::new_flow(&ctx.paths, &ctx.config);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let run = plain::run(
        flow,
        &services,
        &mut stdin.lock(),
        &mut stdout.lock(),
        terminal.stdin_tty,
    )?;
    finish(args, terminal, run, &mut stdout.lock())
}

fn finish(
    args: &GlobalArgs,
    terminal: Terminal,
    run: plain::Run,
    out: &mut dyn Write,
) -> Result<(), CmdError> {
    if let Some(reason) = run.failure {
        return Err(CmdError::failed(format!(
            "{reason}\nNothing was changed. Fix that and run review-buddy --setup again."
        )));
    }
    let Outcome::Written {
        path,
        backup,
        sources,
    } = run.outcome
    else {
        return Err(CmdError::Cancelled(
            "Setup stopped. Nothing was changed.\nRun review-buddy --setup when you're ready."
                .into(),
        ));
    };
    writeln!(out, "\nSaved {}", path.display())?;
    if let Some(backup) = backup {
        writeln!(out, "Your old file is kept as {}", backup.display())?;
    }
    writeln!(out, "Checking {} sign-in…", sources.join(", "))?;
    let ctx = Context::build(args.clone(), terminal)?;
    let checks = auth::gather(&ctx)?;
    write!(out, "{}", auth::render_lines(&checks, &ctx.out.painter))?;
    auth::require_signed_in(&checks)?;
    writeln!(out, "You're set. Run review-buddy to open your queue.")?;
    Ok(())
}
