//! The queue as data: which changes sit in which bucket, the rows the pane draws, and the
//! text of each row. Pure, so selection, scrolling and rendering all agree on one model.

use rb_core::{
    triage::{triage, triage_ignoring_noise, Bucket, TriageConfig},
    ChangeState, ChangeSummary, CiState, MyReview, MyRole, SourceId, Timestamp,
};

use super::AppState;
use crate::config::{Config, ShowFilter};

/// Everything the queue reads from `[triage]`, shared by the dashboard and the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSettings {
    pub triage: TriageConfig,
    pub show: Vec<ShowFilter>,
    /// Rows each bucket shows before `+N more`.
    pub bucket_limit: usize,
}

impl Default for QueueSettings {
    fn default() -> Self {
        queue_settings(&Config::default())
    }
}

/// Builds the queue settings from the loaded config.
pub fn queue_settings(config: &Config) -> QueueSettings {
    let t = &config.triage;
    QueueSettings {
        triage: TriageConfig {
            noise_authors: t.noise_authors.clone(),
            stale_after: t.stale_after,
        },
        show: t.show.clone(),
        bucket_limit: t.bucket_limit as usize,
    }
}

/// Whether the Show filters let a change into the queue. Noise is decided by bucket, later.
pub fn visible(change: &ChangeSummary, show: &[ShowFilter]) -> bool {
    if change.draft && !show.contains(&ShowFilter::Drafts) {
        return false;
    }
    match change.my_role {
        MyRole::Reviewing => show.contains(&ShowFilter::Reviewing),
        MyRole::Assigned => show.contains(&ShowFilter::Assigned),
        MyRole::Authored => show.contains(&ShowFilter::Authored),
        MyRole::Mentioned => true,
    }
}

/// One selectable stop in the queue, in reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// An index into `AppState::changes`.
    Change(usize),
    /// The `+N more` row that expands a bucket cut by `bucket_limit`, or collapses it again.
    More(Bucket),
    /// The collapsible Noise row.
    Noise,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub bucket: Bucket,
    pub changes: Vec<usize>,
    /// Changes left out by the bucket limit.
    pub more: usize,
    /// The bucket holds more than `bucket_limit`, so it gets a `+N more` row.
    pub cut: bool,
    pub expanded: bool,
}

impl Section {
    /// Every change the bucket holds, shown or not.
    pub fn total(&self) -> usize {
        self.changes.len() + self.more
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    /// Waiting on you, Worth a look and Can wait; empty buckets are left out.
    pub sections: Vec<Section>,
    pub noise: Vec<usize>,
    pub noise_open: bool,
}

impl Queue {
    /// Buckets the changes of one source (or of every `in_all` source when `source` is `None`).
    pub fn build(state: &AppState, source: Option<&SourceId>, noise_open: bool) -> Self {
        Self::build_with(state, source, noise_open, &[])
    }

    /// Like [`Queue::build`], with the buckets listed in `expanded` shown past `bucket_limit`.
    pub fn build_with(
        state: &AppState,
        source: Option<&SourceId>,
        noise_open: bool,
        expanded: &[Bucket],
    ) -> Self {
        let now = state.now.unwrap_or(Timestamp(0));
        let settings = &state.queue_settings;
        let mut buckets: [Vec<usize>; 4] = Default::default();
        for (index, change) in state.changes.iter().enumerate() {
            if change.state != ChangeState::Open
                || !in_scope(state, change, source)
                || !visible(change, &settings.show)
            {
                continue;
            }
            let mut bucket = triage(change, &settings.triage, now).bucket;
            if bucket == Bucket::Noise && settings.show.contains(&ShowFilter::Noise) {
                bucket = triage_ignoring_noise(change, &settings.triage, now).bucket;
            }
            let slot = Bucket::ORDER.iter().position(|b| *b == bucket).unwrap_or(2);
            buckets[slot].push(index);
        }
        for list in &mut buckets {
            list.sort_by(|a, b| {
                let (a, b) = (&state.changes[*a], &state.changes[*b]);
                own_failure(b)
                    .cmp(&own_failure(a))
                    .then(b.updated_at.cmp(&a.updated_at))
                    .then_with(|| {
                        source_order(state, &a.id.source_id)
                            .cmp(&source_order(state, &b.id.source_id))
                    })
                    .then_with(|| (&a.id.repo, a.id.number).cmp(&(&b.id.repo, b.id.number)))
            });
        }
        let [wait, look, later, noise] = buckets;
        let sections = [
            (Bucket::Wait, wait),
            (Bucket::Look, look),
            (Bucket::Later, later),
        ]
        .into_iter()
        .filter(|(_, changes)| !changes.is_empty())
        .map(|(bucket, mut changes)| {
            let cut = changes.len() > settings.bucket_limit;
            let expanded = cut && expanded.contains(&bucket);
            let more = if expanded {
                0
            } else {
                changes.len().saturating_sub(settings.bucket_limit)
            };
            changes.truncate(changes.len() - more);
            Section {
                bucket,
                changes,
                more,
                cut,
                expanded,
            }
        })
        .collect();
        Self {
            sections,
            noise,
            noise_open,
        }
    }

