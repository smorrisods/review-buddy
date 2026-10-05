//! `review-buddy pr view` and `pr open`.

use std::collections::BTreeSet;

use rb_core::triage::{triage, TriageConfig, TriageOutcome};
use rb_core::{
    ChangeDetail, ChangeId, ChangeState, Check, CiState, FilePatch, FileStatus, Provider,
    ReviewerState, Source, Thread, Timestamp,
};
use rb_platform::{browser, CommandRunner, SystemRunner};
use rb_theme::Role;
use serde_json::{json, Value};

use super::context::Context;
use super::error::CmdError;
use super::markdown;
use super::output::{self, json as json_out, Painter};
use super::selector::{Target, Which};
use super::DEMO_LABEL;
use crate::app::queue::opened_phrase;

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
    "body",
    "checks",
    "files",
    "comments",
];

#[derive(Debug, Clone, Default)]
pub struct ViewOptions {
    pub selector: Option<String>,
    pub comments: bool,
    pub web: bool,
}

/// What a run produced. Printing happens at the edge so the logic can be tested.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: Option<String>,
    /// stdout is prose that may go through the pager on a terminal.
    pub paged: bool,
}

pub fn run(ctx: &Context, options: &ViewOptions) -> Result<(), CmdError> {
    let outcome = execute(ctx, options, &SystemRunner)?;
    if let Some(note) = &outcome.stderr {
        eprintln!("{note}");
    }
    if outcome.paged {
        output::print_paged(&ctx.out, &outcome.stdout)
    } else {
        output::print(&outcome.stdout)
    }
}

/// Does the work with an injected command runner, so tests never start a browser.
pub fn execute(
    ctx: &Context,
    options: &ViewOptions,
    runner: &dyn CommandRunner,
) -> Result<Outcome, CmdError> {
    let web = options.web || ctx.args.web;
    let plan = json_out::plan(ctx.args.json.as_deref(), ctx.args.jq.as_deref());
    if !web && plan == json_out::JsonPlan::ListFields {
        let text =
            json_out::render(&ctx.args, FIELDS, &Value::Null, false).expect("--json is set")?;
        return Ok(Outcome {
            stdout: text,
            ..Outcome::default()
        });
    }

    let target = ctx.resolve_selector(options.selector.as_deref())?;
    let source = find_source(ctx, &target)?;
    let provider = ctx.provider_for(&source)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CmdError::failed(format!("Couldn't start the async runtime: {e}.")))?;

    if web {
        let id = runtime.block_on(find_id(provider.as_ref(), &source, &target))?;
        return open_in_browser(ctx, provider.web_url(&id).as_str(), &id, runner);
    }

    let want = Want::from_plan(&plan, options.comments);
    let view = runtime.block_on(fetch(provider.as_ref(), &source, &target, &want, ctx.now()))?;

    let pretty = ctx.out.tty;
    if let Some(text) = json_out::render(&ctx.args, FIELDS, &view.to_json(), pretty) {
        return Ok(Outcome {
            stdout: text?,
            ..Outcome::default()
        });
    }
    Ok(Outcome {
        stdout: view.human(options.comments, ctx.out.width, &ctx.out.painter),
        paged: true,
        ..Outcome::default()
    })
}

fn open_in_browser(
    ctx: &Context,
    url: &str,
    id: &ChangeId,
    runner: &dyn CommandRunner,
) -> Result<Outcome, CmdError> {
    if ctx.is_demo() {
        return Ok(Outcome {
            stdout: format!("{url}\n"),
            stderr: Some(format!(
                "Demo mode doesn't open a browser, so here's the link instead. {DEMO_LABEL}"
            )),
            paged: false,
        });
    }
    browser::open_url(url, runner).map_err(|e| {
        CmdError::failed(format!(
            "Couldn't open a browser: {e}.\nOpen this link yourself: {url}"
        ))
    })?;
    Ok(Outcome {
        stdout: String::new(),
        stderr: Some(format!("Opened {id} in your browser.")),
        paged: false,
    })
}

