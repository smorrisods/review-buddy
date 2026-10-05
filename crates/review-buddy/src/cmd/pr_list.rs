//! `review-buddy pr list`: changes across sources, filtered, closer to `gh pr list` than to the
//! queue.

use rb_core::{ChangeState, ChangeSummary, MyRole};
use rb_theme::Role;

use super::changes::{self, ci_cell, Loaded, FIELDS};
use super::context::Context;
use super::error::CmdError;
use super::output::{self, json, Cell, Column, Table};
use crate::app::queue::age;
use crate::cli::{PrListArgs, StateArg};

const ME: &str = "@me";

pub fn run(ctx: &Context, args: &PrListArgs) -> Result<(), CmdError> {
    check_flags(args)?;
    let wants_me = [&args.author, &args.assignee, &args.reviewer]
        .into_iter()
        .any(|f| f.as_deref().is_some_and(is_me));
    let loaded = changes::load(ctx, wants_me)?;
    let mut matched: Vec<&ChangeSummary> = loaded
        .changes
        .iter()
        .filter(|c| matches(c, args, loaded.me(c)))
        .collect();
    matched.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
    matched.truncate(args.limit as usize);

    let value = serde_json::Value::Array(matched.iter().map(|c| loaded.json(c)).collect());
    if let Some(text) = json::render(&ctx.args, FIELDS, &value, ctx.out.tty) {
        return output::print(&text?);
    }
    if matched.is_empty() {
        if ctx.out.tty {
            eprintln!("Nothing matches. Try a wider --state, or fewer filters.");
        }
        return Ok(());
    }
    output::print(&ctx.out.table(&table(&loaded, &matched, ctx.out.tty)))
}

fn is_me(value: &str) -> bool {
    value.eq_ignore_ascii_case(ME)
}

/// The change model doesn't carry assignees, so only your own assignments can be filtered.
fn check_flags(args: &PrListArgs) -> Result<(), CmdError> {
    match args.assignee.as_deref() {
        Some(who) if !is_me(who) => Err(CmdError::usage(format!(
            "Filtering by assignee {who} isn't available yet.\nTry --assignee @me for the changes assigned to you."
        ))),
        _ => Ok(()),
    }
}

fn same_login(wanted: &str, login: &str) -> bool {
    wanted.trim_start_matches('@').eq_ignore_ascii_case(login)
}

fn is_user(wanted: &str, login: &str, me: Option<&str>) -> bool {
    if is_me(wanted) {
        me.is_some_and(|me| me.eq_ignore_ascii_case(login))
    } else {
        same_login(wanted, login)
    }
}

fn matches(change: &ChangeSummary, args: &PrListArgs, me: Option<&str>) -> bool {
    let state_ok = match args.state {
        StateArg::Open => change.state == ChangeState::Open,
        StateArg::Closed => change.state == ChangeState::Closed,
        StateArg::Merged => change.state == ChangeState::Merged,
        StateArg::All => true,
    };
    state_ok
        && (!args.draft || change.draft)
        && args
            .author
            .as_deref()
            .is_none_or(|a| is_user(a, &change.author, me))
        && args
            .assignee
            .as_deref()
            .is_none_or(|_| change.my_role == MyRole::Assigned)
        && args.reviewer.as_deref().is_none_or(|r| {
            change.reviewers.iter().any(|x| is_user(r, &x.login, me))
                || (is_me(r) && change.my_role == MyRole::Reviewing)
        })
        && args.label.iter().all(|wanted| {
            change
                .labels
                .iter()
                .any(|l| l.eq_ignore_ascii_case(wanted.trim()))
        })
        && args
            .search
            .as_deref()
            .is_none_or(|query| searches(change, query))
}

/// Every word of the query must appear in the title, repository, author, branch or a label.
fn searches(change: &ChangeSummary, query: &str) -> bool {
    let haystack = [
        change.title.as_str(),
        change.id.repo.as_str(),
        change.author.as_str(),
        change.branch.as_str(),
    ]
    .into_iter()
    .chain(change.labels.iter().map(String::as_str))
    .collect::<Vec<_>>()
    .join("\n")
    .to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}

