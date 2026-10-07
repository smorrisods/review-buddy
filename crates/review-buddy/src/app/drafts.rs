//! Review drafts as the app holds them: one per change, kept when you leave a diff, saved to disk
//! (debounced like `session.toml`) unless they are memory only, and restored when the change is
//! opened again. Pure: writes leave as [`Cmd::SaveDrafts`].
//!
//! A draft here is your own unsent text. It is not the pending review a forge may hold for you;
//! that one arrives as pending threads and is edited through the provider.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use rb_core::{ChangeId, DraftComment, ReviewDraft, Timestamp};
use rb_diff::LineId;

use super::composer::{Confirm, ConfirmKind, Target};
use super::diff::{self, DiffState, Phase};
use super::update::set_status;
use super::{App, Cmd, Msg, Notice, NoticeKind, Screen};
use crate::drafts::{Position, StoredDraft};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Default)]
pub struct Drafts {
    /// Where drafts are written; `None` keeps them in memory only (demo mode, `ui.drafts = "off"`).
    dir: Option<PathBuf>,
    kept: HashMap<ChangeId, StoredDraft>,
    /// Changes whose draft was discarded, so a stand-in draft from a fixture doesn't come back.
    discarded: HashSet<ChangeId>,
    /// Changes whose file needs writing, or removing when they have no draft any more.
    dirty: HashSet<ChangeId>,
    scheduled: bool,
}

impl Drafts {
    /// `loaded` is what the folder held at launch, so launching alone writes nothing.
    pub fn new(dir: Option<PathBuf>, loaded: Vec<StoredDraft>) -> Self {
        Self {
            dir,
            kept: loaded.into_iter().map(|d| (d.id.clone(), d)).collect(),
            ..Self::default()
        }
    }

    /// Whether drafts survive a restart.
    pub fn persists(&self) -> bool {
        self.dir.is_some()
    }

    pub fn dir(&self) -> Option<&std::path::Path> {
        self.dir.as_deref()
    }

    pub fn get(&self, id: &ChangeId) -> Option<&StoredDraft> {
        self.kept.get(id)
    }

