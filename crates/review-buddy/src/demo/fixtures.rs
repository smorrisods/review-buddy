//! Bundled TOML and patch fixtures, turned into domain types relative to a frozen `now`.

use anyhow::{anyhow, bail, Context, Result};
use rb_core::{
    triage::parse_age, AuthMode, ChangeDetail, ChangeId, ChangeState, ChangeSummary, Check,
    CiState, Comment, CommentId, DraftComment, FilePatch, FileStatus, ForgeKind, Mergeability,
    MyReview, MyRole, OpenThreads, ReviewDraft, Reviewer, Scope, Side, Signals, Source, SourceId,
    Thread, ThreadId, Timestamp, User,
};
use serde::Deserialize;
use url::Url;

/// The demo account, whose own reviews the queue reports apart from everyone else's.
const DEMO_USER: &str = "smorris";

const DEMO_TOML: &str = include_str!("../../fixtures/demo/demo.toml");

const PATCHES: &[(&str, &str)] = &[
    (
        "menus.rs.patch",
        include_str!("../../fixtures/demo/menus.rs.patch"),
    ),
    (
        "menubar.rs.patch",
        include_str!("../../fixtures/demo/menubar.rs.patch"),
    ),
    (
        "ui-mod.rs.patch",
        include_str!("../../fixtures/demo/ui-mod.rs.patch"),
    ),
    (
        "palette.rs.patch",
        include_str!("../../fixtures/demo/palette.rs.patch"),
    ),
    (
        "cache.rs.patch",
        include_str!("../../fixtures/demo/cache.rs.patch"),
    ),
    (
        "zshrc.patch",
        include_str!("../../fixtures/demo/zshrc.patch"),
    ),
    (
        "setup.md.patch",
        include_str!("../../fixtures/demo/setup.md.patch"),
    ),
    (
        "cargo-ratatui.patch",
        include_str!("../../fixtures/demo/cargo-ratatui.patch"),
    ),
    (
        "cargo-tokio.patch",
        include_str!("../../fixtures/demo/cargo-tokio.patch"),
    ),
];

/// One change with everything the provider serves for it.
#[derive(Debug, Clone)]
pub struct DemoChange {
    pub detail: ChangeDetail,
    pub files: Vec<FilePatch>,
    pub threads: Vec<Thread>,
    pub checks: Vec<Check>,
    pub draft: ReviewDraft,
}

impl DemoChange {
    pub fn id(&self) -> &ChangeId {
        &self.detail.summary.id
    }
}

#[derive(Debug, Clone)]
pub struct Fixtures {
    pub user: User,
    pub sources: Vec<Source>,
    pub changes: Vec<DemoChange>,
}

#[derive(Deserialize)]
struct Raw {
    user: User,
    #[serde(default)]
    source: Vec<RawSource>,
    #[serde(default)]
    change: Vec<RawChange>,
}

#[derive(Deserialize)]
struct RawSource {
    id: String,
    kind: ForgeKind,
    host: String,
    label: String,
    #[serde(default)]
    owners: Vec<String>,
    #[serde(default)]
    repos: Vec<String>,
    #[serde(default)]
    user: bool,
    auth: Option<AuthMode>,
    #[serde(default = "yes")]
    in_all: bool,
    tag_colour: Option<String>,
}

#[derive(Deserialize)]
struct RawChange {
    source: String,
    repo: String,
    number: u64,
    title: String,
    author: String,
    #[serde(default)]
    author_is_bot: bool,
    #[serde(default = "open")]
    state: ChangeState,
    #[serde(default)]
    draft: bool,
    created_ago: String,
    updated_ago: String,
    branch: String,
    base: String,
    head_sha: String,
    base_sha: String,
    ci: CiState,
    #[serde(default)]
    labels: Vec<String>,
    my_role: MyRole,
    my_review: MyReview,
    my_reviewed_sha: Option<String>,
    #[serde(default)]
    i_commented: bool,
    #[serde(default)]
    has_new_activity: bool,
    mergeable: Option<bool>,
    #[serde(default)]
    mergeability: Mergeability,
    #[serde(default)]
    commit_count: u32,
    /// Overrides the comment total, which otherwise counts the comments on `thread`.
    comments: Option<u32>,
    /// Overrides the open-thread count, which otherwise counts unresolved `thread`s.
    open_threads: Option<OpenThreads>,
    #[serde(default)]
    review_required: bool,
    body: String,
    #[serde(default)]
    reviewer: Vec<Reviewer>,
    #[serde(default)]
    file: Vec<RawFile>,
    #[serde(default)]
    check: Vec<RawCheck>,
    #[serde(default)]
    thread: Vec<RawThread>,
    #[serde(default)]
    draft_comment: Vec<DraftComment>,
}