pub(super) fn find_source(ctx: &Context, target: &Target) -> Result<Source, CmdError> {
    ctx.sources()?
        .into_iter()
        .find(|s| s.id == target.source)
        .ok_or_else(|| {
            CmdError::usage("That source isn't available. Check review-buddy source list.")
        })
}

/// Which expensive parts to fetch. The description rides along with the detail call.
#[derive(Debug, Default)]
struct Want {
    checks: bool,
    threads: bool,
    files: bool,
}

impl Want {
    fn from_plan(plan: &json_out::JsonPlan, comments: bool) -> Self {
        match plan {
            json_out::JsonPlan::Emit {
                fields: Some(fields),
                ..
            } => {
                let has = |name: &str| fields.iter().any(|f| f == name);
                Self {
                    checks: has("checks"),
                    threads: has("comments"),
                    files: has("files"),
                }
            }
            json_out::JsonPlan::Emit { fields: None, .. } => Self {
                checks: true,
                threads: true,
                files: true,
            },
            _ => Self {
                checks: true,
                threads: comments,
                files: false,
            },
        }
    }
}

pub(super) async fn find_id(
    provider: &dyn Provider,
    source: &Source,
    target: &Target,
) -> Result<ChangeId, CmdError> {
    match &target.which {
        Which::Number(number) => Ok(ChangeId {
            source_id: source.id.clone(),
            kind: target.kind,
            repo: target.repo.clone(),
            number: *number,
        }),
        Which::Branch(branch) => {
            let page = provider.list_changes(&source.scope, None).await?;
            page.items
                .into_iter()
                .find(|c| {
                    c.id.repo == target.repo && &c.branch == branch && c.state == ChangeState::Open
                })
                .map(|c| c.id)
                .ok_or_else(|| {
                    CmdError::usage(format!(
                        "Couldn't find an open change for branch {branch} in {}.\nPass a number or a URL, e.g. review-buddy pr view 214.",
                        target.repo
                    ))
                })
        }
    }
}

async fn fetch(
    provider: &dyn Provider,
    source: &Source,
    target: &Target,
    want: &Want,
    now: Timestamp,
) -> Result<View, CmdError> {
    let id = find_id(provider, source, target).await?;
    let detail = provider.change_detail(&id).await?;
    let checks = if want.checks {
        provider.checks(&id).await?
    } else {
        Vec::new()
    };
    let threads = if want.threads {
        provider.threads(&id).await?
    } else {
        Vec::new()
    };
    let files = if want.files {
        provider.files(&id).await?
    } else {
        Vec::new()
    };
    let outcome = triage(&detail.summary, &TriageConfig::default(), now);
    Ok(View {
        host: source.host.clone(),
        detail,
        checks,
        threads,
        files,
        outcome,
        now,
        want_checks: want.checks,
    })
}

struct View {
    host: String,
    detail: ChangeDetail,
    checks: Vec<Check>,
    threads: Vec<Thread>,
    files: Vec<FilePatch>,
    outcome: TriageOutcome,
    now: Timestamp,
    want_checks: bool,
}