    /// Every selectable item in order. The Noise row exists whenever there is noise.
    pub fn items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = self
            .sections
            .iter()
            .flat_map(|s| {
                s.changes
                    .iter()
                    .map(|i| Item::Change(*i))
                    .chain(s.cut.then_some(Item::More(s.bucket)))
            })
            .collect();
        if !self.noise.is_empty() {
            items.push(Item::Noise);
            if self.noise_open {
                items.extend(self.noise.iter().map(|i| Item::Change(*i)));
            }
        }
        items
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty() && self.noise.is_empty()
    }

    /// The rows the pane draws, top to bottom.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for section in &self.sections {
            if !rows.is_empty() {
                rows.push(Row::Gap);
            }
            rows.push(Row::Heading(section.bucket, section.total()));
            rows.extend(section.changes.iter().map(|i| Row::Item(Item::Change(*i))));
            if section.cut {
                rows.push(Row::Item(Item::More(section.bucket)));
            }
        }
        if !self.noise.is_empty() {
            if !rows.is_empty() {
                rows.push(Row::Gap);
            }
            rows.push(Row::Item(Item::Noise));
            if self.noise_open {
                rows.extend(self.noise.iter().map(|i| Row::Item(Item::Change(*i))));
            }
        }
        if !rows.is_empty() {
            rows.push(Row::Gap);
            rows.push(Row::End);
        }
        rows
    }
}

/// One band of the queue pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Heading(Bucket, usize),
    Item(Item),
    Gap,
    /// `── That's everything.` and its note.
    End,
}

impl Row {
    pub fn height(self) -> u16 {
        match self {
            Row::Heading(..) | Row::Gap | Row::Item(Item::More(_) | Item::Noise) => 1,
            Row::Item(Item::Change(_)) | Row::End => 2,
        }
    }
}

pub fn total_height(rows: &[Row]) -> u16 {
    rows.iter().map(|r| r.height()).sum()
}

/// Top line and height of `item` within the pane's content.
pub fn item_span(rows: &[Row], item: Item) -> Option<(u16, u16)> {
    let mut top = 0;
    for row in rows {
        if *row == Row::Item(item) {
            return Some((top, row.height()));
        }
        top += row.height();
    }
    None
}

fn in_scope(state: &AppState, change: &ChangeSummary, source: Option<&SourceId>) -> bool {
    match source {
        Some(id) => &change.id.source_id == id,
        None => state
            .sources
            .iter()
            .find(|s| s.id == change.id.source_id)
            .is_none_or(|s| s.in_all),
    }
}

/// Where a source sits in config order; unknown sources go last.
fn source_order(state: &AppState, id: &SourceId) -> usize {
    state
        .sources
        .iter()
        .position(|s| &s.id == id)
        .unwrap_or(usize::MAX)
}

fn own_failure(change: &ChangeSummary) -> bool {
    change.my_role == MyRole::Authored && change.ci == CiState::Fail
}

/// How many open changes a source holds, whatever the Show filters say (`None` is All).
pub fn total_in(state: &AppState, source: Option<&SourceId>) -> usize {
    state
        .changes
        .iter()
        .filter(|c| c.state == ChangeState::Open && in_scope(state, c, source))
        .count()
}

