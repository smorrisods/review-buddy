//! Editing and deleting pending comments: your local draft comments, comments restored from
//! disk (which are the same thing), and pending comments the forge holds (started on the web).
//! Local ones change in memory; forge-side ones go through the provider behind a confirm.
//! Also the pending-comment list in the Files pane and the comments whose line has gone.

use rb_core::{CommentId, ForgeKind, Side, ThreadId};
use rb_diff::Anchor as LineAnchor;

use super::composer::{self, forge_name, info, Anchor, Composer, Confirm, ConfirmKind, Target};
use super::diff::{self, DiffFocus, DiffState};
use super::diffview::Origin;
use super::update::push_toast;
use super::{App, Cmd, Notice, NoticeKind};

/// Which pending comment an edit or delete is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentRef {
    /// The nth comment of the local draft.
    Draft(usize),
    /// A pending comment on the forge.
    Remote {
        thread: ThreadId,
        comment: CommentId,
    },
}

/// One pending comment, as the Files pane lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub which: CommentRef,
    pub origin: Origin,
    pub path: String,
    pub side: Side,
    pub start_line: Option<u32>,
    pub line: u32,
    pub body: String,
    /// The line isn't in the diff any more.
    pub outdated: bool,
}

impl Entry {
    pub fn anchor(&self) -> Anchor {
        Anchor {
            path: self.path.clone(),
            side: self.side,
            start_line: self.start_line,
            line: self.line,
        }
    }

    /// `a.rs:12 first words`, for the list.
    pub fn label(&self) -> String {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        format!("{name}:{} {}", self.line, first_words(&self.body, 40))
    }

    /// What a confirm names: `a.rs lines 3–5 · “first words”`.
    pub fn what(&self) -> String {
        format!(
            "{} · “{}”",
            self.anchor().place(),
            first_words(&self.body, 48)
        )
    }
}

