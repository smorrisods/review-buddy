//! `review-buddy pr checks`: check runs or pipeline jobs, with an optional watch.

use std::time::Duration;

use rb_core::{ChangeId, Check, CiState, Provider};
use rb_theme::Role;
use serde_json::{json, Value};

use super::context::Context;
use super::error::CmdError;
use super::output::{self, json, Cell, Column, Table};
use super::selector::Which;
use crate::ui::dashboard::ci_look;

const FIELDS: &[&str] = &["name", "state", "durationSecs", "url", "required"];

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub watch: bool,
    pub interval: u64,
    pub fail_fast: bool,
    pub required: bool,
}

/// Where the checks stand overall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Passed,
    Failed,
    Running,
}

pub fn verdict(checks: &[Check]) -> Verdict {
    if checks.iter().any(|c| c.state == CiState::Fail) {
        Verdict::Failed
    } else if checks.iter().any(|c| c.state == CiState::Running) {
        Verdict::Running
    } else {
        Verdict::Passed
    }
}

/// Keeps only required checks, but only where the forge says which ones are; otherwise all of
/// them, since there is nothing to narrow by.
pub fn narrow_required(checks: Vec<Check>) -> Vec<Check> {
    if checks.iter().all(|c| c.required.is_none()) {
        return checks;
    }
    checks
        .into_iter()
        .filter(|c| c.required == Some(true))
        .collect()
}

/// Fetches the checks, and with `watch` keeps fetching until they settle (or the first failure
/// with `fail_fast`). `progress` sees every snapshot except the last; `sleep` waits between polls.
pub async fn poll(
    provider: &dyn Provider,
    id: &ChangeId,
    opts: &Options,
    progress: &mut dyn FnMut(&[Check]),
    sleep: &mut dyn FnMut(Duration),
) -> Result<(Vec<Check>, Verdict), CmdError> {
    loop {
        let mut checks = provider.checks(id).await?;
        if opts.required {
            checks = narrow_required(checks);
        }
        let verdict = verdict(&checks);
        let running = checks.iter().any(|c| c.state == CiState::Running);
        let settled = !opts.watch || !running || (opts.fail_fast && verdict == Verdict::Failed);
        if settled {
            return Ok((checks, verdict));
        }
        progress(&checks);
        sleep(Duration::from_secs(opts.interval));
    }
}

pub fn duration_text(secs: i64) -> String {
    match (secs / 3600, secs / 60 % 60, secs % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s:02}s"),
        (h, m, _) => format!("{h}h {m:02}m"),
    }
}

fn state_word(state: CiState) -> &'static str {
    match state {
        CiState::Pass => "pass",
        CiState::Running => "running",
        CiState::Fail => "fail",
        CiState::None => "none",
        CiState::Neutral => "neutral",
        CiState::Skipped => "skipped",
        CiState::Cancelled => "cancelled",
    }
}

fn table(checks: &[Check]) -> Table {
    let mut table = Table::new(vec![
        Column::fixed("STATE"),
        Column::flex("CHECK"),
        Column::fixed("TIME"),
        Column::fixed("URL"),
    ]);
    for check in checks {
        let (glyph, role) = ci_look(check.state);
        let state = Cell::styled(format!("{glyph} {}", state_word(check.state)), role)
            .with_pipe(state_word(check.state));
        let time = match check.duration_secs() {
            Some(s) => Cell::plain(duration_text(s)).with_pipe(s.to_string()),
            None => Cell::styled("–", Role::Muted).with_pipe(""),
        };
        let url = check
            .url
            .as_ref()
            .map_or_else(String::new, ToString::to_string);
        table.push(vec![
            state,
            Cell::plain(check.name.clone()),
            time,
            Cell::plain(url),
        ]);
    }
    table
}

