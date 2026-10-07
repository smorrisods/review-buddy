//! The Pending reviews list: every change you have a saved draft for, with its source, repo and
//! number, title, comment count, age and whether the code changed since you wrote it.
//! `⏎` opens the diff, `x` or `delete` discards one after a confirm that defaults to No.

use crossterm::event::{KeyCode, KeyEvent};
use rb_core::{ChangeId, Timestamp};

use super::queue::age;
use super::{diff, App, Cmd, Notice, NoticeKind, Screen};

/// Two presses on one row this many ticks apart open it.
const DOUBLE_CLICK_TICKS: u64 = 2;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingList {
    pub selected: usize,
    /// A discard waiting for an answer, with the focus on the safe button unless moved.
    pub confirm: Option<Discard>,
    last_click: Option<(usize, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discard {
    pub id: ChangeId,
    pub yes: bool,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: ChangeId,
    pub source: String,
    pub title: String,
    pub comments: usize,
    pub age: String,
    /// The change's head moved since the draft was written.
    pub outdated: bool,
    /// The change isn't in any source's queue right now.
    pub missing: bool,
}

pub fn rows(app: &App) -> Vec<Row> {
    let now = app.state.now.unwrap_or(Timestamp(0));
    app.drafts
        .list()
        .into_iter()
        .map(|d| {
            let change = app.state.changes.iter().find(|c| c.id == d.id);
            Row {
                id: d.id.clone(),
                source: d.id.source_id.as_str().to_string(),
                title: change.map_or_else(|| d.title.clone(), |c| c.title.clone()),
                comments: d.comments.len().max(1),
                age: age(now, Timestamp(d.written_at)),
                outdated: change.is_some_and(|c| d.moved(&c.head_sha)),
                missing: app.state.loaded && change.is_none(),
            }
        })
        .collect()
}

pub fn open(app: &mut App) -> Vec<Cmd> {
    if app.screen != Screen::Dashboard || app.drafts.is_empty() {
        return super::composer::info(
            app,
            "No pending reviews. Comments you keep as a draft show up here.",
        );
    }
    app.pending = Some(PendingList::default());
    app.mark_dirty();
    Vec::new()
}

pub fn close(app: &mut App) {
    app.pending = None;
    app.mark_dirty();
}

fn selected_id(app: &App) -> Option<ChangeId> {
    let n = app.pending.as_ref()?.selected;
    rows(app).into_iter().nth(n).map(|r| r.id)
}

pub fn on_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    if app.pending.as_ref().is_some_and(|p| p.confirm.is_some()) {
        return on_confirm_key(app, key);
    }
    let total = app.drafts.len();
    let Some(list) = app.pending.as_mut() else {
        return Vec::new();
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('q' | 'D') => close(app),
        KeyCode::Down | KeyCode::Char('j') => {
            list.selected = (list.selected + 1).min(total.saturating_sub(1));
        }
        KeyCode::Up | KeyCode::Char('k') => list.selected = list.selected.saturating_sub(1),
        KeyCode::Char('g') | KeyCode::Home => list.selected = 0,
        KeyCode::Char('G') | KeyCode::End => list.selected = total.saturating_sub(1),
        KeyCode::Enter => return open_selected(app),
        KeyCode::Char('x') | KeyCode::Delete => ask_discard(app),
        _ => return Vec::new(),
    }
    app.mark_dirty();
    Vec::new()
}

fn on_confirm_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    let Some(confirm) = app.pending.as_mut().and_then(|p| p.confirm.as_mut()) else {
        return Vec::new();
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('n' | 'N') => return answer(app, false),
        KeyCode::Char('y' | 'Y') => return answer(app, true),
        KeyCode::Enter => {
            let yes = confirm.yes;
            return answer(app, yes);
        }
        KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Char('h' | 'l') => confirm.yes = !confirm.yes,
        _ => return Vec::new(),
    }
    app.mark_dirty();
    Vec::new()
}

