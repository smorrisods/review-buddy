//! `review-buddy drafts …`: the review drafts saved on this computer. Nothing here talks to a
//! forge, and nothing here sends anything. A draft is your own unsent text; the pending review a
//! forge holds is a different thing and isn't touched.

use std::io::Write;
use std::path::Path;

use rb_core::{ChangeId, ForgeKind};
use serde_json::{json, Value};

use super::changes::iso;
use super::context::Context;
use super::error::CmdError;
use super::output::{self, json, Cell, Column, Table};
use super::prompt::{confirm_write, Interaction};
use super::selector::{self, Selector};
use super::DEMO_LABEL;
use crate::drafts::{self, StoredDraft};

const FIELDS: &[&str] = &[
    "source",
    "forge",
    "repo",
    "number",
    "ref",
    "title",
    "comments",
    "summary",
    "verdict",
    "headSha",
    "writtenAt",
];

fn verdict_word(draft: &StoredDraft) -> Option<&'static str> {
    use rb_core::Verdict;
    draft.verdict.map(|v| match v {
        Verdict::Approve => "approve",
        Verdict::RequestChanges => "request_changes",
        Verdict::Comment => "comment",
    })
}

fn forge_word(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::GitHub => "github",
        ForgeKind::GitLab => "gitlab",
    }
}

fn reference(id: &ChangeId) -> String {
    id.short_ref()
}

fn to_json(all: &[StoredDraft]) -> Value {
    Value::Array(
        all.iter()
            .map(|d| {
                json!({
                    "source": d.id.source_id.as_str(),
                    "forge": forge_word(d.id.kind),
                    "repo": d.id.repo,
                    "number": d.id.number,
                    "ref": reference(&d.id),
                    "title": d.title,
                    "comments": d.comments.len(),
                    "summary": d.summary,
                    "verdict": verdict_word(d),
                    "headSha": d.head_sha,
                    "writtenAt": iso(rb_core::Timestamp(d.written_at)),
                })
            })
            .collect(),
    )
}

fn table(all: &[StoredDraft]) -> Table {
    let mut table = Table::new(vec![
        Column::fixed("Source"),
        Column::fixed("Change"),
        Column::fixed("Comments"),
        Column::fixed("Saved"),
        Column::flex("Title"),
    ]);
    for d in all {
        table.push(vec![
            Cell::plain(d.id.source_id.as_str()),
            Cell::plain(reference(&d.id)),
            Cell::plain(d.comments.len().to_string()),
            Cell::plain(iso(rb_core::Timestamp(d.written_at))),
            Cell::plain(d.title.clone()),
        ]);
    }
    table
}

/// The drafts this run should look at. In demo mode that is the demo world's own pending
/// comments (nothing is on disk); otherwise the folder.
fn load(ctx: &Context) -> Result<Vec<StoredDraft>, CmdError> {
    let mut all = if ctx.is_demo() {
        demo_drafts(ctx)?
    } else {
        drafts::load_all(&ctx.paths.paths.drafts_dir())
    };
    if !ctx.args.sources.is_empty() {
        all.retain(|d| {
            ctx.args
                .sources
                .iter()
                .any(|s| s == d.id.source_id.as_str())
        });
    }
    Ok(all)
}

#[cfg(feature = "demo")]
fn demo_drafts(ctx: &Context) -> Result<Vec<StoredDraft>, CmdError> {
    let Some(world) = ctx.demo_world() else {
        return Ok(Vec::new());
    };
    let loaded = super::changes::load(ctx, false)?;
    let mut out = Vec::new();
    for change in &loaded.changes {
        let Some(draft) = world.draft(&change.id).filter(|d| !d.is_empty()) else {
            continue;
        };
        out.push(StoredDraft::new(
            change.id.clone(),
            change.title.clone(),
            change.head_sha.clone(),
            loaded.now.0,
            &draft,
            None,
        ));
    }
    Ok(out)
}

#[cfg(not(feature = "demo"))]
fn demo_drafts(_ctx: &Context) -> Result<Vec<StoredDraft>, CmdError> {
    Ok(Vec::new())
}

pub fn list(ctx: &Context) -> Result<(), CmdError> {
    let all = load(ctx)?;
    if let Some(text) = json::render(&ctx.args, FIELDS, &to_json(&all), ctx.out.tty) {
        return output::print(&text?);
    }
    if all.is_empty() {
        if ctx.out.tty {
            eprintln!("No saved drafts. Comments you keep as a draft in the diff show up here.");
        }
        return Ok(());
    }
    let mut text = ctx.out.table(&table(&all));
    if ctx.out.tty && ctx.is_demo() {
        text.push_str(&format!("\nThese are demo drafts {DEMO_LABEL}\n"));
    }
    output::print(&text)
}