    /// How many comments are kept for `id`, plus one when only a summary is.
    pub fn count(&self, id: &ChangeId) -> usize {
        self.kept
            .get(id)
            .map_or(0, |d| d.comments.len().max(usize::from(!d.is_empty())))
    }

    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }

    pub fn len(&self) -> usize {
        self.kept.len()
    }

    /// Every kept draft, newest first.
    pub fn list(&self) -> Vec<&StoredDraft> {
        let mut all: Vec<&StoredDraft> = self.kept.values().collect();
        all.sort_by(|a, b| {
            b.written_at
                .cmp(&a.written_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        all
    }

    /// Whether `id` was thrown away this session.
    pub fn was_discarded(&self, id: &ChangeId) -> bool {
        self.discarded.contains(id)
    }

    /// Keeps `draft`. A change in what you wrote queues a write; a moved cursor alone doesn't.
    pub fn put(&mut self, draft: StoredDraft) {
        let id = draft.id.clone();
        self.discarded.remove(&id);
        match self.kept.get_mut(&id) {
            Some(old) if old.same_content(&draft) => old.position = draft.position,
            _ => {
                self.kept.insert(id.clone(), draft);
                self.queue(id);
            }
        }
    }

    /// Forgets the draft for `id`, on disk too.
    pub fn discard(&mut self, id: &ChangeId) {
        self.discarded.insert(id.clone());
        if self.kept.remove(id).is_some() {
            self.queue(id.clone());
        }
    }

    /// Writes the cursor position of `id`'s draft the next time anything is written.
    fn touch(&mut self, id: &ChangeId) {
        if self.kept.contains_key(id) {
            self.queue(id.clone());
        }
    }

    fn queue(&mut self, id: ChangeId) {
        if self.dir.is_some() {
            self.dirty.insert(id);
        }
    }

    /// Asks for [`Msg::SaveDraftsDue`] once, after the debounce delay.
    fn schedule(&mut self) -> Option<Cmd> {
        if self.dirty.is_empty() || self.scheduled {
            return None;
        }
        self.scheduled = true;
        Some(Cmd::After {
            delay: SAVE_DEBOUNCE,
            msg: Msg::SaveDraftsDue,
        })
    }

    /// What to write now. `None` when nothing changed since the last write.
    pub fn flush(&mut self) -> Option<Cmd> {
        self.scheduled = false;
        let dir = self.dir.clone()?;
        if self.dirty.is_empty() {
            return None;
        }
        let mut ids: Vec<ChangeId> = self.dirty.drain().collect();
        ids.sort();
        let writes = ids
            .into_iter()
            .map(|id| {
                let draft = self.kept.get(&id).cloned();
                (id, draft)
            })
            .collect();
        Some(Cmd::SaveDrafts { dir, writes })
    }
}

/// The draft as the open diff holds it, or `None` when no diff with patches is open. Your
/// composer's text joins as a comment when `with_composer` is set (the app is quitting).
pub(super) fn snapshot(app: &App, with_composer: bool) -> Option<StoredDraft> {
    let s = app.diff.as_ref().filter(|s| s.phase == Phase::Ready)?;
    let data = s.data.as_ref()?;
    let mut draft = data.draft.clone();
    if let Some(modal) = &s.review {
        draft.body = modal.summary.text().trim().to_string();
    }
    if with_composer {
        if let Some(c) = s.composer.as_ref().filter(|c| c.is_dirty()) {
            if let Target::Line(anchor) = &c.target {
                draft.comments.push(DraftComment {
                    path: anchor.path.clone(),
                    side: anchor.side,
                    start_line: anchor.start_line,
                    line: anchor.line,
                    body: c.editor.text().trim().to_string(),
                });
            }
        }
    }
    let change = app.state.changes.iter().find(|c| c.id == s.id);
    let previous = app.drafts.get(&s.id);
    let known = change
        .map(|c| c.head_sha.clone())
        .filter(|sha| !sha.is_empty());
    let head_sha = match (s.stale, previous, known) {
        (true, Some(old), _) | (false, Some(old), None) => old.head_sha.clone(),
        (_, _, known) => known.unwrap_or_default(),
    };
    let title = change.map_or_else(
        || previous.map(|p| p.title.clone()).unwrap_or_default(),
        |c| c.title.clone(),
    );
    let verdict = s.review.as_ref().map(|m| m.verdict).or(s.verdict);
    let at = app.state.now.unwrap_or(Timestamp(0)).0;
    let mut stored = StoredDraft::new(s.id.clone(), title, head_sha, at, &draft, verdict);
    stored.position = position(s);
    Some(stored)
}

fn position(s: &DiffState) -> Option<Position> {
    let file = s.current()?;
    let id = s.view.rows.line_id(s.view.cursor)?;
    Some(Position {
        path: file.diff.path.clone(),
        hunk: id.hunk,
        line: id.line,
    })
}

/// Keeps the store in step with the open diff. Runs after every message, like the layout tracker.
pub(super) fn sync(app: &mut App) -> Vec<Cmd> {
    let quitting = app.should_quit();
    if let Some(stored) = snapshot(app, quitting) {
        if stored.is_empty() {
            let id = stored.id.clone();
            if app.drafts.get(&id).is_some() {
                app.drafts.discard(&id);
            }
        } else {
            app.drafts.put(stored);
        }
    }
    if quitting {
        return app.drafts.flush().into_iter().collect();
    }
    app.drafts.schedule().into_iter().collect()
}

pub(super) fn on_save_due(app: &mut App) -> Vec<Cmd> {
    app.drafts.flush().into_iter().collect()
}

/// Puts a saved draft back into a diff that just loaded, with the cursor where it was. Returns
/// the status to show.
pub(super) fn restore(app: &mut App) -> Vec<Cmd> {
    let Some(id) = app.diff.as_ref().map(|s| s.id.clone()) else {
        return Vec::new();
    };
    let sha = app
        .state
        .changes
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.head_sha.clone())
        .unwrap_or_default();
    let Some(stored) = app.drafts.get(&id).cloned() else {
        if app.drafts.was_discarded(&id) {
            if let Some(data) = app.diff.as_mut().and_then(|s| s.data.as_mut()) {
                data.draft = ReviewDraft::default();
            }
        }
        set_baseline(app);
        return Vec::new();
    };
    let stale = stored.moved(&sha);
    if let Some(s) = app.diff.as_mut() {
        if let Some(data) = s.data.as_mut() {
            data.draft = stored.draft();
        }
        s.verdict = stored.verdict;
        s.stale = stale;
    }
    diff::rebuild(app, None);
    if let Some(at) = &stored.position {
        diff::goto(
            app,
            &at.path,
            LineId {
                hunk: at.hunk,
                line: at.line,
            },
        );
    }
    set_baseline(app);
    let what = match stored.comments.len() {
        0 => "summary".to_string(),
        1 => "1 comment".to_string(),
        n => format!("{n} comments"),
    };
    let mut text = format!("Draft restored · {what}");
    if stale {
        text.push_str(" · the code changed since you wrote this");
    }
    set_status(app, Notice::new(NoticeKind::Info, text))
}

/// Remembers the draft as it stands, so leaving only asks about what changed since.
fn set_baseline(app: &mut App) {
    if let Some(s) = app.diff.as_mut() {
        if let Some(data) = s.data.as_ref() {
            s.baseline = Some((data.draft.clone(), s.verdict));
        }
    }
}

/// What leaving would put at risk: `(comments, has a summary)`, when the open diff holds
/// something that changed since it opened.
fn unsaved(app: &App) -> Option<(usize, bool)> {
    let now = snapshot(app, false)?;
    if now.is_empty() {
        return None;
    }
    let s = app.diff.as_ref()?;
    let (draft, verdict) = s.baseline.as_ref()?;
    let same = *draft == now.draft() && *verdict == now.verdict;
    (!same).then_some((now.comments.len(), !now.summary.trim().is_empty()))
}

/// `esc`/`q` on the diff: asks first when there's something unsaved, otherwise just leaves.
pub(super) fn leave(app: &mut App) -> Vec<Cmd> {
    if app.diff.as_ref().is_some_and(|s| s.submitting) {
        return super::composer::info(app, "Still sending. One moment.");
    }
    match unsaved(app) {
        Some((count, summary_only)) => {
            let summary_only = count == 0 && summary_only;
            if let Some(s) = app.diff.as_mut() {
                s.confirm = Some(Confirm::new(ConfirmKind::Leave {
                    count,
                    summary_only,
                }));
            }
            app.mark_dirty();
            Vec::new()
        }
        None => {
            keep_and_close(app);
            Vec::new()
        }
    }
}

/// Saves what the diff holds (with the cursor) and goes back to the queue.
pub(super) fn keep_and_close(app: &mut App) {
    sync_now(app);
    close(app);
}

/// Throws the draft away and goes back to the queue.
pub(super) fn discard_and_close(app: &mut App) {
    if let Some(id) = app.diff.as_ref().map(|s| s.id.clone()) {
        app.drafts.discard(&id);
    }
    close(app);
}

fn sync_now(app: &mut App) {
    if let Some(stored) = snapshot(app, false) {
        let id = stored.id.clone();
        if stored.is_empty() {
            if app.drafts.get(&id).is_some() {
                app.drafts.discard(&id);
            }
        } else {
            app.drafts.put(stored);
            app.drafts.touch(&id);
        }
    }
}

fn close(app: &mut App) {
    app.diff = None;
    app.screen = Screen::Dashboard;
    app.mark_dirty();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::diffview::tests::{draft, PATCH};
    use crate::app::{update, AppConfig, DiffData};
    use crossterm::event::{KeyCode, KeyEvent};
    use rb_core::Verdict;
    use rb_core::{FilePatch, FileStatus, ForgeKind, SourceId};
    use rb_theme::ColourDepth;

    fn id() -> ChangeId {
        ChangeId {
            source_id: SourceId("s".into()),
            kind: ForgeKind::GitHub,
            repo: "o/r".into(),
            number: 1,
        }
    }

    fn patch() -> FilePatch {
        FilePatch {
            path: "src/a.rs".into(),
            old_path: None,
            status: FileStatus::Modified,
            adds: 2,
            dels: 1,
            patch: Some(PATCH.to_string()),
        }
    }

    pub(crate) fn app(persist: Option<PathBuf>, loaded: Vec<StoredDraft>) -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        app.drafts = Drafts::new(persist, loaded);
        app
    }

    fn open(app: &mut App, comments: Vec<DraftComment>) {
        diff::open_change(app, &id());
        let data = DiffData::new(
            vec![patch()],
            Vec::new(),
            ReviewDraft {
                body: String::new(),
                comments,
            },
        );
        update(
            app,
            Msg::DiffLoaded {
                id: id(),
                result: Ok(Box::new(data)),
            },
        );
    }

    fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
        update(app, Msg::Key(KeyEvent::from(code)))
    }

    fn stored(sha: &str, comments: Vec<DraftComment>) -> StoredDraft {
        StoredDraft::new(
            id(),
            "T".into(),
            sha.into(),
            10,
            &ReviewDraft {
                body: "Summary".into(),
                comments,
            },
            Some(Verdict::Approve),
        )
    }

    fn change_at(id: &ChangeId, sha: &str) -> rb_core::ChangeSummary {
        let mut c = crate::app::queue::tests::change(1, rb_core::MyRole::Reviewing, 0);
        c.id = id.clone();
        c.head_sha = sha.into();
        c
    }

    fn dir() -> PathBuf {
        PathBuf::from("/state/drafts")
    }

    fn comment(app: &mut App, text: &str) {
        press(app, KeyCode::Char('c'));
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
        press(app, KeyCode::Enter);
    }

    #[test]
    fn leaving_with_new_comments_asks_and_keep_is_the_default() {
        let mut app = app(None, Vec::new());
        open(&mut app, Vec::new());
        comment(&mut app, "Plain.");
        press(&mut app, KeyCode::Esc);
        let confirm = app.diff.as_ref().unwrap().confirm.clone().unwrap();
        assert_eq!(confirm.focus, 0);
        assert!(matches!(
            confirm.kind,
            ConfirmKind::Leave {
                count: 1,
                summary_only: false
            }
        ));
        assert_eq!(confirm.title(), "Keep this comment as a draft?");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::Dashboard);
        assert!(app.diff.is_none());
        assert_eq!(app.drafts.count(&id()), 1);
    }

    #[test]
    fn discard_drops_the_draft_and_cancel_stays() {
        let mut app = app(Some(dir()), Vec::new());
        open(&mut app, Vec::new());
        comment(&mut app, "a");
        comment(&mut app, "b");
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(
            app.diff.as_ref().unwrap().confirm.as_ref().unwrap().title(),
            "Keep these 2 comments as a draft?"
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Diff);
        assert!(app.diff.as_ref().unwrap().confirm.is_none());
        assert_eq!(app.drafts.count(&id()), 2, "autosave still has them");
        press(&mut app, KeyCode::Char('q'));
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.screen, Screen::Dashboard);
        assert_eq!(app.drafts.count(&id()), 0);
        assert!(app.drafts.was_discarded(&id()));
        match app.drafts.flush() {
            Some(Cmd::SaveDrafts { writes, .. }) => assert!(writes[0].1.is_none()),
            other => panic!("expected the file to go, got {other:?}"),
        }
    }

    #[test]
    fn leaving_a_draft_that_is_unchanged_since_it_opened_just_leaves() {
        let mut app = app(None, Vec::new());
        open(&mut app, vec![draft(4, "a")]);
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.screen, Screen::Dashboard);
        assert_eq!(app.drafts.count(&id()), 1);
    }

    #[test]
    fn leaving_with_nothing_unsaved_just_leaves() {
        let mut app = app(None, Vec::new());
        open(&mut app, Vec::new());
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Dashboard);
        assert!(app.drafts.is_empty());
    }

    #[test]
    fn reopening_restores_comments_summary_verdict_and_position() {
        let mut app = app(None, Vec::new());
        open(&mut app, Vec::new());
        comment(&mut app, "Plain.");
        press(&mut app, KeyCode::Char('j'));
        let at = app.diff.as_ref().and_then(diff::cursor_id);
        press(&mut app, KeyCode::Char('R'));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::Dashboard);
        let kept = app.drafts.get(&id()).unwrap();
        assert_eq!(kept.verdict, Some(Verdict::Comment));
        assert!(kept.position.is_some());

        open(&mut app, Vec::new());
        let s = app.diff.as_ref().unwrap();
        assert_eq!(s.data.as_ref().unwrap().draft.comments.len(), 1);
        assert_eq!(s.verdict, Some(Verdict::Comment));
        assert_eq!(diff::cursor_id(s), at);
        assert_eq!(
            app.status.as_ref().unwrap().notice.text,
            "Draft restored · 1 comment"
        );
    }

    #[test]
    fn a_draft_from_disk_restores_and_flags_a_moved_head() {
        let loaded = stored("old", vec![draft(4, "x")]);
        let mut app = app(Some(dir()), vec![loaded]);
        app.state.changes = vec![change_at(&id(), "new")];
        open(&mut app, Vec::new());
        let s = app.diff.as_ref().unwrap();
        assert!(s.stale);
        assert_eq!(s.data.as_ref().unwrap().draft.body, "Summary");
        let text = &app.status.as_ref().unwrap().notice.text;
        assert!(
            text.contains("the code changed since you wrote this"),
            "{text}"
        );
    }

    #[test]
    fn restoring_untouched_writes_nothing() {
        let loaded = stored("abc", vec![draft(4, "x")]);
        let mut app = app(Some(dir()), vec![loaded]);
        open(&mut app, Vec::new());
        assert!(app.drafts.flush().is_none(), "nothing changed");
    }

    #[test]
    fn changes_are_debounced_and_written_in_one_batch() {
        let mut app = app(Some(dir()), Vec::new());
        diff::open_change(&mut app, &id());
        let data = DiffData::new(vec![patch()], Vec::new(), ReviewDraft::default());
        let cmds = update(
            &mut app,
            Msg::DiffLoaded {
                id: id(),
                result: Ok(Box::new(data)),
            },
        );
        assert!(cmds.is_empty());
        press(&mut app, KeyCode::Char('c'));
        for c in "Why?".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        let cmds = press(&mut app, KeyCode::Enter);
        let after: Vec<_> = cmds
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    Cmd::After {
                        msg: Msg::SaveDraftsDue,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(after.len(), 1, "one debounce timer");
        let more = press(&mut app, KeyCode::Char('c'));
        assert!(more.iter().all(|c| !matches!(c, Cmd::After { .. })));
        let cmds = update(&mut app, Msg::SaveDraftsDue);
        match cmds.as_slice() {
            [Cmd::SaveDrafts { dir: d, writes }] => {
                assert_eq!(d, &dir());
                assert_eq!(writes.len(), 1);
                assert_eq!(writes[0].1.as_ref().unwrap().comments[0].body, "Why?");
            }
            other => panic!("expected one write, got {other:?}"),
        }
        assert!(update(&mut app, Msg::SaveDraftsDue).is_empty());
    }

    #[test]
    fn memory_only_drafts_never_ask_for_a_write() {
        let mut app = app(None, Vec::new());
        open(&mut app, vec![draft(4, "x")]);
        assert!(!app.drafts.persists());
        assert!(app.drafts.flush().is_none());
        let cmds = update(&mut app, Msg::SaveDraftsDue);
        assert!(cmds.is_empty());
    }

    #[test]
    fn quitting_flushes_immediately_and_keeps_composer_text() {
        let mut app = app(Some(dir()), Vec::new());
        open(&mut app, Vec::new());
        press(&mut app, KeyCode::Char('c'));
        for c in "half a thought".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        let cmds = update(
            &mut app,
            Msg::Key(KeyEvent::new(
                KeyCode::Char('c'),
                crossterm::event::KeyModifiers::CONTROL,
            )),
        );
        assert!(
            app.should_quit(),
            "drafts are saved, so nothing blocks quitting"
        );
        match cmds.as_slice() {
            [.., Cmd::SaveDrafts { writes, .. }] => {
                assert_eq!(
                    writes[0].1.as_ref().unwrap().comments[0].body,
                    "half a thought"
                );
            }
            other => panic!("expected a final write, got {other:?}"),
        }
    }

    #[test]
    fn memory_only_quit_still_warns() {
        let mut app = app(None, Vec::new());
        open(&mut app, vec![draft(4, "x")]);
        let ctrl_c = || {
            Msg::Key(KeyEvent::new(
                KeyCode::Char('c'),
                crossterm::event::KeyModifiers::CONTROL,
            ))
        };
        update(&mut app, ctrl_c());
        assert!(!app.should_quit());
        assert!(app
            .status
            .as_ref()
            .unwrap()
            .notice
            .text
            .contains("aren't saved"));
        update(&mut app, ctrl_c());
        assert!(app.should_quit());
    }

    #[test]
    fn emptying_the_draft_removes_it() {
        let loaded = stored("abc", vec![draft(4, "x")]);
        let mut app = app(Some(dir()), vec![loaded]);
        open(&mut app, Vec::new());
        app.diff.as_mut().unwrap().data.as_mut().unwrap().draft = ReviewDraft::default();
        update(&mut app, Msg::Tick);
        assert_eq!(app.drafts.count(&id()), 0);
        match app.drafts.flush() {
            Some(Cmd::SaveDrafts { writes, .. }) => assert!(writes[0].1.is_none()),
            other => panic!("expected a removal, got {other:?}"),
        }
    }

    #[test]
    fn a_submitted_review_clears_the_draft() {
        let mut app = app(Some(dir()), Vec::new());
        open(&mut app, vec![draft(4, "x")]);
        press(&mut app, KeyCode::Char('R'));
        let cmds = press(&mut app, KeyCode::Enter);
        assert!(cmds.iter().any(|c| matches!(c, Cmd::SubmitReview { .. })));
        update(
            &mut app,
            Msg::ReviewSubmitted {
                id: id(),
                verdict: Verdict::Comment,
                result: Ok(()),
                demo: false,
            },
        );
        assert_eq!(app.drafts.count(&id()), 0);
        match app.drafts.flush() {
            Some(Cmd::SaveDrafts { writes, .. }) => assert!(writes[0].1.is_none()),
            other => panic!("expected a removal, got {other:?}"),
        }
    }

    #[test]
    fn a_discarded_fixture_draft_does_not_come_back() {
        let mut app = app(None, Vec::new());
        app.drafts.discard(&id());
        open(&mut app, vec![draft(4, "from the fixture")]);
        assert!(app
            .diff
            .as_ref()
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .draft
            .comments
            .is_empty());
    }
}