fn checks_json(checks: &[Check]) -> Value {
    Value::Array(
        checks
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "state": state_word(c.state),
                    "durationSecs": c.duration_secs(),
                    "url": c.url.as_ref().map(ToString::to_string),
                    "required": c.required,
                })
            })
            .collect(),
    )
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The error that carries the exit code for an unsettled or failed verdict.
pub fn outcome(checks: &[Check], verdict: Verdict) -> Result<(), CmdError> {
    let count = |s: CiState| checks.iter().filter(|c| c.state == s).count();
    match verdict {
        Verdict::Passed => Ok(()),
        Verdict::Failed => Err(CmdError::failed(format!(
            "{} failed.",
            plural(count(CiState::Fail), "check", "checks")
        ))),
        Verdict::Running => Err(CmdError::ChecksPending(format!(
            "{} still running.\nUse --watch to wait for them.",
            plural(count(CiState::Running), "check is", "checks are")
        ))),
    }
}

pub fn run(ctx: &Context, selector: Option<&str>, opts: &Options) -> Result<(), CmdError> {
    let target = ctx.resolve_selector(selector)?;
    let Which::Number(number) = target.which else {
        return Err(CmdError::usage(
            "Checks need a change number.\nTry review-buddy pr checks 214, or give owner/repo#214.",
        ));
    };
    let source = ctx
        .sources()?
        .into_iter()
        .find(|s| s.id == target.source)
        .ok_or_else(|| {
            CmdError::usage("That source isn't enabled.\nCheck review-buddy source list.")
        })?;
    let provider = ctx.provider_for(&source)?;
    let id = ChangeId {
        source_id: target.source,
        kind: target.kind,
        repo: target.repo,
        number,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CmdError::failed(format!("Couldn't start the runtime: {e}.")))?;
    let mut progress = |checks: &[Check]| {
        eprint!("{}", ctx.out.table(&table(checks)));
        eprintln!("Checking again in {}s…", opts.interval);
    };
    let mut sleep = |wait: Duration| sleep_for(ctx, &id, wait);
    let (checks, verdict) = runtime.block_on(poll(
        provider.as_ref(),
        &id,
        opts,
        &mut progress,
        &mut sleep,
    ))?;

    if let Some(text) = json::render(&ctx.args, FIELDS, &checks_json(&checks), ctx.out.tty) {
        output::print(&text?)?;
    } else if checks.is_empty() {
        output::print("No checks reported.\n")?;
    } else {
        output::print(&ctx.out.table(&table(&checks)))?;
    }
    outcome(&checks, verdict)
}

/// Waits between polls. In demo mode nothing real is pending, so CI moves on instead.
fn sleep_for(ctx: &Context, id: &ChangeId, wait: Duration) {
    #[cfg(feature = "demo")]
    if let Some(world) = ctx.demo_world() {
        world.settle_next_running(id);
        return;
    }
    let _ = (ctx, id);
    std::thread::sleep(wait);
}

#[cfg(all(test, feature = "demo"))]
mod tests {
    use super::*;
    use crate::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
    use rb_core::{ForgeKind, Timestamp};

    fn check(name: &str, state: CiState, required: Option<bool>) -> Check {
        Check {
            name: name.into(),
            state,
            url: None,
            started_at: Some(Timestamp(1000)),
            completed_at: Some(Timestamp(1075)),
            required,
        }
    }

    fn opts() -> Options {
        Options {
            watch: true,
            interval: 10,
            fail_fast: false,
            required: false,
        }
    }

    fn world_and_id() -> (DemoWorld, ChangeId) {
        let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
        let source = world
            .sources()
            .into_iter()
            .find(|s| s.id.0 == "liminal-hq")
            .unwrap();
        let id = ChangeId {
            source_id: source.id,
            kind: ForgeKind::GitHub,
            repo: "liminal-hq/review-buddy".into(),
            number: 214,
        };
        (world, id)
    }

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn verdicts_rank_failure_over_running_over_pass() {
        let p = check("a", CiState::Pass, None);
        let r = check("b", CiState::Running, None);
        let f = check("c", CiState::Fail, None);
        assert_eq!(verdict(std::slice::from_ref(&p)), Verdict::Passed);
        assert_eq!(verdict(&[]), Verdict::Passed);
        assert_eq!(verdict(&[p.clone(), r.clone()]), Verdict::Running);
        assert_eq!(verdict(&[p, r, f]), Verdict::Failed);
    }