impl View {
    fn to_json(&self) -> Value {
        let s = &self.detail.summary;
        let mut value = json!({
            "source": s.id.source_id.as_str(),
            "forge": s.id.kind,
            "host": self.host,
            "repo": s.id.repo,
            "number": s.id.number,
            "ref": s.id.short_ref(),
            "url": self.detail.web_url.as_str(),
            "title": s.title,
            "author": s.author,
            "state": s.state,
            "isDraft": s.draft,
            "createdAt": iso(s.created_at),
            "updatedAt": iso(s.updated_at),
            "headRefName": s.branch,
            "baseRefName": s.base,
            "headSha": s.head_sha,
            "additions": s.adds,
            "deletions": s.dels,
            "changedFiles": s.files,
            "ci": s.ci,
            "reviewers": s.reviewers,
            "myRole": s.my_role,
            "myReview": s.my_review,
            "bucket": self.outcome.bucket,
            "bucketReason": self.outcome.reason.to_string(),
            "labels": s.labels,
            "body": self.detail.body,
        });
        let map = value.as_object_mut().expect("an object");
        if self.want_checks {
            map.insert(
                "checks".into(),
                json!(self
                    .checks
                    .iter()
                    .map(|c| json!({
                        "name": c.name,
                        "state": c.state,
                        "url": c.url.as_ref().map(|u| u.as_str()),
                        "required": c.required,
                        "durationSecs": c.duration_secs(),
                    }))
                    .collect::<Vec<_>>()),
            );
        }
        map.insert(
            "files".into(),
            json!(self
                .files
                .iter()
                .map(|f| json!({
                    "path": f.path,
                    "oldPath": f.old_path,
                    "status": file_status(f.status),
                    "additions": f.adds,
                    "deletions": f.dels,
                }))
                .collect::<Vec<_>>()),
        );
        map.insert("comments".into(), json!(self.comments_json()));
        value
    }

    fn comments_json(&self) -> Vec<Value> {
        self.threads
            .iter()
            .flat_map(|t| {
                t.comments.iter().map(move |c| {
                    json!({
                        "author": c.author,
                        "body": c.body,
                        "createdAt": iso(c.created_at),
                        "path": t.path,
                        "startLine": t.start_line,
                        "line": t.line,
                        "pending": c.pending || t.pending,
                        "resolved": t.resolved,
                        "outdated": t.outdated,
                    })
                })
            })
            .collect()
    }

    fn human(&self, comments: bool, width: usize, p: &Painter) -> String {
        let s = &self.detail.summary;
        let mut out = String::new();
        out.push_str(&p.paint(Role::TextBright, &s.title));
        out.push('\n');
        let mut status = vec![s.id.short_ref(), state_word(s.state).to_string()];
        if s.draft {
            status.push("draft".into());
        }
        out.push_str(&p.paint(Role::Muted, &status.join(" · ")));
        out.push('\n');
        out.push_str(&format!(
            "{} wants {} → {}\n",
            p.paint(Role::Interactive, &s.author),
            p.paint(Role::Cyan, &s.branch),
            s.base
        ));
        out.push_str(&format!(
            "+{} −{} · {} · {}\n\n",
            s.adds,
            s.dels,
            plural(s.files, "file"),
            opened_phrase(self.now, s.created_at)
        ));

        let mut rows: Vec<(&str, Vec<String>)> = Vec::new();
        rows.push((
            "Bucket",
            vec![format!(
                "{} · {}",
                self.outcome.bucket.title(),
                self.outcome.reason
            )],
        ));
        rows.push(("Reviewers", vec![reviewers_line(&s.reviewers)]));
        rows.push(("Checks", self.checks_lines(p)));
        if !s.labels.is_empty() {
            rows.push(("Labels", vec![s.labels.join(", ")]));
        }
        rows.push(("Link", vec![self.detail.web_url.to_string()]));
        for (label, lines) in rows {
            for (i, line) in lines.iter().enumerate() {
                let head = if i == 0 { label } else { "" };
                out.push_str(&format!(
                    "{} {line}\n",
                    p.paint(Role::Muted, &format!("{head:<9}"))
                ));
            }
        }

        let body = markdown::render(&self.detail.body, width, p);
        out.push('\n');
        if body.is_empty() {
            out.push_str(&p.paint(Role::Muted, "No description."));
            out.push('\n');
        } else {
            out.push_str(&body);
        }
        if comments {
            out.push('\n');
            out.push_str(&self.comments_section(width, p));
        }
        out
    }

