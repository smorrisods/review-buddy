//! Built-in bucket triage (SPEC §5). Pure: the caller supplies `now`.
//!
//! The `[[triage.rule]]` engine will run *before* `triage` and report `TriageReason::Rule`; the
//! outcome type already carries a reason so `review-buddy triage explain` has one shape to print.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{ChangeSummary, MyReview, MyRole, Timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    /// Waiting on you. Config key `wait`.
    Wait,
    /// Worth a look. Config key `look`.
    Look,
    /// Can wait. Config key `later`.
    Later,
    /// Hidden by default. Config key `noise`.
    Noise,
}

impl Bucket {
    /// Queue order: Noise is last.
    pub const ORDER: [Bucket; 4] = [Bucket::Wait, Bucket::Look, Bucket::Later, Bucket::Noise];

    pub fn title(self) -> &'static str {
        match self {
            Self::Wait => "Waiting on you",
            Self::Look => "Worth a look",
            Self::Later => "Can wait",
            Self::Noise => "Noise",
        }
    }

    pub fn config_key(self) -> &'static str {
        match self {
            Self::Wait => "wait",
            Self::Look => "look",
            Self::Later => "later",
            Self::Noise => "noise",
        }
    }

    pub fn from_config_key(key: &str) -> Option<Self> {
        Self::ORDER.into_iter().find(|b| b.config_key() == key)
    }
}

/// The `[triage]` settings the built-in rules read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageConfig {
    /// Exact logins or `*` globs, matched case-insensitively.
    pub noise_authors: Vec<String>,
    /// Open changes untouched for longer than this drop to Can wait.
    pub stale_after: Duration,
}

impl Default for TriageConfig {
    fn default() -> Self {
        Self {
            noise_authors: ["renovate[bot]", "dependabot[bot]", "release-please[bot]"]
                .map(String::from)
                .to_vec(),
            stale_after: Duration::from_secs(14 * 86_400),
        }
    }
}

/// Why a change landed in its bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriageReason {
    NoiseAuthor {
        pattern: String,
    },
    BotAccount,
    ReviewRequested,
    AssignedNotReviewed,
    Mentioned,
    PreviouslyCommented,
    AuthoredWithActivity,
    /// Would have been `was`, but nothing has happened for longer than `stale_after`.
    Stale {
        was: Bucket,
    },
    Default,
    /// Set by the `[[triage.rule]]` engine: 1-based rule number and a description of its conditions.
    Rule {
        number: usize,
        summary: String,
    },
}

impl fmt::Display for TriageReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoiseAuthor { pattern } => write!(f, "author matches noise_authors `{pattern}`"),
            Self::BotAccount => f.write_str("author is a bot account"),
            Self::ReviewRequested => f.write_str("your review is requested"),
            Self::AssignedNotReviewed => {
                f.write_str("you are assigned and haven't reviewed since the last push")
            }
            Self::Mentioned => f.write_str("you are mentioned"),
            Self::PreviouslyCommented => f.write_str("you have commented before"),
            Self::AuthoredWithActivity => f.write_str("you authored it and it has new activity"),
            Self::Stale { was } => write!(
                f,
                "no activity for longer than stale_after (would have been {})",
                was.title()
            ),
            Self::Default => f.write_str("nothing needs you right now"),
            Self::Rule { number, summary } => write!(f, "bucketed by rule {number} · {summary}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageOutcome {
    pub bucket: Bucket,
    pub reason: TriageReason,
}

/// Bucket a change using the built-in rules; first match wins.
pub fn triage(change: &ChangeSummary, config: &TriageConfig, now: Timestamp) -> TriageOutcome {
    if let Some(reason) = noise_reason(change, config) {
        return TriageOutcome {
            bucket: Bucket::Noise,
            reason,
        };
    }
    triage_ignoring_noise(change, config, now)
}

/// The bucket a change would take if Noise didn't exist: where it sits when the `noise`
/// Show filter mixes bot updates into the normal buckets.
pub fn triage_ignoring_noise(
    change: &ChangeSummary,
    config: &TriageConfig,
    now: Timestamp,
) -> TriageOutcome {
    let (bucket, reason) = match attention(change) {
        Some(found) => found,
        None => (Bucket::Later, TriageReason::Default),
    };
    if matches!(bucket, Bucket::Wait | Bucket::Look) && is_stale(change, config, now) {
        return TriageOutcome {
            bucket: Bucket::Later,
            reason: TriageReason::Stale { was: bucket },
        };
    }
    TriageOutcome { bucket, reason }
}

/// Convenience for callers that only need the bucket.
pub fn bucket_for(change: &ChangeSummary, config: &TriageConfig, now: Timestamp) -> Bucket {
    triage(change, config, now).bucket
}