/// How many open changes the Show filters let through (`None` is All). This is what the
/// bucket headings add up to, plus Noise.
pub fn count_in(state: &AppState, source: Option<&SourceId>) -> usize {
    state
        .changes
        .iter()
        .filter(|c| {
            c.state == ChangeState::Open
                && in_scope(state, c, source)
                && visible(c, &state.queue_settings.show)
        })
        .count()
}

/// How many open changes the Show filters hide (`None` is All).
pub fn hidden_in(state: &AppState, source: Option<&SourceId>) -> usize {
    total_in(state, source) - count_in(state, source)
}

/// The text of one two-line row, before styling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowParts {
    pub ci: CiState,
    pub title: String,
    pub age: String,
    pub forge: &'static str,
    pub reference: String,
    pub author: String,
    pub status: &'static str,
}

pub fn row_parts(change: &ChangeSummary, now: Timestamp) -> RowParts {
    let repo = change.id.repo.rsplit('/').next().unwrap_or(&change.id.repo);
    let sep = match change.id.kind {
        rb_core::ForgeKind::GitHub => '#',
        rb_core::ForgeKind::GitLab => '!',
    };
    RowParts {
        ci: change.ci,
        title: change.title.clone(),
        age: age(now, change.updated_at),
        forge: change.id.kind.tag(),
        reference: format!("{repo}{sep}{}", change.id.number),
        author: change.author.clone(),
        status: status_tag(change),
    }
}

pub fn status_tag(change: &ChangeSummary) -> &'static str {
    if change.author_is_bot {
        return "bot update";
    }
    if change.draft {
        return "draft";
    }
    match change.my_review {
        MyReview::Approved => return "approved by you",
        MyReview::ChangesRequested => return "you asked for changes",
        MyReview::Commented | MyReview::None => {}
    }
    match change.my_role {
        MyRole::Reviewing => "review requested",
        MyRole::Assigned => "assigned to you",
        MyRole::Mentioned => "you're mentioned",
        MyRole::Authored if change.ci == CiState::Fail => "yours · CI failing",
        MyRole::Authored => "yours",
    }
}

/// A short relative age: `now`, `5m`, `2h`, `3d`, `3w`, `4mo`, `2y`.
pub fn age(now: Timestamp, then: Timestamp) -> String {
    let secs = now.secs_since(then).max(0);
    let (minute, hour, day) = (60, 3_600, 86_400);
    match secs {
        s if s < minute => "now".to_string(),
        s if s < hour => format!("{}m", s / minute),
        s if s < day => format!("{}h", s / hour),
        s if s < 14 * day => format!("{}d", s / day),
        s if s < 60 * day => format!("{}w", s / (7 * day)),
        s if s < 365 * day => format!("{}mo", s / (30 * day)),
        s => format!("{}y", s / (365 * day)),
    }
}