/// The one draft `selector` names, or why it doesn't name exactly one.
pub fn pick<'a>(all: &'a [StoredDraft], selector: &str) -> Result<&'a StoredDraft, CmdError> {
    let parsed = selector::parse(Some(selector)).map_err(|e| CmdError::usage(e.to_string()))?;
    let matches: Vec<&StoredDraft> = all
        .iter()
        .filter(|d| match &parsed {
            Selector::Url(u) => {
                d.id.repo == u.repo && d.id.number == u.number && d.id.kind == u.kind
            }
            Selector::Qualified {
                source,
                repo,
                number,
            } => {
                d.id.repo == *repo
                    && d.id.number == *number
                    && source
                        .as_deref()
                        .is_none_or(|s| s == d.id.source_id.as_str())
            }
            Selector::Number(n) => d.id.number == *n,
            Selector::Branch(_) | Selector::Current => false,
        })
        .collect();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(CmdError::usage(format!(
            "There's no saved draft for `{selector}`.\nSee review-buddy drafts list."
        ))),
        many => {
            let names: Vec<String> = many.iter().map(|d| reference(&d.id)).collect();
            Err(CmdError::usage(format!(
                "More than one draft matches `{selector}`: {}.\nUse owner/repo#number to pick one.",
                names.join(", ")
            )))
        }
    }
}

fn comments_phrase(draft: &StoredDraft) -> String {
    match draft.comments.len() {
        0 => "a summary".to_string(),
        1 => "1 comment".to_string(),
        n => format!("{n} comments"),
    }
}

/// Removes the draft `selector` names after the confirmation. Returns what to print.
pub fn discard_in(
    dir: &Path,
    all: &[StoredDraft],
    selector: &str,
    interaction: Interaction,
    input: &mut dyn std::io::BufRead,
    prompt_out: &mut dyn Write,
) -> Result<String, CmdError> {
    let draft = pick(all, selector)?;
    let name = reference(&draft.id);
    let preview = format!(
        "This discards your saved draft on {name} ({}).\nNothing was sent to the forge, and nothing on the forge changes.",
        comments_phrase(draft)
    );
    confirm_write(&preview, interaction, input, prompt_out)?;
    drafts::remove(dir, &draft.id).map_err(|e| {
        CmdError::failed(format!(
            "Couldn't remove the draft file in {}: {e}.\nCheck the folder's permissions.",
            dir.display()
        ))
    })?;
    Ok(format!("Discarded your draft on {name}.\n"))
}

/// Removes every draft file in `dir` after the confirmation.
pub fn clear_in(
    dir: &Path,
    interaction: Interaction,
    input: &mut dyn std::io::BufRead,
    prompt_out: &mut dyn Write,
) -> Result<String, CmdError> {
    let found = drafts::load_all(dir).len();
    let files = std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .count()
    });
    if files == 0 {
        return Ok(format!(
            "There are no saved drafts in {}, so nothing was changed.\n",
            dir.display()
        ));
    }
    let preview = format!(
        "This removes all {files} saved draft files in {} ({found} readable).\nThat includes drafts for changes that are no longer in any source. Nothing was sent to a forge.",
        dir.display()
    );
    confirm_write(&preview, interaction, input, prompt_out)?;
    let removed = drafts::clear(dir).map_err(|e| {
        CmdError::failed(format!(
            "Couldn't clear {}: {e}.\nCheck the folder's permissions, or delete it yourself.",
            dir.display()
        ))
    })?;
    Ok(format!("Removed {removed} saved drafts.\n"))
}

pub fn discard(ctx: &Context, selector: &str) -> Result<(), CmdError> {
    let all = load(ctx)?;
    if ctx.is_demo() {
        let draft = pick(&all, selector)?;
        return output::print(&format!(
            "Would discard your draft on {} {DEMO_LABEL}\nNothing was changed.\n",
            reference(&draft.id)
        ));
    }
    let dir = ctx.paths.paths.drafts_dir();
    let stdin = std::io::stdin();
    let text = discard_in(
        &dir,
        &all,
        selector,
        ctx.interaction,
        &mut stdin.lock(),
        &mut std::io::stdout(),
    )?;
    output::print(&text)
}

