//! `review-buddy open <selector>`: start the interface on one change's diff.

use rb_core::ChangeId;

use super::context::{Context, Terminal};
use super::error::CmdError;
use super::pr_view;
use crate::runtime::{self, RunOptions};

pub fn run(ctx: &Context, selector: Option<&str>, terminal: Terminal) -> Result<(), CmdError> {
    let id = resolve(ctx, selector)?;
    if !terminal.stdout_tty || !terminal.stdin_tty {
        return Err(CmdError::usage(
            "Review Buddy needs an interactive terminal to draw in.\nRun this from a terminal, or use review-buddy pr view for plain text.",
        ));
    }
    let options = run_options(ctx, id)?;
    runtime::run(options)
        .map_err(|e| CmdError::failed(format!("Review Buddy couldn't start the interface: {e:#}")))
}

/// Finds the change the selector names, checking with the forge that it exists.
pub fn resolve(ctx: &Context, selector: Option<&str>) -> Result<ChangeId, CmdError> {
    let target = ctx.resolve_selector(selector)?;
    let source = pr_view::find_source(ctx, &target)?;
    let provider = ctx.provider_for(&source)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CmdError::failed(format!("Couldn't start the async runtime: {e}.")))?;
    runtime.block_on(async {
        let id = pr_view::find_id(provider.as_ref(), &source, &target).await?;
        provider.change_detail(&id).await?;
        Ok(id)
    })
}

fn run_options(ctx: &Context, id: ChangeId) -> Result<RunOptions, CmdError> {
    #[allow(unused_mut)]
    let mut options = RunOptions::default();
    if ctx.is_demo() {
        #[cfg(feature = "demo")]
        {
            options.demo = Some(
                crate::demo::Demo::start(ctx.args.frozen_time.as_deref())
                    .map_err(|e| CmdError::usage(format!("{e:#}")))?,
            );
        }
    } else {
        #[cfg(feature = "live")]
        {
            options.live = Some(std::sync::Arc::new(
                crate::providers::Live::from_context(ctx)
                    .map_err(|e| CmdError::failed(format!("{e:#}")))?,
            ));
        }
    }
    options.open = Some(id);
    Ok(options)
}