fn state_word(state: ChangeState, draft: bool) -> &'static str {
    match (state, draft) {
        (ChangeState::Open, true) => "draft",
        (ChangeState::Open, false) => "open",
        (ChangeState::Merged, _) => "merged",
        (ChangeState::Closed, _) => "closed",
    }
}

/// Pipe rows are `source ref state ci author updatedAt title`.
fn table(loaded: &Loaded, changes: &[&ChangeSummary], tty: bool) -> Table {
    let mut table = Table::new(vec![
        Column::fixed("Source"),
        Column::fixed("Ref"),
        Column::fixed("State"),
        Column::fixed("CI"),
        Column::fixed("Author"),
        Column::fixed("Updated"),
        Column::flex("Title"),
    ]);
    for c in changes {
        let reference = if tty {
            Cell::styled(c.id.short_ref(), Role::Accent)
        } else {
            Cell::plain(c.id.short_ref())
        };
        table.push(vec![
            Cell::plain(loaded.source_name(c)),
            reference,
            Cell::plain(state_word(c.state, c.draft)),
            ci_cell(c.ci),
            Cell::plain(&c.author),
            Cell::plain(age(loaded.now, c.updated_at)).with_pipe(changes::iso(c.updated_at)),
            Cell::plain(&c.title),
        ]);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::queue::tests::change;
    use rb_core::{Reviewer, ReviewerState};

    fn args() -> PrListArgs {
        PrListArgs {
            state: StateArg::Open,
            author: None,
            assignee: None,
            reviewer: None,
            label: vec![],
            draft: false,
            search: None,
            limit: 30,
        }
    }

    #[test]
    fn state_filter() {
        let mut merged = change(1, MyRole::Authored, 0);
        merged.state = ChangeState::Merged;
        assert!(!matches(&merged, &args(), None));
        let all = PrListArgs {
            state: StateArg::All,
            ..args()
        };
        assert!(matches(&merged, &all, None));
        let want_merged = PrListArgs {
            state: StateArg::Merged,
            ..args()
        };
        assert!(matches(&merged, &want_merged, None));
    }

    #[test]
    fn author_and_me() {
        let c = change(1, MyRole::Reviewing, 0);
        let by = |a: &str| PrListArgs {
            author: Some(a.into()),
            ..args()
        };
        assert!(matches(&c, &by("ADA"), None));
        assert!(matches(&c, &by("@ada"), None));
        assert!(!matches(&c, &by("grace"), None));
        assert!(matches(&c, &by("@me"), Some("ada")));
        assert!(!matches(&c, &by("@me"), Some("grace")));
    }

    #[test]
    fn reviewer_assignee_label_draft_and_search() {
        let mut c = change(7, MyRole::Reviewing, 0);
        c.reviewers = vec![Reviewer {
            login: "tess".into(),
            state: ReviewerState::Requested,
        }];
        c.labels = vec!["Bug".into(), "ui".into()];
        c.title = "Fix the Title menu".into();
        let with = |f: &dyn Fn(&mut PrListArgs)| {
            let mut a = args();
            f(&mut a);
            matches(&c, &a, Some("me"))
        };
        assert!(with(&|a| a.reviewer = Some("tess".into())));
        assert!(with(&|a| a.reviewer = Some("@me".into())));
        assert!(!with(&|a| a.reviewer = Some("zed".into())));
        assert!(!with(&|a| a.assignee = Some("@me".into())));
        assert!(with(&|a| a.label = vec!["bug".into(), "UI".into()]));
        assert!(!with(&|a| a.label = vec!["bug".into(), "nope".into()]));
        assert!(!with(&|a| a.draft = true));
        assert!(with(&|a| a.search = Some("title MENU".into())));
        assert!(!with(&|a| a.search = Some("title absent".into())));
    }

    #[test]
    fn other_assignees_are_explained() {
        let other = PrListArgs {
            assignee: Some("grace".into()),
            ..args()
        };
        assert!(check_flags(&other).unwrap_err().to_string().contains("@me"));
        let me = PrListArgs {
            assignee: Some("@me".into()),
            ..args()
        };
        assert!(check_flags(&me).is_ok());
    }

    #[test]
    fn draft_state_words() {
        assert_eq!(state_word(ChangeState::Open, true), "draft");
        assert_eq!(state_word(ChangeState::Merged, false), "merged");
    }
}
