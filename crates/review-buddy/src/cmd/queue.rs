//! `review-buddy queue`: the triaged queue across sources, bucketed by the same engine as the
//! dashboard.

use rb_core::triage::Bucket;
use rb_core::{ChangeState, ChangeSummary};
use rb_theme::Role;

use super::changes::{self, ci_cell, Loaded, FIELDS};
use super::context::Context;
use super::error::CmdError;
use super::output::{self, json, Cell, Column, Painter, Table};
use crate::app::queue::{age, Queue, QueueSettings};
use crate::app::AppState;
use crate::cli::{BucketArg, QueueArgs};
use crate::config::ShowFilter;

const SHOW_NAMES: &str = "reviewing, assigned, authored, drafts, noise";

/// One bucket's worth of the queue, as indexes into the loaded changes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Group {
    bucket: Bucket,
    changes: Vec<usize>,
    /// Rows left out by the bucket limit.
    more: usize,
}

/// The queue after Show filters and `--bucket`, plus how many open changes the filters hid.
/// Group indexes point into `changes`, the changes that passed the Show filters.
#[derive(Debug)]
struct Plan {
    changes: Vec<ChangeSummary>,
    groups: Vec<Group>,
    hidden: usize,
}

pub fn run(ctx: &Context, args: &QueueArgs) -> Result<(), CmdError> {
    let show = show_filters(args, &ctx.config.triage.show)?;
    let loaded = changes::load(ctx, false)?;
    let limit = (ctx.out.tty && !args.all).then_some(ctx.config.triage.bucket_limit as usize);
    let plan = plan(&loaded, &show, &args.bucket, limit);

    if let Some(text) = json::render(&ctx.args, FIELDS, &to_json(&loaded, &plan), ctx.out.tty) {
        return output::print(&text?);
    }
    if plan.groups.is_empty() {
        if ctx.out.tty {
            eprintln!("{}", empty_note(plan.hidden));
        }
        return Ok(());
    }
    if ctx.out.tty {
        output::print(&tty_report(ctx, &loaded, &plan))
    } else {
        output::print(&ctx.out.table(&table(&loaded, &plan, false)))
    }
}

fn show_filters(args: &QueueArgs, configured: &[ShowFilter]) -> Result<Vec<ShowFilter>, CmdError> {
    if args.show.is_empty() {
        return Ok(configured.to_vec());
    }
    args.show
        .iter()
        .map(|name| {
            let name = name.trim().to_lowercase();
            [
                ShowFilter::Reviewing,
                ShowFilter::Assigned,
                ShowFilter::Authored,
                ShowFilter::Drafts,
                ShowFilter::Noise,
            ]
            .into_iter()
            .find(|f| f.as_str() == name)
            .ok_or_else(|| {
                CmdError::usage(format!(
                    "`{name}` isn't something to show.\nTry --show with: {SHOW_NAMES}."
                ))
            })
        })
        .collect()
}

fn bucket_of(arg: BucketArg) -> Bucket {
    match arg {
        BucketArg::Wait => Bucket::Wait,
        BucketArg::Look => Bucket::Look,
        BucketArg::Later => Bucket::Later,
        BucketArg::Noise => Bucket::Noise,
    }
}

fn plan(loaded: &Loaded, show: &[ShowFilter], only: &[BucketArg], limit: Option<usize>) -> Plan {
    let open = loaded
        .changes
        .iter()
        .filter(|c| c.state == ChangeState::Open)
        .count();
    let state = AppState {
        loaded: true,
        sources: loaded.sources.clone(),
        changes: loaded.changes.clone(),
        now: Some(loaded.now),
        queue_settings: QueueSettings {
            triage: loaded.triage_config.clone(),
            // Noise is listed as its own group here, so the shared queue keeps it apart.
            show: show
                .iter()
                .copied()
                .filter(|f| *f != ShowFilter::Noise)
                .collect(),
            bucket_limit: limit.unwrap_or(usize::MAX),
        },
        ..AppState::default()
    };
    let queue = Queue::build(&state, None, false);

    let wanted: Vec<Bucket> = only.iter().copied().map(bucket_of).collect();
    let mut buckets: Vec<(Bucket, Vec<usize>, usize)> = queue
        .sections
        .into_iter()
        .map(|s| (s.bucket, s.changes, s.more))
        .collect();
    if (show.contains(&ShowFilter::Noise) || wanted.contains(&Bucket::Noise))
        && !queue.noise.is_empty()
    {
        buckets.push((Bucket::Noise, queue.noise, 0));
    }
    let shown: usize = buckets.iter().map(|(_, c, more)| c.len() + more).sum();

    let groups = buckets
        .into_iter()
        .filter(|(bucket, ..)| wanted.is_empty() || wanted.contains(bucket))
        .map(|(bucket, mut changes, more)| {
            let extra = limit.map_or(0, |l| changes.len().saturating_sub(l));
            changes.truncate(changes.len() - extra);
            Group {
                bucket,
                changes,
                more: more + extra,
            }
        })
        .collect();
    Plan {
        changes: state.changes,
        groups,
        hidden: open.saturating_sub(shown),
    }
}