fn ask_discard(app: &mut App) {
    if let (Some(id), Some(list)) = (selected_id(app), app.pending.as_mut()) {
        list.confirm = Some(Discard { id, yes: false });
    }
}

/// The answer to the discard confirm.
pub fn answer(app: &mut App, yes: bool) -> Vec<Cmd> {
    let Some(list) = app.pending.as_mut() else {
        return Vec::new();
    };
    let Some(confirm) = list.confirm.take() else {
        return Vec::new();
    };
    app.mark_dirty();
    if !yes {
        return Vec::new();
    }
    app.drafts.discard(&confirm.id);
    let left = app.drafts.len();
    if left == 0 {
        close(app);
    } else if let Some(list) = app.pending.as_mut() {
        list.selected = list.selected.min(left - 1);
    }
    super::update::set_status(
        app,
        Notice::new(
            NoticeKind::Info,
            format!("Discarded your draft on {}.", confirm.id.short_ref()),
        ),
    )
}

fn open_selected(app: &mut App) -> Vec<Cmd> {
    let Some(id) = selected_id(app) else {
        return Vec::new();
    };
    close(app);
    diff::open_change(app, &id)
}

/// A click on row `n`: selects it, and opens it on a quick second click.
pub fn click(app: &mut App, n: usize) -> Vec<Cmd> {
    let ticks = app.ticks;
    let total = app.drafts.len();
    let Some(list) = app.pending.as_mut() else {
        return Vec::new();
    };
    if n >= total {
        return Vec::new();
    }
    list.selected = n;
    let double = list
        .last_click
        .is_some_and(|(row, tick)| row == n && ticks.saturating_sub(tick) <= DOUBLE_CLICK_TICKS);
    list.last_click = (!double).then_some((n, ticks));
    app.mark_dirty();
    if double {
        return open_selected(app);
    }
    Vec::new()
}

