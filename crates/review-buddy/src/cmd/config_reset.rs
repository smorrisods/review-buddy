//! `review-buddy config reset-layout`: forget the remembered panel layout.

use std::io::Write;
use std::path::Path;

use super::context::Context;
use super::error::CmdError;
use super::output;
use super::prompt::{confirm_write, Interaction};
use super::DEMO_LABEL;

/// Removes the remembered layout at `path` after the confirmation. Returns what to print.
pub fn reset(
    path: &Path,
    interaction: Interaction,
    input: &mut dyn std::io::BufRead,
    prompt_out: &mut dyn Write,
) -> Result<String, CmdError> {
    if !path.exists() {
        return Ok(format!(
            "There's no remembered layout at {}, so nothing was changed.\n",
            path.display()
        ));
    }
    let preview = format!(
        "This removes the remembered panel layout at {}.\nYour config.toml isn't touched, and the next run starts from it.",
        path.display()
    );
    confirm_write(&preview, interaction, input, prompt_out)?;
    std::fs::remove_file(path).map_err(|e| {
        CmdError::failed(format!(
            "Couldn't remove {}: {e}.\nCheck the file's permissions, or delete it yourself.",
            path.display()
        ))
    })?;
    Ok(format!(
        "Removed the remembered layout at {}. The next run starts from your config.\n",
        path.display()
    ))
}

pub fn run(ctx: &Context) -> Result<(), CmdError> {
    let path = ctx.paths.paths.session_file();
    if ctx.is_demo() {
        return output::print(&format!(
            "Would remove the remembered layout at {} {DEMO_LABEL}\nNothing was changed.\n",
            path.display()
        ));
    }
    let stdin = std::io::stdin();
    let text = reset(
        &path,
        ctx.interaction,
        &mut stdin.lock(),
        &mut std::io::stdout(),
    )?;
    output::print(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.toml");
        std::fs::write(&path, "[layout]\n").unwrap();
        (dir, path)
    }

    fn ask(path: &Path, yes: bool, interactive: bool, answer: &str) -> Result<String, CmdError> {
        reset(
            path,
            Interaction { interactive, yes },
            &mut answer.as_bytes(),
            &mut Vec::new(),
        )
    }

    #[test]
    fn yes_removes_the_file() {
        let (_dir, path) = file();
        let text = ask(&path, true, false, "").unwrap();
        assert!(text.starts_with("Removed"));
        assert!(!path.exists());
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let text = ask(&dir.path().join("session.toml"), false, false, "").unwrap();
        assert!(text.contains("nothing was changed"));
    }

    #[test]
    fn without_a_terminal_it_needs_yes() {
        let (_dir, path) = file();
        let err = ask(&path, false, false, "y\n").unwrap_err();
        assert_eq!(err.exit(), super::super::error::Exit::Cancelled);
        assert!(err.to_string().contains("--yes"));
        assert!(path.exists());
    }

    #[test]
    fn on_a_terminal_only_an_explicit_yes_removes_it() {
        let (_dir, path) = file();
        for answer in ["\n", "n\n", ""] {
            assert!(ask(&path, false, true, answer).is_err(), "{answer:?}");
            assert!(path.exists());
        }
        ask(&path, false, true, "y\n").unwrap();
        assert!(!path.exists());
    }
}
