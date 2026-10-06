//! `review-buddy completion <shell>`: prints a completion script. No config, network or TUI.

use std::io::Write;

use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::{Cli, ShellArg};

use super::error::CmdError;

fn shell(arg: ShellArg) -> Shell {
    match arg {
        ShellArg::Bash => Shell::Bash,
        ShellArg::Zsh => Shell::Zsh,
        ShellArg::Fish => Shell::Fish,
        ShellArg::Elvish => Shell::Elvish,
        ShellArg::Powershell => Shell::PowerShell,
    }
}

pub fn script(arg: ShellArg) -> Vec<u8> {
    let mut buffer = Vec::new();
    clap_complete::generate(shell(arg), &mut Cli::command(), "review-buddy", &mut buffer);
    buffer
}

pub fn run(arg: ShellArg) -> Result<(), CmdError> {
    // A closed pipe (`| head`) is not an error worth reporting.
    let _ = std::io::stdout().write_all(&script(arg));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shell_script_is_non_empty_and_names_commands_and_flags() {
        for arg in [
            ShellArg::Bash,
            ShellArg::Zsh,
            ShellArg::Fish,
            ShellArg::Elvish,
            ShellArg::Powershell,
        ] {
            let text = String::from_utf8(script(arg)).unwrap();
            assert!(!text.is_empty(), "{arg:?}");
            for word in ["queue", "pr", "auth", "completion", "demo"] {
                assert!(text.contains(word), "{arg:?} lacks {word}");
            }
        }
    }
}
