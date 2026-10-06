//! What the interface knows about refreshing: each source's state, and the times and glyphs
//! shown for it. The scheduling itself lives in the runtime (`providers::refresh`).

use rb_core::Timestamp;

use super::{AppState, SourceFailure};
use crate::app::failure::FailureKind;

/// Where one source is in its refresh cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceStatus {
    #[default]
    Idle,
    Refreshing,
    Ok,
    /// Couldn't reach the host; the last good rows are still shown.
    Offline {
        since: Timestamp,
    },
    /// The host asked for a pause; nothing is sent until `until`.
    RateLimited {
        until: Timestamp,
    },
    AuthFailed,
    /// Something else went wrong that retrying on its own won't fix.
    Failed,
}

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// A spinner frame for the tick count, or a still glyph under reduced motion.
pub fn spinner(ticks: u64, reduced_motion: bool) -> &'static str {
    if reduced_motion {
        "…"
    } else {
        FRAMES[(ticks % FRAMES.len() as u64) as usize]
    }
}

/// `HH:MM` in UTC.
pub fn hhmm(at: Timestamp) -> String {
    let secs = at.0.rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

impl AppState {
    pub fn is_refreshing(&self, source: &rb_core::SourceId) -> bool {
        self.refreshing.contains(source)
    }

    /// The state a source's row should show: refreshing wins, then the last reported state.
    pub fn status_of(&self, source: &rb_core::SourceId) -> SourceStatus {
        if self.is_refreshing(source) {
            return SourceStatus::Refreshing;
        }
        self.statuses.get(source).copied().unwrap_or_default()
    }

    /// The time to show in the `offline · cached HH:MM` banner: every source is failing to
    /// connect, and there is something cached to look at.
    pub fn offline_since_cache(&self) -> Option<Timestamp> {
        let all_offline = !self.sources.is_empty()
            && self.sources.iter().all(|s| {
                self.failures
                    .get(&s.id)
                    .is_some_and(|f| f.kind == FailureKind::Offline)
            });
        if all_offline && !self.changes.is_empty() {
            self.last_refreshed
        } else {
            None
        }
    }
}

/// The note for a source's second line when something is worth saying, in calm words.
pub fn status_note(status: SourceStatus, failure: Option<&SourceFailure>) -> Option<String> {
    match status {
        SourceStatus::RateLimited { until } => Some(format!("paused until {}", hhmm(until))),
        _ => failure.map(|f| f.short().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_zero_padded_utc() {
        assert_eq!(hhmm(Timestamp(0)), "00:00");
        assert_eq!(hhmm(Timestamp(10 * 3600 + 42 * 60 + 59)), "10:42");
        assert_eq!(hhmm(Timestamp(86_400 + 61)), "00:01");
    }

    #[test]
    fn spinner_moves_unless_motion_is_reduced() {
        assert_ne!(spinner(0, false), spinner(1, false));
        assert_eq!(spinner(0, true), spinner(5, true));
        assert_eq!(spinner(0, false), spinner(10, false));
    }

    #[test]
    fn a_rate_limit_note_names_the_time() {
        let note = status_note(
            SourceStatus::RateLimited {
                until: Timestamp(3 * 3600 + 5 * 60),
            },
            None,
        );
        assert_eq!(note.as_deref(), Some("paused until 03:05"));
        assert_eq!(status_note(SourceStatus::Ok, None), None);
    }
}