#[derive(Deserialize)]
struct RawCheck {
    name: String,
    state: CiState,
    started_ago: Option<String>,
    duration_secs: Option<i64>,
    required: Option<bool>,
}

#[derive(Deserialize)]
struct RawFile {
    path: String,
    old_path: Option<String>,
    status: FileStatus,
    patch: String,
}

#[derive(Deserialize)]
struct RawThread {
    id: String,
    path: Option<String>,
    start_line: Option<u32>,
    line: Option<u32>,
    #[serde(default = "new_side")]
    side: Side,
    #[serde(default)]
    pending: bool,
    #[serde(default)]
    resolved: bool,
    #[serde(default)]
    outdated: bool,
    comment: Vec<RawComment>,
}

#[derive(Deserialize)]
struct RawComment {
    id: String,
    author: String,
    body: String,
    created_ago: String,
}

fn yes() -> bool {
    true
}

fn open() -> ChangeState {
    ChangeState::Open
}

fn new_side() -> Side {
    Side::New
}

/// Loads the bundled fixtures. Ages resolve against `now`.
pub fn load(now: Timestamp) -> Result<Fixtures> {
    let raw: Raw = toml::from_str(DEMO_TOML).context("demo fixtures don't parse")?;
    let sources = raw
        .source
        .iter()
        .map(|s| Source {
            id: SourceId::new(&s.id),
            kind: s.kind,
            host: s.host.clone(),
            label: s.label.clone(),
            scope: Scope {
                owners: s.owners.clone(),
                repos: s.repos.clone(),
                user: s.user,
            },
            auth: s.auth.unwrap_or(AuthMode::Cli),
            in_all: s.in_all,
            include_drafts: true,
            tag_colour: s.tag_colour.clone(),
        })
        .collect::<Vec<_>>();
    let changes = raw
        .change
        .into_iter()
        .map(|c| change(c, &sources, now))
        .collect::<Result<Vec<_>>>()?;
    Ok(Fixtures {
        user: raw.user,
        sources,
        changes,
    })
}

fn change(raw: RawChange, sources: &[Source], now: Timestamp) -> Result<DemoChange> {
    let source = sources
        .iter()
        .find(|s| s.id.as_str() == raw.source)
        .ok_or_else(|| anyhow!("unknown source `{}`", raw.source))?;
    let id = ChangeId {
        source_id: source.id.clone(),
        kind: source.kind,
        repo: raw.repo,
        number: raw.number,
    };
    let files = raw
        .file
        .into_iter()
        .map(|f| file(f, &id))
        .collect::<Result<Vec<_>>>()?;
    let threads = raw
        .thread
        .into_iter()
        .map(|t| thread(t, now))
        .collect::<Result<Vec<_>>>()?;
    let signals = Signals {
        comments: raw.comments.unwrap_or_else(|| {
            let total: usize = threads.iter().map(|t| t.comments.len()).sum();
            u32::try_from(total).unwrap_or(u32::MAX)
        }),
        open_threads: raw.open_threads.unwrap_or_else(|| {
            let open = threads.iter().filter(|t| !t.resolved).count();
            OpenThreads::Count(u32::try_from(open).unwrap_or(u32::MAX))
        }),
        ..Signals::from_reviewers(&raw.reviewer, DEMO_USER, raw.review_required)
    };
    let summary = ChangeSummary {
        adds: files.iter().map(|f| f.adds).sum(),
        dels: files.iter().map(|f| f.dels).sum(),
        files: u32::try_from(files.len()).unwrap_or(u32::MAX),
        title: raw.title,
        author: raw.author,
        author_is_bot: raw.author_is_bot,
        state: raw.state,
        draft: raw.draft,
        created_at: ago(now, &raw.created_ago)?,
        updated_at: ago(now, &raw.updated_ago)?,
        branch: raw.branch,
        base: raw.base,
        head_sha: raw.head_sha,
        base_sha: raw.base_sha,
        ci: raw.ci,
        labels: raw.labels,
        reviewers: raw.reviewer,
        my_role: raw.my_role,
        my_review: raw.my_review,
        my_reviewed_sha: raw.my_reviewed_sha,
        i_commented: raw.i_commented,
        has_new_activity: raw.has_new_activity,
        signals,
        id: id.clone(),
    };
    Ok(DemoChange {
        detail: ChangeDetail {
            summary,
            body: raw.body.trim_end().to_string(),
            web_url: change_url(&source.host, &id)?,
            mergeable: raw.mergeable,
            mergeability: raw.mergeability,
            commit_count: raw.commit_count,
        },
        files,
        threads,
        checks: raw
            .check
            .into_iter()
            .map(|c| check(c, now))
            .collect::<Result<Vec<_>>>()?,
        draft: ReviewDraft {
            body: String::new(),
            comments: raw.draft_comment,
        },
    })
}