pub fn clear(ctx: &Context) -> Result<(), CmdError> {
    let dir = ctx.paths.paths.drafts_dir();
    if ctx.is_demo() {
        return output::print(&format!(
            "Would remove the saved drafts in {} {DEMO_LABEL}\nNothing was changed.\n",
            dir.display()
        ));
    }
    let stdin = std::io::stdin();
    let text = clear_in(
        &dir,
        ctx.interaction,
        &mut stdin.lock(),
        &mut std::io::stdout(),
    )?;
    output::print(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::{DraftComment, ReviewDraft, Side, SourceId};

    fn draft(source: &str, repo: &str, number: u64) -> StoredDraft {
        StoredDraft::new(
            ChangeId {
                source_id: SourceId::new(source),
                kind: ForgeKind::GitHub,
                repo: repo.into(),
                number,
            },
            "Fix it".into(),
            "abc".into(),
            1_700_000_000,
            &ReviewDraft {
                body: String::new(),
                comments: vec![DraftComment {
                    path: "a.rs".into(),
                    side: Side::New,
                    start_line: None,
                    line: 1,
                    body: "x".into(),
                }],
            },
            None,
        )
    }

    fn yes() -> Interaction {
        Interaction {
            yes: true,
            interactive: false,
        }
    }

    #[test]
    fn selectors_match_by_reference_url_or_a_unique_number() {
        let all = vec![draft("work", "acme/a", 7), draft("home", "me/b", 9)];
        assert_eq!(pick(&all, "acme/a#7").unwrap().id.number, 7);
        assert_eq!(pick(&all, "home:me/b#9").unwrap().id.repo, "me/b");
        assert_eq!(pick(&all, "9").unwrap().id.repo, "me/b");
        assert_eq!(
            pick(&all, "https://github.com/acme/a/pull/7")
                .unwrap()
                .id
                .number,
            7
        );
        assert!(pick(&all, "acme/a#8").is_err());
        assert!(pick(&all, "work:me/b#9").is_err(), "the source must match");
        assert!(pick(&all, "feature-branch").is_err());
    }

    #[test]
    fn a_bare_number_that_is_ambiguous_asks_for_more() {
        let all = vec![draft("work", "acme/a", 7), draft("work", "acme/b", 7)];
        let err = pick(&all, "7").unwrap_err().to_string();
        assert!(
            err.contains("acme/a#7") && err.contains("acme/b#7"),
            "{err}"
        );
    }

    #[test]
    fn discarding_needs_a_yes_and_only_removes_that_draft() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (draft("work", "acme/a", 7), draft("work", "acme/a", 8));
        drafts::save(dir.path(), &a).unwrap();
        drafts::save(dir.path(), &b).unwrap();
        let all = drafts::load_all(dir.path());
        let no = Interaction {
            yes: false,
            interactive: false,
        };
        let err = discard_in(
            dir.path(),
            &all,
            "acme/a#7",
            no,
            &mut "y\n".as_bytes(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("--yes"));
        assert_eq!(drafts::load_all(dir.path()).len(), 2);

        let tty = Interaction {
            yes: false,
            interactive: true,
        };
        assert!(discard_in(
            dir.path(),
            &all,
            "acme/a#7",
            tty,
            &mut "\n".as_bytes(),
            &mut Vec::new()
        )
        .is_err());
        assert_eq!(drafts::load_all(dir.path()).len(), 2, "the default is No");
        let text = discard_in(
            dir.path(),
            &all,
            "acme/a#7",
            yes(),
            &mut "".as_bytes(),
            &mut Vec::new(),
        )
        .unwrap();
        assert!(text.contains("acme/a#7"));
        assert_eq!(drafts::load_all(dir.path()).len(), 1);
    }

    #[test]
    fn clear_removes_everything_including_unreadable_files() {
        let dir = tempfile::tempdir().unwrap();
        drafts::save(dir.path(), &draft("work", "acme/a", 7)).unwrap();
        std::fs::write(dir.path().join("0000.json"), "{ nope").unwrap();
        let text = clear_in(dir.path(), yes(), &mut "".as_bytes(), &mut Vec::new()).unwrap();
        assert_eq!(text, "Removed 2 saved drafts.\n");
        let again = clear_in(dir.path(), yes(), &mut "".as_bytes(), &mut Vec::new()).unwrap();
        assert!(again.contains("nothing was changed"));
        let missing = dir.path().join("missing");
        assert!(
            clear_in(&missing, yes(), &mut "".as_bytes(), &mut Vec::new())
                .unwrap()
                .contains("nothing was changed")
        );
    }

    #[test]
    fn json_and_table_carry_the_same_rows() {
        let all = vec![draft("work", "acme/a", 7)];
        let value = to_json(&all);
        for field in FIELDS {
            assert!(value[0].get(field).is_some(), "{field}");
        }
        assert_eq!(value[0]["ref"], "acme/a#7");
        assert_eq!(value[0]["comments"], 1);
        let text = Table::new(vec![Column::fixed("A")]);
        assert!(text.is_empty());
        assert!(!table(&all).is_empty());
    }
}