/// The first line of prose in `body`, cut to `max` characters. A body that is only a suggestion
/// reads as "suggestion".
pub fn first_words(body: &str, max: usize) -> String {
    let mut fenced = false;
    let mut first = "";
    for line in body.lines().map(str::trim) {
        if line.starts_with("```") {
            fenced = !fenced;
        } else if !fenced && !line.is_empty() {
            first = line;
            break;
        }
    }
    if first.is_empty() && body.contains("```suggestion") {
        first = "suggestion";
    }
    if first.chars().count() <= max {
        return first.to_string();
    }
    let cut: String = first.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

fn is_outdated(state: &DiffState, path: &str, side: Side, line: u32) -> bool {
    let Some(file) = state.files().iter().find(|f| f.diff.path == path) else {
        return true;
    };
    file.diff
        .parsed()
        .is_some_and(|p| p.find_by_anchor(LineAnchor { side, line }).is_none())
}

/// Every pending comment on the change: the local draft in order, then the forge's. The forge's
/// are left out where the provider can't edit them.
pub fn entries(state: &DiffState) -> Vec<Entry> {
    let Some(data) = state.data.as_ref() else {
        return Vec::new();
    };
    let mut out: Vec<Entry> = data
        .draft
        .comments
        .iter()
        .enumerate()
        .map(|(i, c)| Entry {
            which: CommentRef::Draft(i),
            origin: Origin::Draft(i),
            path: c.path.clone(),
            side: c.side,
            start_line: c.start_line,
            line: c.line,
            body: c.body.clone(),
            outdated: is_outdated(state, &c.path, c.side, c.line),
        })
        .collect();
    if data.edit_pending {
        for (i, t) in data.threads.iter().enumerate() {
            let (Some(path), Some(line)) = (t.path.as_ref(), t.line) else {
                continue;
            };
            let Some(comment) = t.comments.iter().rev().find(|c| c.pending) else {
                continue;
            };
            out.push(Entry {
                which: CommentRef::Remote {
                    thread: t.id.clone(),
                    comment: comment.id.clone(),
                },
                origin: Origin::Thread(i),
                path: path.clone(),
                side: t.side,
                start_line: t.start_line,
                line,
                body: comment.body.clone(),
                outdated: t.outdated || is_outdated(state, path, t.side, line),
            });
        }
    }
    out
}

/// How many rows taller the review block gets: one per pending comment listed (to four), a
/// line for the kept verdict and one when the code changed since the draft was written.
pub fn review_extra(state: &DiffState) -> u16 {
    let listed = entries(state).len().min(LIST_ROWS) as u16;
    listed + u16::from(state.verdict.is_some()) + u16::from(state.stale)
}

/// Pending comments the Files pane lists at once.
pub const LIST_ROWS: usize = 4;

/// The editable blocks hanging from the cursor line.
pub fn attached(state: &DiffState) -> Vec<(u32, Origin)> {
    let editable_threads = state.data.as_ref().is_some_and(|d| d.edit_pending);
    state
        .view
        .rows
        .attached(state.view.cursor)
        .into_iter()
        .filter(|(_, o)| editable_threads || !matches!(o, Origin::Thread(_)))
        .collect()
}

/// `(how many, which one is picked)` on the cursor line.
pub fn picked(state: &DiffState) -> (usize, usize) {
    let total = attached(state).len();
    let at = match state.pick {
        Some((row, i)) if row == state.view.cursor => i,
        _ => 0,
    };
    (total, at.min(total.saturating_sub(1)))
}

/// The block drawn as picked: only worth marking when a line has several.
pub fn picked_block(state: &DiffState) -> Option<u32> {
    let (total, at) = picked(state);
    (total > 1).then(|| attached(state)[at].0)
}

fn entry_for(state: &DiffState, origin: Origin) -> Option<Entry> {
    entries(state).into_iter().find(|e| e.origin == origin)
}

fn cursor_entry(app: &App) -> Option<Entry> {
    let state = app.diff.as_ref()?;
    let (total, at) = picked(state);
    if total == 0 {
        return None;
    }
    entry_for(state, attached(state)[at].1)
}

const NONE_HERE: &str = "There's no pending comment on this line. Press c to add one.";

/// `,` and `.`: steps between the pending comments on the cursor line.
pub fn step_pick(app: &mut App, forward: bool) -> Vec<Cmd> {
    let Some(state) = app.diff.as_mut() else {
        return Vec::new();
    };
    let (total, at) = picked(state);
    if total == 0 {
        return info(app, NONE_HERE);
    }
    let next = if forward {
        (at + 1) % total
    } else {
        (at + total - 1) % total
    };
    state.pick = Some((state.view.cursor, next));
    app.mark_dirty();
    if total == 1 {
        return info(app, "One pending comment on this line.");
    }
    info(app, format!("Comment {} of {total} on this line", next + 1))
}

/// `e` or `⏎` on a line: reopens the picked pending comment in the composer.
pub fn edit_at_cursor(app: &mut App) -> Vec<Cmd> {
    if !composer::ready(app) {
        return Vec::new();
    }
    match cursor_entry(app) {
        Some(entry) => open_edit(app, &entry),
        None => info(app, NONE_HERE),
    }
}

/// `d` or `delete` on a line: asks before removing the picked pending comment.
pub fn delete_at_cursor(app: &mut App) -> Vec<Cmd> {
    if !composer::ready(app) {
        return Vec::new();
    }
    match cursor_entry(app) {
        Some(entry) => ask_delete(app, &entry),
        None => info(app, NONE_HERE),
    }
}

fn open_edit(app: &mut App, entry: &Entry) -> Vec<Cmd> {
    if matches!(entry.which, CommentRef::Draft(_)) && entry.outdated {
        return ask_outdated(app, entry);
    }
    let target = Target::Edit {
        which: entry.which.clone(),
        place: entry.anchor().place(),
    };
    composer::open(app, Composer::new(target, &entry.body))
}

fn ask_delete(app: &mut App, entry: &Entry) -> Vec<Cmd> {
    let remote = matches!(entry.which, CommentRef::Remote { .. });
    let kind = ConfirmKind::DeleteComment {
        which: entry.which.clone(),
        what: entry.what(),
        remote,
    };
    composer::set_confirm(app, Confirm::new(kind))
}

fn ask_outdated(app: &mut App, entry: &Entry) -> Vec<Cmd> {
    let CommentRef::Draft(index) = entry.which else {
        return info(
            app,
            "That line isn't in the diff any more. Open the change on the forge to finish it there.",
        );
    };
    let kind = ConfirmKind::Outdated {
        index,
        what: entry.what(),
    };
    composer::set_confirm(app, Confirm::new(kind))
}

/// Enter in the Files pane's review list: shows the comment in the diff.
pub fn jump_entry(app: &mut App, n: usize) -> Vec<Cmd> {
    let Some(entry) = app
        .diff
        .as_ref()
        .and_then(|s| entries(s).into_iter().nth(n))
    else {
        return Vec::new();
    };
    if let Some(s) = app.diff.as_mut() {
        s.review_sel = n;
    }
    if entry.outdated {
        return ask_outdated(app, &entry);
    }
    diff::goto_anchor(app, &entry.path, entry.side, entry.line);
    if let Some(s) = app.diff.as_mut() {
        s.focus = DiffFocus::Diff;
        let at = attached(s).iter().position(|(_, o)| *o == entry.origin);
        s.pick = at.map(|i| (s.view.cursor, i));
    }
    app.mark_dirty();
    Vec::new()
}

/// `e` in the Files pane's review list.
pub fn edit_entry(app: &mut App) -> Vec<Cmd> {
    let Some(entry) = selected_entry(app) else {
        return Vec::new();
    };
    if !entry.outdated {
        diff::goto_anchor(app, &entry.path, entry.side, entry.line);
    }
    open_edit(app, &entry)
}

/// `d` in the Files pane's review list.
pub fn delete_entry(app: &mut App) -> Vec<Cmd> {
    match selected_entry(app) {
        Some(entry) => ask_delete(app, &entry),
        None => Vec::new(),
    }
}

fn selected_entry(app: &App) -> Option<Entry> {
    let s = app.diff.as_ref()?;
    entries(s).into_iter().nth(s.review_sel)
}

/// Moves the selection in the review list.
pub fn move_selection(app: &mut App, delta: isize) {
    let Some(s) = app.diff.as_mut() else {
        return;
    };
    let total = entries(s).len();
    if total == 0 {
        return;
    }
    s.review_sel = s.review_sel.saturating_add_signed(delta).min(total - 1);
    app.mark_dirty();
}

/// `⏎` in the composer while editing.
pub fn save_edit(app: &mut App) -> Vec<Cmd> {
    let Some(c) = app.diff.as_ref().and_then(|s| s.composer.as_ref()) else {
        return Vec::new();
    };
    let Target::Edit { which, place } = c.target.clone() else {
        return Vec::new();
    };
    let body = c.editor.text().trim().to_string();
    if body.is_empty() {
        return info(app, "Write something first, or press esc to close.");
    }
    match which {
        CommentRef::Draft(index) => {
            let keep = diff::cursor_id_of(app);
            if let Some(s) = app.diff.as_mut() {
                s.composer = None;
                if let Some(c) = s
                    .data
                    .as_mut()
                    .and_then(|d| d.draft.comments.get_mut(index))
                {
                    c.body = body;
                }
            }
            diff::rebuild(app, keep);
            app.mark_dirty();
            info(app, "Comment updated. It's still pending.")
        }
        CommentRef::Remote { .. } => {
            let forge = app
                .diff
                .as_ref()
                .map_or("the forge", |s| forge_name(s.id.kind));
            let what = format!("{place} on {forge}");
            composer::set_confirm(app, Confirm::new(ConfirmKind::SaveRemote { what }))
        }
    }
}

/// The edit was confirmed: sends it to the forge.
pub fn send_edit(app: &mut App) -> Vec<Cmd> {
    let Some(s) = app.diff.as_mut() else {
        return Vec::new();
    };
    let Some(c) = s.composer.as_mut() else {
        return Vec::new();
    };
    let Target::Edit {
        which: CommentRef::Remote { thread, comment },
        ..
    } = c.target.clone()
    else {
        return Vec::new();
    };
    let body = c.editor.text().trim().to_string();
    c.sending = true;
    s.submitting = true;
    let id = s.id.clone();
    app.mark_dirty();
    vec![Cmd::EditComment {
        id,
        thread,
        comment,
        body,
    }]
}

/// The delete was confirmed.
pub fn delete_confirmed(app: &mut App, which: &CommentRef) -> Vec<Cmd> {
    match which {
        CommentRef::Draft(index) => {
            let keep = diff::cursor_id_of(app);
            let mut gone = None;
            if let Some(data) = app.diff.as_mut().and_then(|s| s.data.as_mut()) {
                if *index < data.draft.comments.len() {
                    gone = Some(data.draft.comments.remove(*index));
                }
            }
            diff::rebuild(app, keep);
            app.mark_dirty();
            match gone {
                Some(c) => info(
                    app,
                    format!(
                        "Deleted your comment on {}.",
                        Anchor {
                            path: c.path,
                            side: c.side,
                            start_line: c.start_line,
                            line: c.line
                        }
                        .place()
                    ),
                ),
                None => Vec::new(),
            }
        }
        CommentRef::Remote { thread, comment } => {
            let Some(s) = app.diff.as_mut() else {
                return Vec::new();
            };
            s.submitting = true;
            let id = s.id.clone();
            app.mark_dirty();
            vec![Cmd::DeleteComment {
                id,
                thread: thread.clone(),
                comment: comment.clone(),
            }]
        }
    }
}

/// The answer to [`Cmd::EditComment`].
pub fn on_edited(
    app: &mut App,
    id: &rb_core::ChangeId,
    thread: &ThreadId,
    comment: &CommentId,
    body: &str,
    result: Result<(), String>,
    demo: bool,
) -> Vec<Cmd> {
    let here = app.diff.as_ref().is_some_and(|s| &s.id == id);
    if here {
        if let Some(s) = app.diff.as_mut() {
            s.submitting = false;
            if let Some(c) = s.composer.as_mut() {
                c.sending = false;
            }
        }
    }
    match result {
        Ok(()) => {
            if here {
                let keep = diff::cursor_id_of(app);
                if let Some(s) = app.diff.as_mut() {
                    s.composer = None;
                    if let Some(c) = s
                        .data
                        .as_mut()
                        .and_then(|d| d.threads.iter_mut().find(|t| &t.id == thread))
                        .and_then(|t| t.comments.iter_mut().find(|c| &c.id == comment))
                    {
                        c.body = body.to_string();
                    }
                }
                diff::rebuild(app, keep);
            }
            app.mark_dirty();
            let mut cmds = push_toast(
                app,
                Notice::new(
                    NoticeKind::Success,
                    format!("Comment updated{}", composer::demo_suffix(demo)),
                ),
            );
            if !demo {
                cmds.push(Cmd::LoadInfo(id.clone()));
            }
            cmds
        }
        Err(err) => {
            app.mark_dirty();
            push_toast(
                app,
                composer::failure(
                    "Couldn't update your comment",
                    err.trim_end_matches('.'),
                    "Your text is still in the box. Press ⏎ to try again.",
                ),
            )
        }
    }
}

/// The answer to [`Cmd::DeleteComment`].
pub fn on_deleted(
    app: &mut App,
    id: &rb_core::ChangeId,
    thread: &ThreadId,
    comment: &CommentId,
    result: Result<(), String>,
    demo: bool,
) -> Vec<Cmd> {
    let here = app.diff.as_ref().is_some_and(|s| &s.id == id);
    if here {
        if let Some(s) = app.diff.as_mut() {
            s.submitting = false;
        }
    }
    match result {
        Ok(()) => {
            if here {
                let keep = diff::cursor_id_of(app);
                if let Some(data) = app.diff.as_mut().and_then(|s| s.data.as_mut()) {
                    if let Some(t) = data.threads.iter_mut().find(|t| &t.id == thread) {
                        t.comments.retain(|c| &c.id != comment);
                    }
                    data.threads.retain(|t| !t.comments.is_empty());
                }
                diff::rebuild(app, keep);
            }
            app.mark_dirty();
            let mut cmds = push_toast(
                app,
                Notice::new(
                    NoticeKind::Success,
                    format!("Comment deleted{}", composer::demo_suffix(demo)),
                ),
            );
            if !demo {
                cmds.push(Cmd::LoadInfo(id.clone()));
            }
            cmds
        }
        Err(err) => {
            app.mark_dirty();
            let still = match id.kind {
                ForgeKind::GitHub => "It's still pending on GitHub.",
                ForgeKind::GitLab => "It's still pending on GitLab.",
            };
            push_toast(
                app,
                composer::failure(
                    "Couldn't delete that comment",
                    err.trim_end_matches('.'),
                    still,
                ),
            )
        }
    }
}

/// What to do with a comment whose line left the diff: add it to the summary, discard it, or
/// leave it as it is.
pub fn outdated_choice(app: &mut App, index: usize, choice: usize) -> Vec<Cmd> {
    match choice {
        0 => to_summary(app, index),
        1 => delete_confirmed(app, &CommentRef::Draft(index)),
        _ => Vec::new(),
    }
}

fn to_summary(app: &mut App, index: usize) -> Vec<Cmd> {
    let keep = diff::cursor_id_of(app);
    let Some(data) = app.diff.as_mut().and_then(|s| s.data.as_mut()) else {
        return Vec::new();
    };
    if index >= data.draft.comments.len() {
        return Vec::new();
    }
    let c = data.draft.comments.remove(index);
    let place = Anchor {
        path: c.path.clone(),
        side: c.side,
        start_line: c.start_line,
        line: c.line,
    }
    .place();
    let note = format!("On {place}, which is no longer in this diff:\n\n{}", c.body);
    data.draft.body = if data.draft.body.trim().is_empty() {
        note
    } else {
        format!("{}\n\n{note}", data.draft.body.trim_end())
    };
    diff::rebuild(app, keep);
    app.mark_dirty();
    info(
        app,
        "Moved to your review summary as a general comment. It posts with the review.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::diffview::tests::{draft, thread, PATCH};
    use crate::app::{update, AppConfig, DiffData, Msg, Screen};
    use crossterm::event::{KeyCode, KeyEvent};
    use rb_core::{Comment, FilePatch, FileStatus, ReviewDraft, SourceId, Timestamp};
    use rb_theme::ColourDepth;

    fn id() -> rb_core::ChangeId {
        rb_core::ChangeId {
            source_id: SourceId("s".into()),
            kind: ForgeKind::GitHub,
            repo: "o/r".into(),
            number: 1,
        }
    }

    fn open(
        comments: Vec<rb_core::DraftComment>,
        threads: Vec<rb_core::Thread>,
        edit: bool,
    ) -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        diff::open_change(&mut app, &id());
        let file = FilePatch {
            path: "src/a.rs".into(),
            old_path: None,
            status: FileStatus::Modified,
            adds: 2,
            dels: 1,
            patch: Some(PATCH.to_string()),
        };
        let data = DiffData::new(
            vec![file],
            threads,
            ReviewDraft {
                body: String::new(),
                comments,
            },
        )
        .with_pending_edit(edit);
        update(
            &mut app,
            Msg::DiffLoaded {
                id: id(),
                result: Ok(Box::new(data)),
            },
        );
        app
    }

    fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
        update(app, Msg::Key(KeyEvent::from(code)))
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    fn goto_line(app: &mut App, new_line: u32) {
        diff::goto_anchor(app, "src/a.rs", Side::New, new_line);
    }

    fn state(app: &App) -> &DiffState {
        app.diff.as_ref().unwrap()
    }

    fn bodies(app: &App) -> Vec<String> {
        state(app)
            .data
            .as_ref()
            .unwrap()
            .draft
            .comments
            .iter()
            .map(|c| c.body.clone())
            .collect()
    }

    fn status(app: &App) -> String {
        app.status
            .as_ref()
            .map(|e| e.notice.text.clone())
            .unwrap_or_default()
    }

    fn range_comment() -> rb_core::DraftComment {
        let mut c = draft(3, "Range note.");
        c.start_line = Some(2);
        c
    }

    fn pending_thread(line: u32) -> rb_core::Thread {
        let mut t = thread(line, Side::New);
        t.id = ThreadId::new("PRRT_p");
        t.pending = true;
        t.comments = vec![Comment {
            id: CommentId::new("PRRC_p"),
            author: "me".into(),
            body: "Started on the web.".into(),
            created_at: Timestamp(0),
            pending: true,
        }];
        t
    }

    #[test]
    fn editing_saves_in_place_keeping_order_anchor_and_range() {
        let mut app = open(
            vec![draft(2, "first"), range_comment(), draft(4, "third")],
            Vec::new(),
            false,
        );
        goto_line(&mut app, 3);
        press(&mut app, KeyCode::Char('e'));
        let composer = state(&app).composer.as_ref().expect("the composer opens");
        assert_eq!(composer.editor.text(), "Range note.");
        assert!(composer
            .title()
            .starts_with("Edit comment · a.rs lines 2–3"));
        type_text(&mut app, " Edited.");
        press(&mut app, KeyCode::Enter);
        assert!(state(&app).composer.is_none());
        assert_eq!(bodies(&app), ["first", "Range note. Edited.", "third"]);
        let edited = &state(&app).data.as_ref().unwrap().draft.comments[1];
        assert_eq!((edited.start_line, edited.line), (Some(2), 3));
        assert_eq!(status(&app), "Comment updated. It's still pending.");
    }

    #[test]
    fn enter_on_a_line_with_a_comment_edits_it_and_esc_asks_only_after_a_change() {
        let mut app = open(vec![draft(2, "first")], Vec::new(), false);
        goto_line(&mut app, 2);
        press(&mut app, KeyCode::Enter);
        assert!(state(&app).composer.is_some());
        press(&mut app, KeyCode::Esc);
        assert!(
            state(&app).composer.is_none(),
            "untouched text closes quietly"
        );
        assert!(state(&app).confirm.is_none());

        press(&mut app, KeyCode::Char('e'));
        type_text(&mut app, "!");
        press(&mut app, KeyCode::Esc);
        assert!(state(&app).confirm.is_some(), "a change asks first");
        press(&mut app, KeyCode::Char('n'));
        assert!(state(&app).composer.is_some());
        assert_eq!(bodies(&app), ["first"]);
    }

    #[test]
    fn deleting_asks_first_defaults_to_no_and_names_what_goes() {
        let mut app = open(
            vec![draft(2, "first"), draft(4, "third")],
            Vec::new(),
            false,
        );
        goto_line(&mut app, 4);
        press(&mut app, KeyCode::Char('d'));
        let confirm = state(&app).confirm.clone().expect("a confirm");
        assert!(!confirm.yes, "the safe answer is the default");
        assert_eq!(confirm.title(), "Delete this pending comment?");
        match &confirm.kind {
            crate::app::composer::ConfirmKind::DeleteComment { what, remote, .. } => {
                assert!(
                    what.contains("a.rs line 4") && what.contains("third"),
                    "{what}"
                );
                assert!(!remote);
            }
            other => panic!("expected a delete confirm, got {other:?}"),
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(bodies(&app), ["first", "third"], "Enter on No keeps it");

        press(&mut app, KeyCode::Delete);
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(bodies(&app), ["first"]);
        assert!(status(&app).contains("Deleted your comment on a.rs line 4"));
    }

    #[test]
    fn several_comments_on_one_line_are_picked_with_comma_and_period() {
        let mut app = open(
            vec![
                draft(4, "one"),
                draft(2, "other"),
                draft(4, "two"),
                draft(4, "three"),
            ],
            Vec::new(),
            false,
        );
        goto_line(&mut app, 4);
        assert_eq!(picked(state(&app)), (3, 0));
        assert!(picked_block(state(&app)).is_some());
        press(&mut app, KeyCode::Char('.'));
        assert_eq!(status(&app), "Comment 2 of 3 on this line");
        press(&mut app, KeyCode::Char('.'));
        press(&mut app, KeyCode::Char('.'));
        assert_eq!(status(&app), "Comment 1 of 3 on this line", "it wraps");
        press(&mut app, KeyCode::Char(','));
        assert_eq!(status(&app), "Comment 3 of 3 on this line");
        press(&mut app, KeyCode::Char('e'));
        assert_eq!(
            state(&app).composer.as_ref().unwrap().editor.text(),
            "three"
        );
        type_text(&mut app, "!");
        press(&mut app, KeyCode::Enter);
        assert_eq!(bodies(&app), ["one", "other", "two", "three!"]);

        goto_line(&mut app, 2);
        assert_eq!(picked(state(&app)), (1, 0), "moving resets the pick");
        assert!(
            picked_block(state(&app)).is_none(),
            "one comment needs no marker"
        );
    }

    #[test]
    fn a_line_without_a_comment_says_so() {
        let mut app = open(vec![draft(2, "first")], Vec::new(), false);
        goto_line(&mut app, 4);
        for key in ['e', 'd', '.'] {
            press(&mut app, KeyCode::Char(key));
            assert!(
                status(&app).contains("no pending comment on this line"),
                "{key}"
            );
        }
        assert!(state(&app).composer.is_none() && state(&app).confirm.is_none());
    }

    #[test]
    fn a_pending_comment_from_the_forge_is_edited_through_the_provider() {
        let mut app = open(Vec::new(), vec![pending_thread(2)], true);
        goto_line(&mut app, 2);
        press(&mut app, KeyCode::Char('e'));
        assert_eq!(
            state(&app).composer.as_ref().unwrap().editor.text(),
            "Started on the web."
        );
        type_text(&mut app, " Reworded.");
        let cmds = press(&mut app, KeyCode::Enter);
        assert!(cmds.is_empty(), "a confirm comes first");
        assert_eq!(
            state(&app).confirm.as_ref().unwrap().title(),
            "Update this pending comment?"
        );
        let cmds = press(&mut app, KeyCode::Char('y'));
        match cmds.as_slice() {
            [Cmd::EditComment {
                thread,
                comment,
                body,
                ..
            }] => {
                assert_eq!((thread.as_str(), comment.as_str()), ("PRRT_p", "PRRC_p"));
                assert_eq!(body, "Started on the web. Reworded.");
            }
            other => panic!("expected one edit, got {other:?}"),
        }
        assert!(state(&app).composer.as_ref().unwrap().sending);

        update(
            &mut app,
            Msg::CommentEdited {
                id: id(),
                thread: ThreadId::new("PRRT_p"),
                comment: CommentId::new("PRRC_p"),
                body: "Started on the web. Reworded.".into(),
                result: Ok(()),
                demo: false,
            },
        );
        let data = state(&app).data.as_ref().unwrap();
        assert!(state(&app).composer.is_none());
        assert_eq!(
            data.threads[0].comments[0].body,
            "Started on the web. Reworded."
        );
    }

    #[test]
    fn a_failed_forge_edit_keeps_the_text_and_says_what_to_do() {
        let mut app = open(Vec::new(), vec![pending_thread(2)], true);
        goto_line(&mut app, 2);
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('y'));
        update(
            &mut app,
            Msg::CommentEdited {
                id: id(),
                thread: ThreadId::new("PRRT_p"),
                comment: CommentId::new("PRRC_p"),
                body: "x".into(),
                result: Err("GitHub is having a moment.".into()),
                demo: false,
            },
        );
        assert!(state(&app).composer.is_some());
        assert!(!state(&app).composer.as_ref().unwrap().sending);
        let toast = &app.toasts.last().unwrap().notice.text;
        assert!(
            toast.contains("Couldn't update your comment") && toast.contains("try again"),
            "{toast}"
        );
    }

    #[test]
    fn deleting_a_forge_comment_asks_then_calls_the_provider() {
        let mut app = open(Vec::new(), vec![pending_thread(2)], true);
        goto_line(&mut app, 2);
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(
            state(&app).confirm.as_ref().unwrap().title(),
            "Delete this pending comment on the forge?"
        );
        let cmds = press(&mut app, KeyCode::Char('y'));
        assert!(matches!(cmds.as_slice(), [Cmd::DeleteComment { .. }]));
        update(
            &mut app,
            Msg::CommentDeleted {
                id: id(),
                thread: ThreadId::new("PRRT_p"),
                comment: CommentId::new("PRRC_p"),
                result: Ok(()),
                demo: true,
            },
        );
        assert!(state(&app).data.as_ref().unwrap().threads.is_empty());
        assert_eq!(
            app.toasts.last().unwrap().notice.text,
            "Comment deleted (demo)"
        );
    }

    #[test]
    fn a_failed_forge_delete_says_it_is_still_pending() {
        let mut app = open(Vec::new(), vec![pending_thread(2)], true);
        goto_line(&mut app, 2);
        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Char('y'));
        update(
            &mut app,
            Msg::CommentDeleted {
                id: id(),
                thread: ThreadId::new("PRRT_p"),
                comment: CommentId::new("PRRC_p"),
                result: Err("nope".into()),
                demo: false,
            },
        );
        assert_eq!(state(&app).data.as_ref().unwrap().threads.len(), 1);
        assert!(app
            .toasts
            .last()
            .unwrap()
            .notice
            .text
            .contains("still pending on GitHub"));
    }

    #[test]
    fn forge_comments_are_hidden_where_the_provider_cannot_edit_them() {
        let mut app = open(Vec::new(), vec![pending_thread(2)], false);
        goto_line(&mut app, 2);
        assert!(entries(state(&app)).is_empty());
        press(&mut app, KeyCode::Char('e'));
        assert!(state(&app).composer.is_none());
    }

    #[test]
    fn the_files_pane_lists_each_comment_and_enter_jumps_to_it() {
        let mut app = open(
            vec![draft(2, "first"), draft(22, "later")],
            Vec::new(),
            false,
        );
        goto_line(&mut app, 2);
        let labels: Vec<String> = entries(state(&app)).iter().map(Entry::label).collect();
        assert_eq!(labels, ["a.rs:2 first", "a.rs:22 later"]);
        press(&mut app, KeyCode::Tab);
        assert_eq!(state(&app).focus, DiffFocus::Review);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(state(&app).review_sel, 1);
        press(&mut app, KeyCode::Enter);
        let s = state(&app);
        assert_eq!(s.focus, DiffFocus::Diff);
        let line = s.view.rows.line_id(s.view.cursor).unwrap();
        assert_eq!((line.hunk, line.line), (1, 2));
        assert_eq!(picked(s), (1, 0));
    }

    #[test]
    fn a_comment_whose_line_is_gone_stays_visible_and_offers_a_way_out() {
        let mut app = open(
            vec![draft(2, "keep me"), draft(99, "lost line")],
            Vec::new(),
            false,
        );
        let all = entries(state(&app));
        assert_eq!(
            all.iter().map(|e| e.outdated).collect::<Vec<_>>(),
            [false, true]
        );
        let block = (0..state(&app).view.rows.len())
            .filter_map(|r| match state(&app).view.rows.row(r) {
                Some(crate::app::diffview::Row::Block { block, line: 0 }) => {
                    state(&app).view.rows.block(block)
                }
                _ => None,
            })
            .find(|b| b.title.contains("line 99"))
            .expect("it is still in the review block");
        assert!(block.title.ends_with("outdated"), "{}", block.title);

        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Enter);
        let confirm = state(&app).confirm.clone().expect("options appear");
        assert_eq!(confirm.title(), "This comment's line is gone");
        assert_eq!(
            confirm.choices().unwrap(),
            ["Add to summary", "Discard", "Leave it"]
        );
        press(&mut app, KeyCode::Char('g'));
        let data = state(&app).data.as_ref().unwrap();
        assert_eq!(data.draft.comments.len(), 1);
        assert!(data.draft.body.contains("lost line") && data.draft.body.contains("line 99"));
    }

    #[test]
    fn an_outdated_comment_can_be_discarded_or_left() {
        let mut app = open(vec![draft(99, "lost line")], Vec::new(), false);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        assert_eq!(bodies(&app), ["lost line"], "leaving it keeps it");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('d'));
        assert!(bodies(&app).is_empty());
        assert_eq!(app.screen, Screen::Diff);
    }

    #[test]
    fn first_words_skip_fences_and_cut_long_lines() {
        assert_eq!(
            first_words("\n```suggestion\nx\n```\nReal words", 40),
            "Real words"
        );
        assert_eq!(
            first_words(&"a".repeat(60), 10),
            format!("{}…", "a".repeat(9))
        );
        assert_eq!(first_words("", 10), "");
        assert_eq!(first_words("```suggestion\nx\n```", 20), "suggestion");
    }

    #[test]
    fn the_review_block_grows_for_the_list_a_kept_verdict_and_a_stale_note() {
        let mut app = open(vec![draft(2, "a"), draft(3, "b")], Vec::new(), false);
        assert_eq!(review_extra(state(&app)), 2);
        app.diff.as_mut().unwrap().verdict = Some(rb_core::Verdict::Approve);
        app.diff.as_mut().unwrap().stale = true;
        assert_eq!(review_extra(state(&app)), 4);
    }
}