fn file(raw: RawFile, id: &ChangeId) -> Result<FilePatch> {
    let patch = PATCHES
        .iter()
        .find(|(name, _)| *name == raw.patch)
        .map(|(_, body)| *body)
        .ok_or_else(|| anyhow!("{id}: no bundled patch `{}`", raw.patch))?;
    let (adds, dels) = count_changes(patch);
    Ok(FilePatch {
        path: raw.path,
        old_path: raw.old_path,
        status: raw.status,
        adds,
        dels,
        patch: Some(patch.to_string()),
    })
}

fn check(raw: RawCheck, now: Timestamp) -> Result<Check> {
    let started_at = raw.started_ago.map(|a| ago(now, &a)).transpose()?;
    let completed_at = started_at
        .zip(raw.duration_secs)
        .map(|(s, d)| Timestamp(s.0 + d));
    Ok(Check {
        name: raw.name,
        state: raw.state,
        url: None,
        started_at,
        completed_at,
        required: raw.required,
    })
}

fn thread(raw: RawThread, now: Timestamp) -> Result<Thread> {
    let comments = raw
        .comment
        .into_iter()
        .map(|c| {
            Ok(Comment {
                id: CommentId::new(c.id),
                author: c.author,
                body: c.body.trim_end().to_string(),
                created_at: ago(now, &c.created_ago)?,
                pending: raw.pending,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Thread {
        id: ThreadId::new(raw.id),
        path: raw.path,
        line: raw.line,
        side: raw.side,
        start_line: raw.start_line,
        start_side: raw.start_line.map(|_| raw.side),
        pending: raw.pending,
        resolved: raw.resolved,
        outdated: raw.outdated,
        comments,
    })
}

/// Added and removed line counts of a unified diff body.
pub fn count_changes(patch: &str) -> (u32, u32) {
    let mut adds = 0;
    let mut dels = 0;
    for line in patch.lines() {
        match line.as_bytes().first() {
            Some(b'+') => adds += 1,
            Some(b'-') => dels += 1,
            _ => {}
        }
    }
    (adds, dels)
}

fn ago(now: Timestamp, text: &str) -> Result<Timestamp> {
    let age = parse_age(text).ok_or_else(|| anyhow!("bad age `{text}`"))?;
    let secs = i64::try_from(age.as_secs()).map_err(|_| anyhow!("age `{text}` is too large"))?;
    Ok(Timestamp(now.0 - secs))
}

/// The browser URL a forge would use for this change.
pub fn change_url(host: &str, id: &ChangeId) -> Result<Url> {
    let path = match id.kind {
        ForgeKind::GitHub => format!("pull/{}", id.number),
        ForgeKind::GitLab => format!("-/merge_requests/{}", id.number),
    };
    let text = format!("https://{host}/{}/{path}", id.repo);
    match Url::parse(&text) {
        Ok(url) => Ok(url),
        Err(err) => bail!("bad URL `{text}`: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rb_core::{triage::triage, Bucket, TriageConfig};

    use super::*;
    use crate::demo::time::{parse_iso, DEFAULT_FROZEN};

    fn now() -> Timestamp {
        parse_iso(DEFAULT_FROZEN).unwrap()
    }

    fn fixtures() -> Fixtures {
        load(now()).unwrap()
    }

    fn bucket_of(change: &DemoChange) -> Bucket {
        triage(&change.detail.summary, &TriageConfig::default(), now()).bucket
    }

    #[test]
    fn seven_changes_across_both_forges_and_four_sources() {
        let f = fixtures();
        assert_eq!(f.changes.len(), 7);
        assert_eq!(f.sources.len(), 4);
        for kind in [ForgeKind::GitHub, ForgeKind::GitLab] {
            assert!(f.changes.iter().any(|c| c.id().kind == kind));
            assert!(f.sources.iter().any(|s| s.kind == kind));
        }
        assert!(f
            .sources
            .iter()
            .any(|s| s.host != "gitlab.com" && s.host != "github.com"));
    }

    #[test]
    fn every_triage_bucket_is_populated() {
        let f = fixtures();
        for bucket in Bucket::ORDER {
            assert!(
                f.changes.iter().any(|c| bucket_of(c) == bucket),
                "nothing in {bucket:?}"
            );
        }
    }

    #[test]
    fn exactly_two_bots_and_both_are_noise() {
        let f = fixtures();
        let bots: Vec<_> = f
            .changes
            .iter()
            .filter(|c| c.detail.summary.author_is_bot)
            .collect();
        assert_eq!(bots.len(), 2);
        assert!(bots.iter().all(|c| bucket_of(c) == Bucket::Noise));
        let noise = f
            .changes
            .iter()
            .filter(|c| bucket_of(c) == Bucket::Noise)
            .count();
        assert_eq!(noise, 2);
    }

    #[test]
    fn ids_are_unique_and_sources_exist() {
        let f = fixtures();
        let ids: HashSet<_> = f.changes.iter().map(|c| c.id().clone()).collect();
        assert_eq!(ids.len(), f.changes.len());
        let source_ids: HashSet<_> = f.sources.iter().map(|s| s.id.clone()).collect();
        assert_eq!(source_ids.len(), f.sources.len());
        assert!(f
            .changes
            .iter()
            .all(|c| source_ids.contains(&c.id().source_id)));
        let threads: Vec<_> = f.changes.iter().flat_map(|c| &c.threads).collect();
        let thread_ids: HashSet<_> = threads.iter().map(|t| &t.id).collect();
        assert_eq!(thread_ids.len(), threads.len());
        let comments: Vec<_> = threads.iter().flat_map(|t| &t.comments).collect();
        let comment_ids: HashSet<_> = comments.iter().map(|c| &c.id).collect();
        assert_eq!(comment_ids.len(), comments.len());
    }

    #[test]
    fn timestamps_are_relative_to_now_and_ordered() {
        for c in fixtures().changes {
            let s = &c.detail.summary;
            assert!(s.created_at <= s.updated_at, "{}", s.id);
            assert!(s.updated_at <= now(), "{}", s.id);
            for comment in c.threads.iter().flat_map(|t| &t.comments) {
                assert!(comment.created_at <= now());
            }
        }
        let shifted = load(Timestamp(now().0 + 86_400)).unwrap();
        assert_eq!(
            shifted.changes[0].detail.summary.updated_at.0 - 86_400,
            fixtures().changes[0].detail.summary.updated_at.0
        );
    }

    #[test]
    fn ci_covers_pass_running_and_fail_with_matching_checks() {
        let f = fixtures();
        let states: HashSet<_> = f
            .changes
            .iter()
            .flat_map(|c| &c.checks)
            .map(|c| c.state)
            .collect();
        for s in [CiState::Pass, CiState::Running, CiState::Fail] {
            assert!(states.contains(&s), "{s:?}");
        }
        for c in &f.changes {
            let checks = &c.checks;
            let summary = c.detail.summary.ci;
            match summary {
                CiState::Fail => assert!(checks.iter().any(|k| k.state == CiState::Fail)),
                CiState::Running => assert!(checks.iter().any(|k| k.state == CiState::Running)),
                CiState::Pass => assert!(checks.iter().all(|k| k.state == CiState::Pass)),
                CiState::None => assert!(checks.is_empty()),
                _ => {}
            }
        }
    }

    #[test]
    fn menus_change_has_a_thread_and_a_pending_suggestion() {
        let f = fixtures();
        let c = f
            .changes
            .iter()
            .find(|c| c.files.iter().any(|f| f.path.ends_with("menus.rs")))
            .expect("a menus.rs change");
        assert_eq!(bucket_of(c), Bucket::Wait);
        let file = c
            .files
            .iter()
            .find(|f| f.path.ends_with("menus.rs"))
            .unwrap();
        let thread = c.threads.first().expect("a thread");
        assert!(thread.comments.len() >= 2 && !thread.resolved);
        let line = thread.line.unwrap();
        let (ranges, _) = hunks(file.patch.as_deref().unwrap());
        assert!(ranges.iter().any(|(s, n)| (*s..s + n).contains(&line)));
        let draft = c.draft.comments.first().expect("a pending comment");
        assert!(draft.body.contains("```suggestion"));
        assert!((ranges.iter()).any(|(s, n)| (*s..s + n).contains(&draft.line)));
        assert_eq!(draft.path, file.path);
    }

    #[test]
    fn patches_are_well_formed_unified_diffs() {
        for (name, body) in PATCHES {
            let (ranges, ok) = hunks(body);
            assert!(!ranges.is_empty(), "{name} has no hunks");
            assert!(ok, "{name} has hunk counts that don't match its lines");
        }
        for c in fixtures().changes {
            let s = &c.detail.summary;
            assert_eq!(s.files as usize, c.files.len());
            assert_eq!(s.adds, c.files.iter().map(|f| f.adds).sum::<u32>());
            assert!(c.files.iter().all(|f| f.adds + f.dels > 0));
        }
    }

    /// New-side `(start, len)` of each hunk, and whether every header's counts match its body.
    fn hunks(patch: &str) -> (Vec<(u32, u32)>, bool) {
        let mut ranges = Vec::new();
        let mut ok = true;
        let mut lines = patch.lines().peekable();
        while let Some(header) = lines.next() {
            let spec = header.strip_prefix("@@ -").expect("hunk header");
            let (spans, _) = spec.split_once(" @@").expect("closing @@");
            let (old, new) = spans.split_once(" +").unwrap();
            let pair = |s: &str| {
                let (a, b) = s.split_once(',').unwrap();
                (a.parse::<u32>().unwrap(), b.parse::<u32>().unwrap())
            };
            let (_, old_len) = pair(old);
            let (new_start, new_len) = pair(new);
            let (mut o, mut n) = (0, 0);
            while let Some(next) = lines.peek() {
                if next.starts_with("@@") {
                    break;
                }
                match next.as_bytes().first() {
                    Some(b'+') => n += 1,
                    Some(b'-') => o += 1,
                    _ => {
                        o += 1;
                        n += 1;
                    }
                }
                lines.next();
            }
            ok &= o == old_len && n == new_len;
            ranges.push((new_start, new_len));
        }
        (ranges, ok)
    }

    #[test]
    fn unknown_ages_and_patches_are_reported() {
        assert!(ago(now(), "soon").is_err());
        let raw = RawFile {
            path: "x".into(),
            old_path: None,
            status: FileStatus::Added,
            patch: "nope.patch".into(),
        };
        let id = fixtures().changes[0].id().clone();
        assert!(file(raw, &id).is_err());
    }

    #[test]
    fn web_urls_follow_each_forge() {
        let f = fixtures();
        let gh = f
            .changes
            .iter()
            .find(|c| c.id().kind == ForgeKind::GitHub)
            .unwrap();
        assert_eq!(
            gh.detail.web_url.as_str(),
            "https://github.com/liminal-hq/review-buddy/pull/214"
        );
        let gl = f.changes.iter().find(|c| c.id().number == 1182).unwrap();
        assert_eq!(
            gl.detail.web_url.as_str(),
            "https://gitlab.platform.example/platform/flow/-/merge_requests/1182"
        );
    }
}