    fn checks_lines(&self, p: &Painter) -> Vec<String> {
        let count = |state| self.checks.iter().filter(|c| c.state == state).count();
        let mut parts = Vec::new();
        for (state, word) in [
            (CiState::Fail, "failing"),
            (CiState::Running, "running"),
            (CiState::Pass, "passing"),
        ] {
            let n = count(state);
            if n > 0 {
                parts.push(format!("{n} {word}"));
            }
        }
        let mut lines = vec![if parts.is_empty() {
            "none reported".to_string()
        } else {
            parts.join(" · ")
        }];
        for check in self.checks.iter().filter(|c| c.state != CiState::Pass) {
            let (word, role) = match check.state {
                CiState::Fail => ("fail", Role::Danger),
                CiState::Running => ("running", Role::Warning),
                _ => ("none", Role::Muted),
            };
            lines.push(format!(
                "{} {}",
                p.paint(role, &format!("{word:<7}")),
                check.name
            ));
        }
        lines
    }

    fn comments_section(&self, width: usize, p: &Painter) -> String {
        let total: usize = self.threads.iter().map(|t| t.comments.len()).sum();
        if total == 0 {
            return format!("{}\n", p.paint(Role::Muted, "No comments yet."));
        }
        let mut out = format!(
            "{}\n",
            p.paint(Role::TextBright, &format!("Comments ({total})"))
        );
        for thread in &self.threads {
            let place = match (&thread.path, thread.line) {
                (Some(path), Some(line)) => match thread.start_line {
                    Some(start) if start != line => format!("{path}:{start}-{line}"),
                    _ => format!("{path}:{line}"),
                },
                (Some(path), None) => path.clone(),
                _ => "Conversation".to_string(),
            };
            let mut tags = vec![place];
            if thread.resolved {
                tags.push("resolved".into());
            }
            if thread.outdated {
                tags.push("outdated".into());
            }
            if thread.pending {
                tags.push("pending".into());
            }
            out.push('\n');
            out.push_str(&p.paint(Role::Muted, &tags.join(" · ")));
            out.push('\n');
            for comment in &thread.comments {
                out.push_str(&format!(
                    "{} · {} ago\n",
                    p.paint(Role::Interactive, &comment.author),
                    age_word(self.now, comment.created_at)
                ));
                let body = markdown::render(&comment.body, width.saturating_sub(2), p);
                for line in body.lines() {
                    if line.is_empty() {
                        out.push('\n');
                    } else {
                        out.push_str(&format!("  {line}\n"));
                    }
                }
            }
        }
        out
    }
}

fn age_word(now: Timestamp, then: Timestamp) -> String {
    match crate::app::queue::age(now, then).as_str() {
        "now" => "moments".to_string(),
        a => a.to_string(),
    }
}

fn state_word(state: ChangeState) -> &'static str {
    match state {
        ChangeState::Open => "open",
        ChangeState::Merged => "merged",
        ChangeState::Closed => "closed",
    }
}

fn file_status(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "added",
        FileStatus::Modified => "modified",
        FileStatus::Removed => "removed",
        FileStatus::Renamed => "renamed",
    }
}