fn noise_reason(change: &ChangeSummary, config: &TriageConfig) -> Option<TriageReason> {
    if let Some(pattern) = config
        .noise_authors
        .iter()
        .find(|p| glob_match(p, &change.author))
    {
        return Some(TriageReason::NoiseAuthor {
            pattern: pattern.clone(),
        });
    }
    is_bot_login(&change.author)
        .then_some(TriageReason::BotAccount)
        .or_else(|| change.author_is_bot.then_some(TriageReason::BotAccount))
}

/// GitHub app accounts end in `[bot]`; GitLab project and group access tokens are `project_<id>_bot…`.
fn is_bot_login(login: &str) -> bool {
    let lower = login.to_lowercase();
    lower.ends_with("[bot]")
        || (lower.starts_with("project_") || lower.starts_with("group_")) && lower.contains("_bot")
}

fn attention(change: &ChangeSummary) -> Option<(Bucket, TriageReason)> {
    let needs_my_review = !reviewed_since_last_push(change);
    match change.my_role {
        MyRole::Reviewing if needs_my_review => {
            return Some((Bucket::Wait, TriageReason::ReviewRequested))
        }
        MyRole::Assigned if needs_my_review => {
            return Some((Bucket::Wait, TriageReason::AssignedNotReviewed))
        }
        _ => {}
    }
    if change.my_role == MyRole::Mentioned {
        return Some((Bucket::Look, TriageReason::Mentioned));
    }
    if change.i_commented || change.my_review == MyReview::Commented {
        return Some((Bucket::Look, TriageReason::PreviouslyCommented));
    }
    if change.my_role == MyRole::Authored && change.has_new_activity {
        return Some((Bucket::Look, TriageReason::AuthoredWithActivity));
    }
    None
}

/// A review counts only while it covers the current head. When the forge didn't tell us which
/// commit it covered, an existing review is assumed to be current.
fn reviewed_since_last_push(change: &ChangeSummary) -> bool {
    if change.my_review == MyReview::None {
        return false;
    }
    change
        .my_reviewed_sha
        .as_deref()
        .is_none_or(|sha| sha == change.head_sha)
}

fn is_stale(change: &ChangeSummary, config: &TriageConfig, now: Timestamp) -> bool {
    let age = now.secs_since(change.updated_at);
    age > 0 && (age as u64) > config.stale_after.as_secs()
}

