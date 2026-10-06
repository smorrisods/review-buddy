//! `review-buddy auth logout`: remove the token review-buddy stored in the OS keyring.
//!
//! `gh` and `glab` sign-ins are never touched.

// Without `live` there is no network, so some of this is only reachable from tests.
#![cfg_attr(not(feature = "live"), allow(dead_code, unused_imports))]

use std::io::Write;

use rb_platform::SecretStore;

use super::context::Context;
use super::error::CmdError;
use super::host;
use super::output;
use super::prompt::{confirm_write, Interaction};
use super::DEMO_LABEL;

const LEFT_ALONE: &str = "Your gh and glab sign-ins were left alone.";

/// Removes the stored token for `host` after the confirmation. Returns what to print.
pub fn sign_out(
    host: &str,
    store: &dyn SecretStore,
    interaction: Interaction,
    input: &mut dyn std::io::BufRead,
    prompt_out: &mut dyn Write,
) -> Result<String, CmdError> {
    let stored = store.get(host).map_err(|e| {
        CmdError::failed(format!(
            "Couldn't read the OS keyring: {e}.\nCheck that your keyring is unlocked, then try again."
        ))
    })?;
    if stored.is_none() {
        return Ok(format!(
            "There's no token for {host} in the OS keyring, so nothing was changed.\n{LEFT_ALONE}\n"
        ));
    }
    let preview = format!(
        "This removes the token review-buddy stored in the OS keyring for {host}.\nYour gh and glab sign-ins aren't touched."
    );
    confirm_write(&preview, interaction, input, prompt_out)?;
    store.delete(host).map_err(|e| {
        CmdError::failed(format!(
            "Couldn't remove the token from the OS keyring: {e}.\nRemove review-buddy/{host} in your keyring app instead."
        ))
    })?;
    Ok(format!(
        "Removed the stored token for {host}.\n{LEFT_ALONE}\n"
    ))
}

pub fn run(ctx: &Context, host: Option<&str>) -> Result<(), CmdError> {
    let target = host::resolve(ctx, host)?;
    if ctx.is_demo() {
        return output::print(&format!(
            "Would remove the stored token for {} from the OS keyring {DEMO_LABEL}\nNothing was changed. {LEFT_ALONE}\n",
            target.host
        ));
    }
    #[cfg(not(feature = "live"))]
    {
        let _ = (sign_out, LEFT_ALONE);
        Err(host::no_network())
    }
    #[cfg(feature = "live")]
    {
        let store = crate::providers::system_store();
        let stdin = std::io::stdin();
        let text = sign_out(
            &target.host,
            store.as_ref(),
            ctx.interaction,
            &mut stdin.lock(),
            &mut std::io::stdout(),
        )?;
        super::probe::forget(ctx, target.kind, &target.host);
        output::print(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_platform::{MemorySecretStore, Secret};

    fn store() -> MemorySecretStore {
        let store = MemorySecretStore::new();
        store.set("github.com", &Secret::new("ghp_x")).unwrap();
        store
    }

    fn ask(
        store: &MemorySecretStore,
        yes: bool,
        interactive: bool,
        answer: &str,
    ) -> Result<String, CmdError> {
        sign_out(
            "github.com",
            store,
            Interaction { yes, interactive },
            &mut answer.as_bytes(),
            &mut Vec::new(),
        )
    }

    #[test]
    fn default_is_no_on_a_terminal() {
        let s = store();
        let err = ask(&s, false, true, "\n").unwrap_err();
        assert_eq!(err.exit().code(), 3);
        assert!(s.get("github.com").unwrap().is_some());
    }

    #[test]
    fn yes_answer_or_flag_removes_it_and_says_gh_is_untouched() {
        for (yes, interactive, answer) in [(false, true, "y\n"), (true, false, "")] {
            let s = store();
            let text = ask(&s, yes, interactive, answer).unwrap();
            assert!(text.contains("Removed the stored token for github.com"));
            assert!(text.contains("gh and glab sign-ins were left alone"));
            assert!(s.get("github.com").unwrap().is_none());
        }
    }

    #[test]
    fn no_terminal_needs_yes() {
        let s = store();
        let err = ask(&s, false, false, "y\n").unwrap_err();
        assert_eq!(err.exit().code(), 3);
        assert!(err.to_string().contains("--yes"));
        assert!(s.get("github.com").unwrap().is_some());
    }

    #[test]
    fn nothing_stored_is_a_calm_no_op() {
        let s = MemorySecretStore::new();
        let text = ask(&s, false, false, "").unwrap();
        assert!(text.contains("nothing was changed"));
    }
}
