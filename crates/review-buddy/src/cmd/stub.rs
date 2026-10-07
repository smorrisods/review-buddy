//! Commands that are declared in `cli.rs` but not built yet, and when they're planned.

use crate::cli::{Command, ThemeAction};

/// The milestone a declared-but-unbuilt command is planned for, or `None` if it runs today.
pub fn milestone(command: &Command) -> Option<&'static str> {
    Some(match command {
        Command::Config { .. }
        | Command::Theme {
            action: ThemeAction::List,
        }
        | Command::Pr { .. }
        | Command::Queue(_)
        | Command::Auth { .. }
        | Command::Source { .. }
        | Command::Drafts { .. }
        | Command::Open { .. }
        | Command::Doctor
        | Command::Completion { .. } => return None,
        Command::Theme { .. } => "v0.1.0",
        Command::Triage { .. } => "v0.1.0",
    })
}