/// `opened 2h ago`, or `opened just now`.
pub fn opened_phrase(now: Timestamp, then: Timestamp) -> String {
    match age(now, then).as_str() {
        "now" => "opened just now".to_string(),
        a => format!("opened {a} ago"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rb_core::{
        AuthMode, ChangeId, ForgeKind, Reviewer, ReviewerState, Scope, Source, SourceId,
    };

    pub fn change(n: u64, role: MyRole, updated: i64) -> ChangeSummary {
        ChangeSummary {
            id: ChangeId {
                source_id: SourceId::new("s1"),
                kind: ForgeKind::GitHub,
                repo: "acme/widgets".into(),
                number: n,
            },
            title: format!("Change {n}"),
            author: "ada".into(),
            author_is_bot: false,
            state: ChangeState::Open,
            draft: false,
            created_at: Timestamp(0),
            updated_at: Timestamp(updated),
            branch: "feat".into(),
            base: "main".into(),
            head_sha: "a".into(),
            base_sha: "b".into(),
            adds: 1,
            dels: 1,
            files: 1,
            ci: CiState::Pass,
            labels: vec![],
            reviewers: vec![Reviewer {
                login: "x".into(),
                state: ReviewerState::Requested,
            }],
            my_role: role,
            my_review: MyReview::None,
            my_reviewed_sha: None,
            i_commented: false,
            has_new_activity: false,
        }
    }

    pub fn source(id: &str, in_all: bool) -> Source {
        Source {
            id: SourceId::new(id),
            kind: ForgeKind::GitHub,
            host: "github.com".into(),
            label: id.into(),
            scope: Scope::everything(),
            auth: AuthMode::Cli,
            in_all,
            include_drafts: true,
            tag_colour: None,
        }
    }

    fn state(changes: Vec<ChangeSummary>) -> AppState {
        AppState {
            loaded: true,
            sources: vec![source("s1", true), source("s2", false)],
            changes,
            now: Some(Timestamp(1_000_000)),
            ..AppState::default()
        }
    }

    #[test]
    fn ages_are_compact_and_never_negative() {
        let now = Timestamp(10_000_000);
        let at = |secs: i64| age(now, Timestamp(10_000_000 - secs));
        assert_eq!(at(0), "now");
        assert_eq!(at(59), "now");
        assert_eq!(at(60), "1m");
        assert_eq!(at(3_599), "59m");
        assert_eq!(at(3_600), "1h");
        assert_eq!(at(2 * 3_600 + 5), "2h");
        assert_eq!(at(86_399), "23h");
        assert_eq!(at(3 * 86_400), "3d");
        assert_eq!(at(13 * 86_400), "13d");
        assert_eq!(at(14 * 86_400), "2w");
        assert_eq!(at(59 * 86_400), "8w");
        assert_eq!(at(90 * 86_400), "3mo");
        assert_eq!(at(800 * 86_400), "2y");
        assert_eq!(age(now, Timestamp(10_000_100)), "now");
    }

    #[test]
    fn opened_phrase_reads_naturally() {
        assert_eq!(
            opened_phrase(Timestamp(7_300), Timestamp(0)),
            "opened 2h ago"
        );
        assert_eq!(opened_phrase(Timestamp(5), Timestamp(0)), "opened just now");
    }

    #[test]
    fn row_parts_use_native_reference_and_status() {
        let mut c = change(214, MyRole::Reviewing, 1_000_000 - 7_200);
        let parts = row_parts(&c, Timestamp(1_000_000));
        assert_eq!(parts.reference, "widgets#214");
        assert_eq!(parts.forge, "GH");
        assert_eq!(parts.age, "2h");
        assert_eq!(parts.status, "review requested");
        c.id.kind = ForgeKind::GitLab;
        assert_eq!(row_parts(&c, Timestamp(1_000_000)).reference, "widgets!214");
        assert_eq!(row_parts(&c, Timestamp(1_000_000)).forge, "GL");
    }

    #[test]
    fn neutral_skipped_and_cancelled_ci_are_not_failures() {
        let mut c = change(1, MyRole::Authored, 0);
        for ci in [CiState::Neutral, CiState::Skipped, CiState::Cancelled] {
            c.ci = ci;
            assert_eq!(status_tag(&c), "yours");
            assert!(!own_failure(&c));
        }
    }

    #[test]
    fn status_tags_cover_the_cases() {
        let mut c = change(1, MyRole::Authored, 0);
        assert_eq!(status_tag(&c), "yours");
        c.ci = CiState::Fail;
        assert_eq!(status_tag(&c), "yours · CI failing");
        c.draft = true;
        assert_eq!(status_tag(&c), "draft");
        c.draft = false;
        c.my_review = MyReview::Approved;
        assert_eq!(status_tag(&c), "approved by you");
        c.author_is_bot = true;
        assert_eq!(status_tag(&c), "bot update");
    }

    #[test]
    fn buckets_split_and_noise_collapses() {
        let mut bot = change(3, MyRole::Reviewing, 5);
        bot.author = "renovate[bot]".into();
        bot.author_is_bot = true;
        let s = state(vec![
            change(1, MyRole::Reviewing, 10),
            change(2, MyRole::Reviewing, 20),
            bot,
        ]);
        let q = Queue::build(&s, None, false);
        assert_eq!(q.sections.len(), 1);
        assert_eq!(q.sections[0].bucket, Bucket::Wait);
        assert_eq!(q.sections[0].changes, vec![1, 0], "newest first");
        assert_eq!(q.noise, vec![2]);
        assert_eq!(
            q.items(),
            vec![Item::Change(1), Item::Change(0), Item::Noise]
        );
        let open = Queue::build(&s, None, true);
        assert_eq!(open.items().len(), 4);
        assert_eq!(open.items()[3], Item::Change(2));
    }

    #[test]
    fn own_failing_ci_sorts_first_in_its_bucket() {
        let mut failing = change(1, MyRole::Authored, 1);
        failing.ci = CiState::Fail;
        failing.has_new_activity = true;
        let mut fresh = change(2, MyRole::Authored, 100);
        fresh.has_new_activity = true;
        let s = state(vec![fresh, failing]);
        let q = Queue::build(&s, None, false);
        assert_eq!(q.sections[0].changes, vec![1, 0]);
    }

    #[test]
    fn all_respects_in_all_but_a_source_view_does_not() {
        let mut other = change(2, MyRole::Reviewing, 1);
        other.id.source_id = SourceId::new("s2");
        let s = state(vec![change(1, MyRole::Reviewing, 1), other]);
        assert_eq!(Queue::build(&s, None, false).items().len(), 1);
        let only = Queue::build(&s, Some(&SourceId::new("s2")), false);
        assert_eq!(only.items(), vec![Item::Change(1)]);
        assert_eq!(count_in(&s, None), 1);
        assert_eq!(count_in(&s, Some(&SourceId::new("s2"))), 1);
    }

    fn on(source: &str, repo: &str, n: u64, updated: i64) -> ChangeSummary {
        let mut c = change(n, MyRole::Reviewing, updated);
        c.id.source_id = SourceId::new(source);
        c.id.repo = repo.into();
        c
    }

    fn order(s: &AppState, source: Option<&str>) -> Vec<(String, u64)> {
        let id = source.map(SourceId::new);
        let q = Queue::build(s, id.as_ref(), false);
        q.items()
            .into_iter()
            .filter_map(|i| match i {
                Item::Change(n) => Some((
                    s.changes[n].id.source_id.as_str().to_string(),
                    s.changes[n].id.number,
                )),
                Item::Noise | Item::More(_) => None,
            })
            .collect()
    }

    #[test]
    fn mixed_queue_orders_by_bucket_then_recency_then_source_then_id() {
        let mut gl = on("s2", "g/p", 7, 500);
        gl.id.kind = rb_core::ForgeKind::GitLab;
        let mut later = on("s1", "a/b", 99, 900);
        later.my_role = MyRole::Authored;
        let mut s = state(vec![
            on("s2", "z/z", 3, 100),
            on("s1", "b/b", 2, 100),
            gl,
            on("s1", "a/b", 5, 100),
            on("s1", "a/b", 1, 100),
            later,
        ]);
        s.sources = vec![source("s1", true), source("s2", true)];
        let want = |v: &[(&str, u64)]| -> Vec<(String, u64)> {
            v.iter().map(|(a, b)| (a.to_string(), *b)).collect()
        };
        assert_eq!(
            order(&s, None),
            want(&[
                ("s2", 7),
                ("s1", 1),
                ("s1", 5),
                ("s1", 2),
                ("s2", 3),
                ("s1", 99)
            ])
        );
        // Config order decides ties between sources, not the order changes arrived in.
        s.sources.reverse();
        assert_eq!(
            order(&s, None),
            want(&[
                ("s2", 7),
                ("s2", 3),
                ("s1", 1),
                ("s1", 5),
                ("s1", 2),
                ("s1", 99)
            ])
        );
    }

    #[test]
    fn ordering_does_not_depend_on_arrival_order() {
        let mut s = state(vec![]);
        s.sources = vec![source("s1", true), source("s2", true)];
        let items = vec![
            on("s2", "x/y", 4, 50),
            on("s1", "x/y", 4, 50),
            on("s1", "x/y", 2, 50),
            on("s2", "x/y", 1, 80),
        ];
        s.changes = items.clone();
        let forward = order(&s, None);
        s.changes = items.into_iter().rev().collect();
        assert_eq!(order(&s, None), forward);
    }

    #[test]
    fn a_source_with_in_all_false_is_selectable_but_not_in_all() {
        let mut s = state(vec![on("s1", "a/b", 1, 1), on("s2", "a/b", 2, 1)]);
        s.sources = vec![source("s1", true), source("s2", false)];
        assert_eq!(order(&s, None), [("s1".to_string(), 1)]);
        assert_eq!(order(&s, Some("s2")), [("s2".to_string(), 2)]);
        assert_eq!(count_in(&s, None), 1);
        assert_eq!(count_in(&s, Some(&SourceId::new("s2"))), 1);
    }

    #[test]
    fn config_reaches_the_queue_settings() {
        let config: Config = toml::from_str(
            "[triage]\nnoise_authors = [\"ada\"]\nstale_after = \"2d\"\nbucket_limit = 3\nshow = [\"drafts\"]\n",
        )
        .unwrap();
        let settings = queue_settings(&config);
        assert_eq!(settings.triage.noise_authors, vec!["ada".to_string()]);
        assert_eq!(
            settings.triage.stale_after,
            std::time::Duration::from_secs(2 * 86_400)
        );
        assert_eq!(settings.bucket_limit, 3);
        assert_eq!(settings.show, vec![ShowFilter::Drafts]);
    }

    #[test]
    fn noise_authors_from_config_move_changes_into_noise() {
        let mut s = state(vec![change(1, MyRole::Reviewing, 10)]);
        s.queue_settings.triage.noise_authors = vec!["ada".into()];
        let q = Queue::build(&s, None, false);
        assert!(q.sections.is_empty());
        assert_eq!(q.noise, vec![0]);
    }

    #[test]
    fn show_filters_hide_roles_and_drafts() {
        let mut draft = change(2, MyRole::Reviewing, 10);
        draft.draft = true;
        let s = state(vec![
            change(1, MyRole::Authored, 10),
            draft,
            change(3, MyRole::Mentioned, 10),
        ]);
        let items = |show: Vec<ShowFilter>| {
            let mut s = s.clone();
            s.queue_settings.show = show;
            Queue::build(&s, None, false).items().len()
        };
        assert_eq!(
            items(vec![ShowFilter::Reviewing]),
            1,
            "mentions always show"
        );
        assert_eq!(items(vec![ShowFilter::Reviewing, ShowFilter::Authored]), 2);
        assert_eq!(
            items(vec![
                ShowFilter::Reviewing,
                ShowFilter::Authored,
                ShowFilter::Drafts
            ]),
            3
        );
    }

    #[test]
    fn closed_changes_are_left_out() {
        let mut merged = change(1, MyRole::Reviewing, 1);
        merged.state = ChangeState::Merged;
        let s = state(vec![merged]);
        assert!(Queue::build(&s, None, false).is_empty());
    }

    #[test]
    fn rows_have_headings_gaps_and_the_end_note() {
        let s = state(vec![
            change(1, MyRole::Reviewing, 10),
            change(2, MyRole::Mentioned, 20),
        ]);
        let rows = Queue::build(&s, None, false).rows();
        assert_eq!(rows[0], Row::Heading(Bucket::Wait, 1));
        assert_eq!(rows[2], Row::Gap);
        assert!(matches!(rows[3], Row::Heading(Bucket::Look, 1)));
        assert_eq!(*rows.last().unwrap(), Row::End);
        assert_eq!(total_height(&rows), 1 + 2 + 1 + 1 + 2 + 1 + 2);
        assert_eq!(item_span(&rows, Item::Change(1)), Some((5, 2)));
        assert_eq!(item_span(&rows, Item::Change(0)), Some((1, 2)));
        assert_eq!(item_span(&rows, Item::Noise), None);
        assert!(Queue::build(&state(vec![]), None, false).rows().is_empty());
    }

    const DAY: i64 = 86_400;

    fn bot(n: u64, updated: i64) -> ChangeSummary {
        let mut c = change(n, MyRole::Reviewing, updated);
        c.author = "renovate[bot]".into();
        c.author_is_bot = true;
        c
    }

    fn at(now: i64, mut s: AppState) -> AppState {
        s.now = Some(Timestamp(now));
        s
    }

    #[test]
    fn stale_after_drops_old_items_to_can_wait_by_age_from_now() {
        let now = 100 * DAY;
        let s = at(
            now,
            state(vec![
                change(1, MyRole::Reviewing, now - 13 * DAY),
                change(2, MyRole::Reviewing, now - 15 * DAY),
            ]),
        );
        let q = Queue::build(&s, None, false);
        let buckets: Vec<_> = q
            .sections
            .iter()
            .map(|x| (x.bucket, x.changes.clone()))
            .collect();
        assert_eq!(buckets, [(Bucket::Wait, vec![0]), (Bucket::Later, vec![1])]);

        let mut tight = s.clone();
        tight.queue_settings.triage.stale_after = std::time::Duration::from_secs(5 * DAY as u64);
        let q = Queue::build(&tight, None, false);
        assert_eq!(q.sections.len(), 1);
        assert_eq!(q.sections[0].bucket, Bucket::Later);
    }

    #[test]
    fn the_noise_filter_mixes_bot_updates_into_their_natural_bucket() {
        let mut s = at(
            10 * DAY,
            state(vec![change(1, MyRole::Reviewing, 9 * DAY), bot(2, 9 * DAY)]),
        );
        let q = Queue::build(&s, None, false);
        assert_eq!(q.noise, vec![1]);
        assert_eq!(q.sections[0].changes, vec![0]);

        s.queue_settings.show.push(ShowFilter::Noise);
        let q = Queue::build(&s, None, false);
        assert!(q.noise.is_empty());
        assert_eq!(q.sections.len(), 1);
        assert_eq!(q.sections[0].bucket, Bucket::Wait);
        assert_eq!(q.sections[0].total(), 2);
        assert!(!q.items().contains(&Item::Noise));
    }

    #[test]
    fn hidden_counts_follow_the_filters_and_the_source() {
        let mut draft = change(2, MyRole::Reviewing, 10);
        draft.draft = true;
        let mut other = change(3, MyRole::Authored, 10);
        other.id.source_id = SourceId::new("s2");
        let mut s = state(vec![change(1, MyRole::Reviewing, 10), draft, other]);
        s.sources = vec![source("s1", true), source("s2", true)];
        s.queue_settings.show = vec![ShowFilter::Reviewing];
        assert_eq!(total_in(&s, None), 3);
        assert_eq!(count_in(&s, None), 1);
        assert_eq!(hidden_in(&s, None), 2);
        assert_eq!(hidden_in(&s, Some(&SourceId::new("s1"))), 1);
        assert_eq!(count_in(&s, Some(&SourceId::new("s2"))), 0);
        s.queue_settings.show = vec![
            ShowFilter::Reviewing,
            ShowFilter::Authored,
            ShowFilter::Drafts,
        ];
        assert_eq!(hidden_in(&s, None), 0);
    }

    #[test]
    fn expanding_a_bucket_lists_everything_and_keeps_a_collapse_row() {
        let mut s = state(
            (1..=5)
                .map(|n| change(n, MyRole::Reviewing, n as i64))
                .collect(),
        );
        s.queue_settings.bucket_limit = 2;
        let q = Queue::build_with(&s, None, false, &[]);
        assert_eq!(q.sections[0].more, 3);
        assert_eq!(q.items().last(), Some(&Item::More(Bucket::Wait)));
        let q = Queue::build_with(&s, None, false, &[Bucket::Wait]);
        assert_eq!(q.sections[0].changes.len(), 5);
        assert!(q.sections[0].expanded && q.sections[0].more == 0);
        assert_eq!(q.items().len(), 6);
        assert!(matches!(q.rows()[0], Row::Heading(Bucket::Wait, 5)));
        s.queue_settings.bucket_limit = 5;
        let q = Queue::build_with(&s, None, false, &[Bucket::Wait]);
        assert!(!q.sections[0].cut, "nothing to collapse at the limit");
    }
}
