//! Commands that are declared in `cli.rs` but not built yet, and when they're planned.

use crate::cli::{AuthAction, Command, ConfigAction, SourceAction, ThemeAction};

/// The milestone a declared-but-unbuilt command is planned for, or `None` if it runs today.
pub fn milestone(command: &Command) -> Option<&'static str> {
    Some(match command {
        Command::Config {
            action: ConfigAction::Paths,
        }
        | Command::Theme {
            action: ThemeAction::List,
        }
        | Command::Pr { .. }
        | Command::Queue(_)
        | Command::Auth {
            action: AuthAction::Status,
        }
        | Command::Source {
            action: SourceAction::List,
        }
        | Command::Open { .. }
        | Command::Doctor
        | Command::Completion { .. } => return None,
        Command::Config { .. } => "v0.2.0",
        Command::Theme { .. } => "v0.1.0",
        Command::Triage { .. } => "v0.1.0",
    })
}
