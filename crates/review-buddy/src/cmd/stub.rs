//! Commands that are declared in `cli.rs` but not built yet, and when they're planned.

use crate::cli::{AuthAction, Command, ConfigAction, PrAction, SourceAction, ThemeAction};

/// The milestone a declared-but-unbuilt command is planned for, or `None` if it runs today.
pub fn milestone(command: &Command) -> Option<&'static str> {
    Some(match command {
        Command::Config {
            action: ConfigAction::Paths,
        }
        | Command::Theme {
            action: ThemeAction::List,
        }
        | Command::Pr {
            action: PrAction::Diff { .. },
        }
        | Command::Pr {
            action: PrAction::Checks { .. },
        }
        | Command::Auth {
            action: AuthAction::Status,
        }
        | Command::Source {
            action: SourceAction::List,
        }
        | Command::Doctor => return None,
        Command::Queue(_) => return None,
        Command::Config { .. } => "v0.2.0",
        Command::Theme { .. } => "v0.1.0",
        Command::Open { .. } | Command::Completion { .. } | Command::Triage { .. } => "v0.1.0",
        Command::Pr { action } => match action {
            PrAction::List(_) => return None,
            PrAction::View { .. } | PrAction::Checks { .. } | PrAction::Open { .. } => "v0.1.0",
            PrAction::Diff { .. } => return None,
        },
    })
}