fn to_json(loaded: &Loaded, plan: &Plan) -> serde_json::Value {
    serde_json::Value::Array(
        plan.groups
            .iter()
            .flat_map(|g| &g.changes)
            .map(|i| loaded.json(&plan.changes[*i]))
            .collect(),
    )
}

/// Pipe rows are `bucket source ref ci author updatedAt title`; a terminal drops the bucket
/// (the headings carry it) and the source, and shows ages.
fn table(loaded: &Loaded, plan: &Plan, tty: bool) -> Table {
    let mut table = Table::new(if tty {
        vec![
            Column::fixed("Ref"),
            Column::fixed("CI"),
            Column::fixed("Author"),
            Column::fixed("Updated"),
            Column::flex("Title"),
        ]
    } else {
        vec![
            Column::fixed("Bucket"),
            Column::fixed("Source"),
            Column::fixed("Ref"),
            Column::fixed("CI"),
            Column::fixed("Author"),
            Column::fixed("Updated"),
            Column::flex("Title"),
        ]
    });
    for group in &plan.groups {
        for i in &group.changes {
            let c = &plan.changes[*i];
            let updated =
                Cell::plain(age(loaded.now, c.updated_at)).with_pipe(changes::iso(c.updated_at));
            if tty {
                table.push(vec![
                    Cell::styled(reference(c), Role::Accent).with_pipe(c.id.short_ref()),
                    ci_cell(c.ci),
                    Cell::plain(&c.author),
                    updated,
                    Cell::plain(&c.title),
                ]);
            } else {
                table.push(vec![
                    Cell::plain(group.bucket.config_key()),
                    Cell::plain(loaded.source_name(c)),
                    Cell::plain(c.id.short_ref()),
                    ci_cell(c.ci),
                    Cell::plain(&c.author),
                    updated,
                    Cell::plain(&c.title),
                ]);
            }
        }
    }
    table
}

fn reference(change: &ChangeSummary) -> String {
    let repo = change.id.repo.rsplit('/').next().unwrap_or(&change.id.repo);
    let sep = match change.id.kind {
        rb_core::ForgeKind::GitHub => '#',
        rb_core::ForgeKind::GitLab => '!',
    };
    format!("{repo}{sep}{}", change.id.number)
}

/// The aligned table with bucket headings spliced in above each group's rows.
fn tty_report(ctx: &Context, loaded: &Loaded, plan: &Plan) -> String {
    let rendered = ctx.out.table(&table(loaded, plan, true));
    let mut lines = rendered.lines();
    let painter = &ctx.out.painter;
    let mut out = String::new();
    out.push_str(lines.next().unwrap_or_default());
    out.push('\n');
    for group in plan.groups.iter() {
        out.push('\n');
        out.push_str(&heading(painter, group));
        out.push('\n');
        for line in lines.by_ref().take(group.changes.len()) {
            out.push_str(line);
            out.push('\n');
        }
        if group.more > 0 {
            let note = format!("+{} more · --all shows everything", group.more);
            out.push_str(&painter.paint(Role::Muted, &note));
            out.push('\n');
        }
    }
    out.push('\n');
    out.push_str(&painter.paint(Role::Muted, "── That's everything."));
    out.push('\n');
    if plan.hidden > 0 {
        let note = format!("{} hidden by your Show filters.", plan.hidden);
        out.push_str(&painter.paint(Role::Muted, &note));
        out.push('\n');
    }
    out
}

fn heading(painter: &Painter, group: &Group) -> String {
    let count = group.changes.len() + group.more;
    painter.paint(Role::Accent, &format!("{} · {count}", group.bucket.title()))
}

fn empty_note(hidden: usize) -> String {
    if hidden > 0 {
        format!("Your Show filters hide all {hidden}. Try --show {SHOW_NAMES}.")
    } else {
        "That's everything. Nothing is waiting on you.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_names_parse_and_reject_unknowns() {
        let args = QueueArgs {
            show: vec!["Drafts".into(), "noise".into()],
            ..QueueArgs::default()
        };
        assert_eq!(
            show_filters(&args, &[]).unwrap(),
            vec![ShowFilter::Drafts, ShowFilter::Noise]
        );
        let bad = QueueArgs {
            show: vec!["everything".into()],
            ..QueueArgs::default()
        };
        assert!(show_filters(&bad, &[])
            .unwrap_err()
            .to_string()
            .contains("--show"));
    }

    #[test]
    fn empty_note_distinguishes_filtered_from_clear() {
        assert!(empty_note(0).starts_with("That's everything."));
        assert!(empty_note(3).starts_with("Your Show filters hide all 3."));
    }
}
