//! What `queue` and `pr list` share: loading changes from every source, the JSON shape of a
//! change, and the cells of a list row.

use std::collections::HashMap;
use std::sync::Arc;

use crate::app::queue::queue_settings;
use rb_core::triage::{triage, TriageConfig, TriageOutcome};
use rb_core::{ChangeSummary, CiState, Provider, Source, SourceId, Timestamp};
use serde::Serialize;
use serde_json::{json, Value};

use super::context::Context;
use super::error::CmdError;
use super::output::Cell;
use crate::ui::dashboard::ci_look;

/// Every field `--json` can name for a listed change.
pub const FIELDS: &[&str] = &[
    "source",
    "forge",
    "host",
    "repo",
    "number",
    "ref",
    "url",
    "title",
    "author",
    "state",
    "isDraft",
    "createdAt",
    "updatedAt",
    "headRefName",
    "baseRefName",
    "headSha",
    "additions",
    "deletions",
    "changedFiles",
    "ci",
    "reviewers",
    "myRole",
    "myReview",
    "bucket",
    "bucketReason",
    "labels",
];

/// The changes of every selected source, with what's needed to describe them.
pub struct Loaded {
    pub changes: Vec<ChangeSummary>,
    pub sources: Vec<Source>,
    pub now: Timestamp,
    pub triage_config: TriageConfig,
    providers: HashMap<SourceId, Arc<dyn Provider>>,
    me: HashMap<SourceId, String>,
}

/// Fetches the changes of every selected source. `with_me` also asks each forge who you are,
/// for `@me`.
pub fn load(ctx: &Context, with_me: bool) -> Result<Loaded, CmdError> {
    let sources = ctx.sources()?;
    if sources.is_empty() {
        return Err(CmdError::usage(
            "No sources are configured yet.\nRun review-buddy --setup or review-buddy source add, or try --demo.",
        ));
    }
    let mut providers = HashMap::new();
    for source in &sources {
        providers.insert(source.id.clone(), ctx.provider_for(source)?);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let fetched = runtime.block_on(futures_util::future::join_all(sources.iter().map(
        |source| {
            let provider = providers[&source.id].clone();
            async move {
                let page = provider.list_changes(&source.scope, None).await?;
                let me = if with_me {
                    Some(provider.whoami().await?.login)
                } else {
                    None
                };
                Ok::<_, rb_core::Error>((page.items, me))
            }
        },
    )));

    let mut changes = Vec::new();
    let mut me = HashMap::new();
    for (source, result) in sources.iter().zip(fetched) {
        let (items, login) = result?;
        changes.extend(items.into_iter().filter(|c| c.id.source_id == source.id));
        if let Some(login) = login {
            me.insert(source.id.clone(), login);
        }
    }
    Ok(Loaded {
        changes,
        sources,
        now: ctx.now(),
        triage_config: queue_settings(&ctx.config).triage,
        providers,
        me,
    })
}

impl Loaded {
    /// Your login on the source that owns `change`, when it was asked for.
    pub fn me(&self, change: &ChangeSummary) -> Option<&str> {
        self.me.get(&change.id.source_id).map(String::as_str)
    }

    fn source(&self, change: &ChangeSummary) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == change.id.source_id)
    }

    pub fn source_name(&self, change: &ChangeSummary) -> String {
        self.source(change)
            .map_or_else(|| change.id.source_id.to_string(), |s| s.id.to_string())
    }

    pub fn triage(&self, change: &ChangeSummary) -> TriageOutcome {
        triage(change, &self.triage_config, self.now)
    }

    /// The change as `--json` sees it, built from `rb-core` types and not a forge's own shape.
    pub fn json(&self, change: &ChangeSummary) -> Value {
        let outcome = self.triage(change);
        let url = self
            .providers
            .get(&change.id.source_id)
            .map(|p| p.web_url(&change.id).to_string());
        json!({
            "source": self.source_name(change),
            "forge": name_of(&change.id.kind),
            "host": self.source(change).map(|s| s.host.as_str()),
            "repo": change.id.repo,
            "number": change.id.number,
            "ref": change.id.short_ref(),
            "url": url,
            "title": change.title,
            "author": change.author,
            "state": name_of(&change.state),
            "isDraft": change.draft,
            "createdAt": iso(change.created_at),
            "updatedAt": iso(change.updated_at),
            "headRefName": change.branch,
            "baseRefName": change.base,
            "headSha": change.head_sha,
            "additions": change.adds,
            "deletions": change.dels,
            "changedFiles": change.files,
            "ci": name_of(&change.ci),
            "reviewers": change.reviewers,
            "myRole": name_of(&change.my_role),
            "myReview": name_of(&change.my_review),
            "bucket": name_of(&outcome.bucket),
            "bucketReason": outcome.reason.to_string(),
            "labels": change.labels,
        })
    }
}

fn name_of<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// The word a pipe sees instead of a CI glyph.
pub fn ci_word(ci: CiState) -> &'static str {
    match ci {
        CiState::Pass => "pass",
        CiState::Running => "running",
        CiState::Fail => "fail",
        CiState::None => "none",
        CiState::Neutral => "neutral",
        CiState::Skipped => "skipped",
        CiState::Cancelled => "cancelled",
    }
}

/// A glyph on a terminal and a word on a pipe.
pub fn ci_cell(ci: CiState) -> Cell {
    let (glyph, role) = ci_look(ci);
    Cell::styled(glyph, role).with_pipe(ci_word(ci))
}

/// `2026-10-05T10:00:00Z`.
pub fn iso(at: Timestamp) -> String {
    let days = at.0.div_euclid(86_400);
    let rem = at.0.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// The inverse of the demo clock's date arithmetic: days since 1970-01-01 to a calendar date.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_formats_known_instants() {
        assert_eq!(iso(Timestamp(0)), "1970-01-01T00:00:00Z");
        assert_eq!(iso(Timestamp(951_868_800)), "2000-03-01T00:00:00Z");
        assert_eq!(iso(Timestamp(1_791_194_400)), "2026-10-05T10:00:00Z");
        assert_eq!(iso(Timestamp(1_791_194_400 + 61)), "2026-10-05T10:01:01Z");
    }

    #[cfg(feature = "demo")]
    #[test]
    fn iso_round_trips_the_demo_clock() {
        for text in [
            "2024-02-29T23:59:59",
            "2026-10-05T10:00:00",
            "1999-12-31T00:00:00",
        ] {
            let at = crate::demo::parse_iso(text).unwrap();
            assert_eq!(iso(at), format!("{text}Z"));
        }
    }

    #[test]
    fn ci_words_are_stable() {
        assert_eq!(ci_word(CiState::Pass), "pass");
        assert_eq!(ci_word(CiState::None), "none");
    }
}