    #[test]
    fn required_narrows_only_where_the_forge_reports_it() {
        let unknown = vec![check("a", CiState::Pass, None)];
        assert_eq!(narrow_required(unknown.clone()), unknown);
        let mixed = vec![
            check("a", CiState::Fail, Some(false)),
            check("b", CiState::Pass, Some(true)),
        ];
        let kept = narrow_required(mixed);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "b");
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration_text(9), "9s");
        assert_eq!(duration_text(75), "1m 15s");
        assert_eq!(duration_text(3720), "1h 02m");
    }

    #[test]
    fn exit_codes_map_from_verdicts() {
        use crate::cmd::error::Exit;
        let r = [check("a", CiState::Running, None)];
        let f = [check("a", CiState::Fail, None)];
        assert!(outcome(&[], Verdict::Passed).is_ok());
        assert_eq!(
            outcome(&f, Verdict::Failed).unwrap_err().exit(),
            Exit::Failure
        );
        assert_eq!(
            outcome(&r, Verdict::Running).unwrap_err().exit(),
            Exit::ChecksPending
        );
    }

    #[test]
    fn piped_rows_use_words_and_tty_rows_use_glyphs() {
        let t = table(&[check("fmt", CiState::Pass, None)]);
        let piped = output::render_tsv(&t);
        assert_eq!(piped, "pass\tfmt\t75\t\n");
        let tty = output::render_table(&t, 80, &output::Painter::plain());
        assert!(tty.contains("● pass"), "{tty}");
    }

    #[test]
    fn without_watch_a_running_snapshot_returns_immediately() {
        let (world, id) = world_and_id();
        let provider = world.provider(ForgeKind::GitHub);
        let mut o = opts();
        o.watch = false;
        let mut sleeps = 0;
        let (_, v) = block_on(poll(&provider, &id, &o, &mut |_| {}, &mut |_| sleeps += 1)).unwrap();
        assert_eq!(v, Verdict::Running);
        assert_eq!(sleeps, 0);
    }

    #[test]
    fn watch_polls_until_the_demo_checks_settle() {
        let (world, id) = world_and_id();
        let provider = world.provider(ForgeKind::GitHub);
        let mut seen = 0;
        let mut waits = Vec::new();
        let (checks, v) = block_on(poll(
            &provider,
            &id,
            &opts(),
            &mut |_| seen += 1,
            &mut |d| {
                waits.push(d);
                world.settle_next_running(&id);
            },
        ))
        .unwrap();
        assert_eq!(v, Verdict::Passed);
        assert_eq!(seen, 1);
        assert_eq!(waits, vec![Duration::from_secs(10)]);
        assert!(checks.iter().all(|c| c.state != CiState::Running));
        assert_eq!(checks[2].duration_secs(), Some(120));
    }

    #[test]
    fn fail_fast_stops_on_the_first_failure_while_others_run() {
        let (world, id) = world_and_id();
        let provider = world.provider(ForgeKind::GitHub);
        let mut o = opts();
        o.fail_fast = true;
        let mut polls = 0;
        let (checks, v) = block_on(poll(&provider, &id, &o, &mut |_| {}, &mut |_| {
            polls += 1;
            world.set_check_state(&id, "fmt", CiState::Fail);
        }))
        .unwrap();
        assert_eq!(v, Verdict::Failed);
        assert_eq!(polls, 1);
        assert!(checks.iter().any(|c| c.state == CiState::Running));
    }

    #[test]
    fn without_fail_fast_watch_waits_for_everything() {
        let (world, id) = world_and_id();
        let provider = world.provider(ForgeKind::GitHub);
        let mut polls = 0;
        let (_, v) = block_on(poll(&provider, &id, &opts(), &mut |_| {}, &mut |_| {
            polls += 1;
            world.set_check_state(&id, "fmt", CiState::Fail);
            world.settle_next_running(&id);
        }))
        .unwrap();
        assert_eq!(v, Verdict::Failed);
        assert_eq!(polls, 1);
    }
}