/// Case-insensitive glob where `*` matches any run of characters, including none.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Parse `"14d"`, `"36h"`, `"90m"`, `"45s"` or `"2w"` (a single unit).
pub fn parse_age(text: &str) -> Option<Duration> {
    let text = text.trim();
    let unit = text.chars().last()?;
    let n: u64 = text[..text.len() - unit.len_utf8()].trim().parse().ok()?;
    let secs = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3_600,
        'd' => 86_400,
        'w' => 604_800,
        _ => return None,
    };
    n.checked_mul(secs).map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChangeId, ChangeState, CiState, ForgeKind, SourceId};

    const NOW: Timestamp = Timestamp(100 * 86_400);
    const HOUR: i64 = 3_600;
    const DAY: i64 = 86_400;

    fn change() -> ChangeSummary {
        ChangeSummary {
            id: ChangeId {
                source_id: SourceId::new("gh"),
                kind: ForgeKind::GitHub,
                repo: "liminal-hq/spindle".into(),
                number: 214,
            },
            title: "Add titleset menus".into(),
            author: "ada".into(),
            author_is_bot: false,
            state: ChangeState::Open,
            draft: false,
            created_at: Timestamp(NOW.0 - 2 * DAY),
            updated_at: Timestamp(NOW.0 - 2 * HOUR),
            branch: "feat/menus".into(),
            base: "main".into(),
            head_sha: "bbb".into(),
            base_sha: "aaa".into(),
            adds: 1,
            dels: 1,
            files: 1,
            ci: CiState::Pass,
            labels: vec![],
            reviewers: vec![],
            my_role: MyRole::Mentioned,
            my_review: MyReview::None,
            my_reviewed_sha: None,
            i_commented: false,
            has_new_activity: false,
            signals: Default::default(),
        }
    }

    fn run(c: &ChangeSummary) -> TriageOutcome {
        triage(c, &TriageConfig::default(), NOW)
    }

    #[test]
    fn default_noise_authors_go_to_noise() {
        for bot in ["renovate[bot]", "dependabot[bot]", "release-please[bot]"] {
            let mut c = change();
            c.author = bot.into();
            let out = run(&c);
            assert_eq!(out.bucket, Bucket::Noise, "{bot}");
            assert!(matches!(out.reason, TriageReason::NoiseAuthor { .. }));
        }
    }

    #[test]
    fn noise_authors_are_case_insensitive_globs() {
        let config = TriageConfig {
            noise_authors: vec!["Build-*".into(), "exact".into()],
            ..TriageConfig::default()
        };
        let mut c = change();
        c.author = "build-agent".into();
        assert_eq!(triage(&c, &config, NOW).bucket, Bucket::Noise);
        c.author = "EXACT".into();
        assert_eq!(triage(&c, &config, NOW).bucket, Bucket::Noise);
        c.author = "exactly".into();
        assert_ne!(triage(&c, &config, NOW).bucket, Bucket::Noise);
    }

    #[test]
    fn unknown_bot_patterns_are_noise_even_with_empty_noise_authors() {
        let config = TriageConfig {
            noise_authors: vec![],
            ..TriageConfig::default()
        };
        for login in ["some-app[bot]", "project_42_bot_abc", "group_7_bot1"] {
            let mut c = change();
            c.author = login.into();
            let out = triage(&c, &config, NOW);
            assert_eq!(out.bucket, Bucket::Noise, "{login}");
            assert_eq!(out.reason, TriageReason::BotAccount);
        }
        let mut c = change();
        c.author_is_bot = true;
        assert_eq!(triage(&c, &config, NOW).bucket, Bucket::Noise);
    }

    #[test]
    fn human_lookalikes_are_not_bots() {
        for login in ["robot", "bot", "project_manager", "abbott"] {
            let mut c = change();
            c.author = login.into();
            assert_ne!(run(&c).bucket, Bucket::Noise, "{login}");
        }
    }

    #[test]
    fn noise_beats_review_requested() {
        let mut c = change();
        c.author = "dependabot[bot]".into();
        c.my_role = MyRole::Reviewing;
        assert_eq!(run(&c).bucket, Bucket::Noise);
    }

    #[test]
    fn review_requested_waits_on_you() {
        let mut c = change();
        c.my_role = MyRole::Reviewing;
        let out = run(&c);
        assert_eq!(out.bucket, Bucket::Wait);
        assert_eq!(out.reason, TriageReason::ReviewRequested);
    }

    #[test]
    fn review_already_covering_head_does_not_wait() {
        let mut c = change();
        c.my_role = MyRole::Reviewing;
        c.my_review = MyReview::Approved;
        c.my_reviewed_sha = Some("bbb".into());
        assert_eq!(run(&c).bucket, Bucket::Later);
    }

    #[test]
    fn new_push_after_review_waits_again() {
        let mut c = change();
        c.my_role = MyRole::Reviewing;
        c.my_review = MyReview::Approved;
        c.my_reviewed_sha = Some("old".into());
        assert_eq!(run(&c).bucket, Bucket::Wait);
    }

    #[test]
    fn review_with_unknown_sha_counts_as_current() {
        let mut c = change();
        c.my_role = MyRole::Assigned;
        c.my_review = MyReview::ChangesRequested;
        c.my_reviewed_sha = None;
        assert_eq!(run(&c).bucket, Bucket::Later);
    }

    #[test]
    fn assigned_and_unreviewed_waits() {
        let mut c = change();
        c.my_role = MyRole::Assigned;
        let out = run(&c);
        assert_eq!(out.bucket, Bucket::Wait);
        assert_eq!(out.reason, TriageReason::AssignedNotReviewed);
    }

    #[test]
    fn assigned_and_reviewed_since_push_drops_out_of_wait() {
        let mut c = change();
        c.my_role = MyRole::Assigned;
        c.my_review = MyReview::Commented;
        c.my_reviewed_sha = Some("bbb".into());
        // Commenting earlier still makes it worth a look.
        assert_eq!(run(&c).bucket, Bucket::Look);
    }

    #[test]
    fn mentioned_is_worth_a_look() {
        let out = run(&change());
        assert_eq!(out.bucket, Bucket::Look);
        assert_eq!(out.reason, TriageReason::Mentioned);
    }

    #[test]
    fn having_commented_is_worth_a_look() {
        let mut c = change();
        c.my_role = MyRole::Reviewing;
        c.my_review = MyReview::Approved;
        c.my_reviewed_sha = Some("bbb".into());
        c.i_commented = true;
        let out = run(&c);
        assert_eq!(out.bucket, Bucket::Look);
        assert_eq!(out.reason, TriageReason::PreviouslyCommented);
    }

    #[test]
    fn authored_with_new_activity_is_worth_a_look() {
        let mut c = change();
        c.my_role = MyRole::Authored;
        c.has_new_activity = true;
        let out = run(&c);
        assert_eq!(out.bucket, Bucket::Look);
        assert_eq!(out.reason, TriageReason::AuthoredWithActivity);
    }

    #[test]
    fn authored_quiet_or_approved_can_wait() {
        let mut c = change();
        c.my_role = MyRole::Authored;
        assert_eq!(run(&c).bucket, Bucket::Later);
        c.my_review = MyReview::Approved;
        let out = run(&c);
        assert_eq!(out.bucket, Bucket::Later);
        assert_eq!(out.reason, TriageReason::Default);
    }

    #[test]
    fn new_activity_alone_does_not_matter_for_others_changes() {
        let mut c = change();
        c.my_role = MyRole::Authored;
        c.has_new_activity = false;
        assert_eq!(run(&c).bucket, Bucket::Later);
    }

    #[test]
    fn drafts_follow_the_same_rules() {
        let mut c = change();
        c.draft = true;
        c.my_role = MyRole::Authored;
        assert_eq!(run(&c).bucket, Bucket::Later);
        c.my_role = MyRole::Reviewing;
        assert_eq!(run(&c).bucket, Bucket::Wait);
    }

    #[test]
    fn stale_wait_and_look_drop_to_can_wait() {
        let mut c = change();
        c.updated_at = Timestamp(NOW.0 - 15 * DAY);
        c.my_role = MyRole::Reviewing;
        let out = run(&c);
        assert_eq!(out.bucket, Bucket::Later);
        assert_eq!(out.reason, TriageReason::Stale { was: Bucket::Wait });
        c.my_role = MyRole::Mentioned;
        assert_eq!(run(&c).reason, TriageReason::Stale { was: Bucket::Look });
    }

    #[test]
    fn stale_boundary_is_exclusive() {
        let mut c = change();
        c.my_role = MyRole::Reviewing;
        c.updated_at = Timestamp(NOW.0 - 14 * DAY);
        assert_eq!(run(&c).bucket, Bucket::Wait);
        c.updated_at = Timestamp(NOW.0 - 14 * DAY - 1);
        assert_eq!(run(&c).bucket, Bucket::Later);
    }

    #[test]
    fn stale_does_not_rescue_noise_and_future_timestamps_are_fresh() {
        let mut c = change();
        c.author = "renovate[bot]".into();
        c.updated_at = Timestamp(NOW.0 - 30 * DAY);
        assert_eq!(run(&c).bucket, Bucket::Noise);
        let mut c = change();
        c.my_role = MyRole::Reviewing;
        c.updated_at = Timestamp(NOW.0 + DAY);
        assert_eq!(run(&c).bucket, Bucket::Wait);
    }

    #[test]
    fn stale_after_is_configurable() {
        let config = TriageConfig {
            stale_after: Duration::from_secs(HOUR as u64),
            ..TriageConfig::default()
        };
        let mut c = change();
        c.my_role = MyRole::Reviewing;
        assert_eq!(triage(&c, &config, NOW).bucket, Bucket::Later);
    }

    #[test]
    fn reasons_read_kindly() {
        assert_eq!(
            TriageReason::ReviewRequested.to_string(),
            "your review is requested"
        );
        assert_eq!(
            TriageReason::Rule {
                number: 3,
                summary: "repo platform/infra, path **/*.tf".into()
            }
            .to_string(),
            "bucketed by rule 3 · repo platform/infra, path **/*.tf"
        );
    }

    #[test]
    fn bucket_keys_round_trip() {
        for b in Bucket::ORDER {
            assert_eq!(Bucket::from_config_key(b.config_key()), Some(b));
        }
        assert_eq!(Bucket::from_config_key("nope"), None);
        assert_eq!(Bucket::Wait.title(), "Waiting on you");
    }

    #[test]
    fn glob_cases() {
        assert!(glob_match("*[bot]", "renovate[bot]"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a*b*c", "axxbyy"));
        assert!(glob_match("**", "anything"));
        assert!(!glob_match("", "x"));
        assert!(glob_match("", ""));
    }

    #[test]
    fn age_parsing() {
        assert_eq!(parse_age("14d"), Some(Duration::from_secs(14 * 86_400)));
        assert_eq!(parse_age(" 2w "), Some(Duration::from_secs(1_209_600)));
        assert_eq!(parse_age("90m"), Some(Duration::from_secs(5_400)));
        assert_eq!(parse_age("14"), None);
        assert_eq!(parse_age("d"), None);
        assert_eq!(parse_age(""), None);
        assert_eq!(parse_age("-1d"), None);
        assert_eq!(parse_age("5y"), None);
    }
}