fn plural(n: u32, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn reviewers_line(reviewers: &[rb_core::Reviewer]) -> String {
    if reviewers.is_empty() {
        return "none".into();
    }
    let mut seen = BTreeSet::new();
    reviewers
        .iter()
        .filter(|r| seen.insert(r.login.clone()))
        .map(|r| {
            let state = match r.state {
                ReviewerState::Requested => "requested",
                ReviewerState::Approved => "approved",
                ReviewerState::Commented => "commented",
                ReviewerState::ChangesRequested => "requested changes",
            };
            format!("{} ({state})", r.login)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// ISO 8601 in UTC, like `2026-10-05T10:00:00Z`.
pub fn iso(ts: Timestamp) -> String {
    let days = ts.0.div_euclid(86_400);
    let secs = ts.0.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(all(test, feature = "demo"))]
mod tests {
    use super::*;
    use crate::cli::GlobalArgs;
    use crate::cmd::context::Terminal;
    use rb_platform::{CommandOutput, PlatformError};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Recorder {
        calls: RefCell<Vec<(String, Vec<String>)>>,
        fail: bool,
    }

    impl CommandRunner for Recorder {
        fn run(
            &self,
            program: &str,
            args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            self.calls.borrow_mut().push((
                program.to_string(),
                args.iter().map(|a| a.to_string()).collect(),
            ));
            if self.fail {
                return Err(PlatformError::Spawn {
                    program: program.into(),
                    reason: "not found".into(),
                });
            }
            Ok(CommandOutput {
                success: true,
                ..CommandOutput::default()
            })
        }
    }

    fn pipe() -> Terminal {
        Terminal {
            stdout_tty: false,
            stdin_tty: false,
            width: None,
        }
    }

    fn demo_ctx(json: Option<Vec<String>>, jq: Option<&str>) -> Context {
        let args = GlobalArgs {
            demo: true,
            frozen_time: Some("2026-10-05T10:00".into()),
            sources: vec!["liminal-hq".into()],
            repo: Some("liminal-hq/review-buddy".into()),
            json,
            jq: jq.map(str::to_string),
            ..GlobalArgs::default()
        };
        Context::build(args, pipe()).unwrap()
    }

    fn options(selector: &str) -> ViewOptions {
        ViewOptions {
            selector: Some(selector.into()),
            ..ViewOptions::default()
        }
    }

    #[test]
    fn iso_formats_utc_times() {
        assert_eq!(iso(Timestamp(0)), "1970-01-01T00:00:00Z");
        assert_eq!(iso(Timestamp(1_791_194_400)), "2026-10-05T10:00:00Z");
        assert_eq!(iso(Timestamp(951_782_400)), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn expensive_fields_are_only_wanted_when_named() {
        let plan = |fields: &[&str]| json_out::JsonPlan::Emit {
            fields: Some(fields.iter().map(|s| s.to_string()).collect()),
            jq: None,
        };
        let cheap = Want::from_plan(&plan(&["number", "title", "body"]), false);
        assert!(!cheap.checks && !cheap.threads && !cheap.files);
        let some = Want::from_plan(&plan(&["comments"]), false);
        assert!(some.threads && !some.checks && !some.files);
        let all = Want::from_plan(
            &json_out::JsonPlan::Emit {
                fields: None,
                jq: Some(".".into()),
            },
            false,
        );
        assert!(all.checks && all.threads && all.files);
        let human = Want::from_plan(&json_out::JsonPlan::Off, false);
        assert!(human.checks && !human.threads);
        assert!(Want::from_plan(&json_out::JsonPlan::Off, true).threads);
    }

    #[test]
    fn cheap_json_does_not_fetch_checks_or_comments() {
        let ctx = demo_ctx(Some(vec!["number".into(), "title".into()]), None);
        let out = execute(&ctx, &options("214"), &Recorder::default()).unwrap();
        assert_eq!(
            out.stdout.trim(),
            r#"{"number":214,"title":"Add a menu bar and keyboard-driven menus"}"#
        );

        let target = ctx.resolve_selector(Some("214")).unwrap();
        let source = find_source(&ctx, &target).unwrap();
        let provider = ctx.provider_for(&source).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let view = rt
            .block_on(fetch(
                provider.as_ref(),
                &source,
                &target,
                &Want::default(),
                ctx.now(),
            ))
            .unwrap();
        assert!(view.checks.is_empty() && view.threads.is_empty() && view.files.is_empty());
        assert!(view.to_json().get("checks").is_none());
    }

    #[test]
    fn requested_expensive_fields_are_fetched() {
        let ctx = demo_ctx(
            Some(vec!["comments".into(), "checks".into(), "body".into()]),
            None,
        );
        let out = execute(&ctx, &options("214"), &Recorder::default()).unwrap();
        let value: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(value["comments"].as_array().unwrap().len(), 3);
        assert!(!value["checks"].as_array().unwrap().is_empty());
        assert!(value["body"].as_str().unwrap().contains("menu bar"));
    }

    #[test]
    fn listing_fields_needs_no_selector_or_network() {
        let ctx = demo_ctx(Some(vec![String::new()]), None);
        let out = execute(&ctx, &ViewOptions::default(), &Recorder::default()).unwrap();
        assert!(out.stdout.starts_with("source\nforge\n"));
        assert!(!out.paged);
    }

    #[test]
    fn human_view_has_every_section() {
        let ctx = demo_ctx(None, None);
        let opts = ViewOptions {
            comments: true,
            ..options("214")
        };
        let out = execute(&ctx, &opts, &Recorder::default()).unwrap();
        assert!(out.paged);
        for needle in [
            "Add a menu bar and keyboard-driven menus",
            "liminal-hq/review-buddy#214 · open",
            "ada wants ada/menus → main",
            "+32 −1 · 3 files · opened 3d ago",
            "Waiting on you · your review is requested",
            "smorris (requested), jo (commented)",
            "1 running · 2 passing",
            "running test (linux)",
            "- `Menu::select_next`",
            "Comments (3)",
            "src/ui/menus.rs:44",
        ] {
            assert!(out.stdout.contains(needle), "{needle}\n{}", out.stdout);
        }
        let without = execute(&ctx, &options("214"), &Recorder::default()).unwrap();
        assert!(!without.stdout.contains("Comments ("));
    }

    #[test]
    fn branches_resolve_through_the_list() {
        let ctx = demo_ctx(Some(vec!["number".into()]), None);
        let out = execute(&ctx, &options("ada/menus"), &Recorder::default()).unwrap();
        assert_eq!(out.stdout.trim(), r#"{"number":214}"#);
        let err = execute(&ctx, &options("no/such-branch"), &Recorder::default()).unwrap_err();
        assert_eq!(err.exit().code(), 2);
        assert!(err.to_string().contains("no/such-branch"));
    }

    #[test]
    fn demo_web_prints_the_link_and_never_runs_a_browser() {
        let ctx = demo_ctx(None, None);
        let runner = Recorder::default();
        let opts = ViewOptions {
            web: true,
            ..options("214")
        };
        let out = execute(&ctx, &opts, &runner).unwrap();
        assert_eq!(
            out.stdout,
            "https://github.com/liminal-hq/review-buddy/pull/214\n"
        );
        assert!(out.stderr.unwrap().contains("(demo)"));
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn live_web_goes_through_the_injected_runner() {
        let ctx = Context::from_env(
            GlobalArgs::default(),
            pipe(),
            Box::new(rb_paths::MapEnv::new("/nonexistent-home")),
        )
        .unwrap();
        let id = ChangeId {
            source_id: rb_core::SourceId::new("work"),
            kind: rb_core::ForgeKind::GitHub,
            repo: "a/b".into(),
            number: 7,
        };
        let url = "https://github.com/a/b/pull/7";
        let runner = Recorder::default();
        let out = open_in_browser(&ctx, url, &id, &runner).unwrap();
        assert!(out.stdout.is_empty() && out.stderr.unwrap().contains("a/b#7"));
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].1.contains(&url.to_string()));

        let failing = Recorder {
            fail: true,
            ..Recorder::default()
        };
        let err = open_in_browser(&ctx, url, &id, &failing).unwrap_err();
        assert_eq!(err.exit().code(), 1);
        assert!(err.to_string().contains(url));
    }

    #[test]
    fn missing_changes_exit_1_with_next_steps() {
        let ctx = demo_ctx(None, None);
        let err = execute(&ctx, &options("9999"), &Recorder::default()).unwrap_err();
        assert_eq!(err.exit().code(), 1);
    }

    #[test]
    fn reviewers_are_deduplicated_and_readable() {
        use rb_core::Reviewer;
        let r = |login: &str, state| Reviewer {
            login: login.into(),
            state,
        };
        assert_eq!(reviewers_line(&[]), "none");
        assert_eq!(
            reviewers_line(&[
                r("a", ReviewerState::ChangesRequested),
                r("a", ReviewerState::Approved),
                r("b", ReviewerState::Requested),
            ]),
            "a (requested changes), b (requested)"
        );
    }
}
