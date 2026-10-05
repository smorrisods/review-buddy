//! Confirmations for writes. They default to No and never wait when there is no terminal.

use std::io::{BufRead, Write};

use super::error::CmdError;

/// Whether the user can be asked, and whether they already said yes with `--yes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interaction {
    pub yes: bool,
    pub interactive: bool,
}

/// Reads one line and answers true only for `y` or `yes`. A blank line, anything else,
/// or the end of input is No.
pub fn confirm(
    question: &str,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> std::io::Result<bool> {
    write!(output, "{question} [y/N] ")?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        writeln!(output)?;
        return Ok(false);
    }
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Gate for a write. `preview` says what is about to happen; it is shown before asking.
///
/// `--yes` goes straight ahead. Without a terminal the write is refused. On a terminal the
/// preview is printed to `output` and the answer must be an explicit yes.
pub fn confirm_write(
    preview: &str,
    interaction: Interaction,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<(), CmdError> {
    if interaction.yes {
        return Ok(());
    }
    if !interaction.interactive {
        return Err(CmdError::Cancelled(format!(
            "{preview}\nNothing was changed. Run it from a terminal to confirm, or pass --yes."
        )));
    }
    writeln!(output, "{preview}")?;
    if confirm("Go ahead?", input, output)? {
        Ok(())
    } else {
        Err(CmdError::Cancelled(
            "Cancelled. Nothing was changed.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(answer: &str) -> (bool, String) {
        let mut out = Vec::new();
        let got = confirm("Merge?", &mut answer.as_bytes(), &mut out).unwrap();
        (got, String::from_utf8(out).unwrap())
    }

    #[test]
    fn only_an_explicit_yes_counts() {
        for yes in ["y\n", "Y\n", "yes\n", " YES \n"] {
            assert!(ask(yes).0, "{yes:?}");
        }
        for no in ["\n", "n\n", "no\n", "yep\n", "sure\n", ""] {
            assert!(!ask(no).0, "{no:?}");
        }
    }

    #[test]
    fn the_prompt_shows_the_default() {
        assert!(ask("y\n").1.starts_with("Merge? [y/N] "));
    }

    #[test]
    fn yes_flag_skips_everything() {
        let mut out = Vec::new();
        let interaction = Interaction {
            yes: true,
            interactive: false,
        };
        confirm_write("Merge a/b#1.", interaction, &mut "".as_bytes(), &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn no_terminal_refuses_with_exit_3() {
        let mut out = Vec::new();
        let interaction = Interaction {
            yes: false,
            interactive: false,
        };
        let err = confirm_write("Merge a/b#1.", interaction, &mut "y\n".as_bytes(), &mut out)
            .unwrap_err();
        assert_eq!(err.exit().code(), 3);
        assert!(err.to_string().contains("--yes"));
        assert!(out.is_empty());
    }

    #[test]
    fn a_terminal_previews_then_asks_and_defaults_to_no() {
        let interaction = Interaction {
            yes: false,
            interactive: true,
        };
        let mut out = Vec::new();
        let err =
            confirm_write("Merge a/b#1.", interaction, &mut "\n".as_bytes(), &mut out).unwrap_err();
        assert_eq!(err.exit().code(), 3);
        let shown = String::from_utf8(out).unwrap();
        assert!(shown.starts_with("Merge a/b#1.\n"));
        assert!(shown.contains("[y/N]"));

        let mut out = Vec::new();
        confirm_write("Merge a/b#1.", interaction, &mut "y\n".as_bytes(), &mut out).unwrap();
    }

    #[test]
    fn end_of_input_is_no() {
        let interaction = Interaction {
            yes: false,
            interactive: true,
        };
        let mut out = Vec::new();
        assert!(confirm_write("x", interaction, &mut "".as_bytes(), &mut out).is_err());
    }
}