/// How many rows the overlay shows at once for a body `height` rows tall.
pub fn visible_rows(height: u16) -> usize {
    usize::from(height.saturating_sub(6)).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{update, AppConfig, Msg};
    use crate::drafts::StoredDraft;
    use crossterm::event::{KeyCode, KeyEvent};
    use rb_core::{DraftComment, ForgeKind, ReviewDraft, Side, SourceId};
    use rb_theme::ColourDepth;

    fn id(n: u64) -> ChangeId {
        ChangeId {
            source_id: SourceId("s".into()),
            kind: ForgeKind::GitHub,
            repo: "o/r".into(),
            number: n,
        }
    }

    fn stored(n: u64, at: i64, sha: &str) -> StoredDraft {
        StoredDraft::new(
            id(n),
            format!("Change {n}"),
            sha.into(),
            at,
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

    fn app(drafts: Vec<StoredDraft>) -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        app.drafts = crate::app::drafts::Drafts::new(Some("/d".into()), drafts);
        app.state.now = Some(Timestamp(10_000));
        app
    }

    fn key(app: &mut App, code: KeyCode) -> Vec<Cmd> {
        update(app, Msg::Key(KeyEvent::from(code)))
    }

    #[test]
    fn d_with_nothing_saved_says_so_and_with_drafts_opens_the_list_newest_first() {
        let mut a = app(Vec::new());
        key(&mut a, KeyCode::Char('D'));
        assert!(a.pending.is_none());
        assert!(a
            .status
            .as_ref()
            .unwrap()
            .notice
            .text
            .starts_with("No pending reviews"));

        let mut a = app(vec![stored(1, 100, "x"), stored(2, 9_000, "x")]);
        key(&mut a, KeyCode::Char('D'));
        assert!(a.pending.is_some());
        let numbers: Vec<u64> = rows(&a).iter().map(|r| r.id.number).collect();
        assert_eq!(numbers, [2, 1]);
        assert_eq!(rows(&a)[0].age, "16m");
        assert_eq!(rows(&a)[0].title, "Change 2");
    }

    #[test]
    fn rows_flag_a_moved_head_and_a_change_that_left_the_queue() {
        let mut a = app(vec![stored(1, 100, "old")]);
        a.state.loaded = true;
        let mut c = crate::app::queue::tests::change(1, rb_core::MyRole::Reviewing, 0);
        c.id = id(1);
        c.head_sha = "new".into();
        c.title = "Fresh title".into();
        a.state.changes = vec![c];
        let row = &rows(&a)[0];
        assert!(row.outdated && !row.missing);
        assert_eq!(row.title, "Fresh title");
        a.state.changes.clear();
        let row = &rows(&a)[0];
        assert!(row.missing && !row.outdated);
        assert_eq!(row.title, "Change 1");
    }

    #[test]
    fn enter_opens_the_diff_and_esc_closes() {
        let mut a = app(vec![stored(1, 100, "x"), stored(2, 200, "x")]);
        key(&mut a, KeyCode::Char('D'));
        key(&mut a, KeyCode::Char('j'));
        let cmds = key(&mut a, KeyCode::Enter);
        assert!(a.pending.is_none());
        assert_eq!(a.screen, Screen::Diff);
        assert!(matches!(cmds.as_slice(), [Cmd::LoadDiff(got)] if got.number == 1));
        a.screen = Screen::Dashboard;
        a.diff = None;
        key(&mut a, KeyCode::Char('D'));
        key(&mut a, KeyCode::Esc);
        assert!(a.pending.is_none());
    }

    #[test]
    fn x_discards_only_after_a_confirm_that_defaults_to_no() {
        let mut a = app(vec![stored(1, 100, "x"), stored(2, 200, "x")]);
        key(&mut a, KeyCode::Char('D'));
        key(&mut a, KeyCode::Char('x'));
        assert!(!a.pending.as_ref().unwrap().confirm.as_ref().unwrap().yes);
        key(&mut a, KeyCode::Enter);
        assert_eq!(a.drafts.len(), 2, "Enter on No keeps it");
        key(&mut a, KeyCode::Delete);
        key(&mut a, KeyCode::Esc);
        assert_eq!(a.drafts.len(), 2);
        assert!(a.pending.is_some(), "esc answers the confirm, not the list");
        key(&mut a, KeyCode::Char('x'));
        key(&mut a, KeyCode::Char('y'));
        assert_eq!(a.drafts.len(), 1);
        assert!(
            a.drafts.get(&id(2)).is_none(),
            "the selected, newest one went"
        );
        assert!(a.status.as_ref().unwrap().notice.text.contains("o/r#2"));
        key(&mut a, KeyCode::Char('x'));
        key(&mut a, KeyCode::Char('y'));
        assert!(a.pending.is_none(), "an empty list closes");
        assert!(a.drafts.is_empty());
    }

    #[test]
    fn a_second_click_on_a_row_opens_it() {
        let mut a = app(vec![stored(1, 100, "x"), stored(2, 200, "x")]);
        key(&mut a, KeyCode::Char('D'));
        assert!(click(&mut a, 1).is_empty());
        assert_eq!(a.pending.as_ref().unwrap().selected, 1);
        let cmds = click(&mut a, 1);
        assert!(matches!(cmds.as_slice(), [Cmd::LoadDiff(got)] if got.number == 1));
        assert!(click(&mut a, 9).is_empty());
    }

    #[test]
    fn a_quit_from_the_list_still_saves() {
        let mut a = app(vec![stored(1, 100, "x")]);
        key(&mut a, KeyCode::Char('D'));
        let cmds = update(
            &mut a,
            Msg::Key(KeyEvent::new(
                KeyCode::Char('c'),
                crossterm::event::KeyModifiers::CONTROL,
            )),
        );
        assert!(a.should_quit());
        let _ = cmds;
    }
}
