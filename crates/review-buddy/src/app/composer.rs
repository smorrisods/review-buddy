//! The composer and the confirmations around it: pure state and transitions. Writing to a forge
//! leaves as a [`Cmd`] and comes back as a [`Msg`], so none of this does I/O.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_core::{
    Capabilities, ChangeId, Comment, DraftComment, FeatureAction, ForgeKind, ReviewDraft, Side,
    Thread, ThreadId, Verdict,
};
use rb_github::plan_review;

use super::diff::{reveal_cursor, viewport};
use super::editor::Editor;
use super::update::{push_toast, set_status};
use super::{diff, App, Cmd, Notice, NoticeKind};
use crate::ui::layout;

/// Where a comment attaches. `start_line` is set for a range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub path: String,
    pub side: Side,
    pub start_line: Option<u32>,
    pub line: u32,
}

impl Anchor {
    pub fn place(&self) -> String {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        match self.start_line {
            Some(start) if start != self.line => format!("{name} lines {start}–{}", self.line),
            _ => format!("{name} line {}", self.line),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A new comment on a line or range.
    Line(Anchor),
    /// A reply on an existing thread.
    Reply { thread: ThreadId, place: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composer {
    pub target: Target,
    pub editor: Editor,
    /// The text the composer opened with, so a prefilled suggestion alone isn't "unsaved".
    initial: String,
    /// The text is on its way to the forge.
    pub sending: bool,
}

impl Composer {
    pub fn new(target: Target, prefill: &str) -> Self {
        Self {
            target,
            editor: Editor::with_text(prefill),
            initial: prefill.to_string(),
            sending: false,
        }
    }

    /// Whether closing would lose something the user typed.
    pub fn is_dirty(&self) -> bool {
        !self.editor.is_blank() && self.editor.text() != self.initial
    }

    pub fn title(&self) -> String {
        let suggestion = self.editor.text().contains("```suggestion");
        let (what, place) = match &self.target {
            Target::Line(anchor) => ("Comment", anchor.place()),
            Target::Reply { place, .. } => ("Reply", place.clone()),
        };
        if suggestion {
            format!("{what} · {place} · with suggestion")
        } else {
            format!("{what} · {place}")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Throw away the composer's text.
    Discard,
    /// Send one comment or reply now.
    PostNow { summary: String, body: String },
    /// Submit the whole review with an approval.
    Approve {
        summary: String,
        comments: Vec<String>,
        verdict: Verdict,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub kind: ConfirmKind,
    /// The confirming button has focus. Discarding starts on the safe choice instead.
    pub yes: bool,
}

impl Confirm {
    fn new(kind: ConfirmKind) -> Self {
        let yes = !matches!(kind, ConfirmKind::Discard);
        Self { kind, yes }
    }

    pub fn title(&self) -> &'static str {
        match self.kind {
            ConfirmKind::Discard => "Discard this comment?",
            ConfirmKind::PostNow { .. } => "Post this comment now?",
            ConfirmKind::Approve { .. } => "Approve this change?",
        }
    }

    /// The labels of the two buttons: the safe one first.
    pub fn buttons(&self) -> (&'static str, &'static str) {
        match self.kind {
            ConfirmKind::Discard => ("No, keep editing", "Discard"),
            ConfirmKind::PostNow { .. } => ("Cancel", "Post now"),
            ConfirmKind::Approve { .. } => ("Cancel", "Approve"),
        }
    }
}

fn state(app: &mut App) -> Option<&mut diff::DiffState> {
    app.diff.as_mut()
}

fn forge_name(kind: ForgeKind) -> &'static str {
    match kind {
        ForgeKind::GitHub => "GitHub",
        ForgeKind::GitLab => "GitLab",
    }
}

fn warn(app: &mut App, text: impl Into<String>) -> Vec<Cmd> {
    set_status(app, Notice::new(NoticeKind::Warning, text))
}

fn info(app: &mut App, text: impl Into<String>) -> Vec<Cmd> {
    set_status(app, Notice::new(NoticeKind::Info, text))
}

/// What `plan_review` rejected, without the error's prefix.
fn plan_message(err: &rb_core::Error) -> String {
    match err {
        rb_core::Error::Api(message) => message.clone(),
        other => other.to_string(),
    }
}

fn ready(app: &App) -> bool {
    app.diff
        .as_ref()
        .is_some_and(|s| s.data.is_some() && !s.has_overlay())
}

/// The anchor of the line under the cursor.
fn cursor_anchor(app: &App) -> Option<Anchor> {
    let s = app.diff.as_ref()?;
    let file = s.current()?;
    let id = s.view.rows.line_id(s.view.cursor)?;
    let anchor = file.diff.parsed()?.anchor(id)?;
    Some(Anchor {
        path: file.diff.path.clone(),
        side: anchor.side,
        start_line: None,
        line: anchor.line,
    })
}

/// The open thread that hangs from the cursor line, if any.
fn cursor_thread(app: &App) -> Option<(&Thread, Anchor)> {
    let anchor = cursor_anchor(app)?;
    let data = app.diff.as_ref()?.data.as_ref()?;
    let mut here = data.threads.iter().filter(|t| {
        t.path.as_deref() == Some(anchor.path.as_str())
            && t.line == Some(anchor.line)
            && t.side == anchor.side
            && !t.outdated
    });
    let first = here.next()?;
    let thread = here.find(|t| !t.resolved).filter(|_| first.resolved);
    Some((thread.unwrap_or(first), anchor))
}

/// Opens the composer for a new comment on the cursor line, optionally prefilled.
pub fn open_comment(app: &mut App) -> Vec<Cmd> {
    open_with(app, "")
}

/// Like [`open_comment`] with `prefill` already in the box, for suggestions.
pub fn open_with(app: &mut App, prefill: &str) -> Vec<Cmd> {
    if !ready(app) {
        return Vec::new();
    }
    let Some(anchor) = cursor_anchor(app) else {
        return info(app, "This file has no lines to comment on.");
    };
    open(app, Composer::new(Target::Line(anchor), prefill))
}

pub fn open_reply(app: &mut App) -> Vec<Cmd> {
    if !ready(app) {
        return Vec::new();
    }
    let Some((thread, anchor)) = cursor_thread(app) else {
        return info(
            app,
            "There's no thread on this line to reply to. Press c to comment instead.",
        );
    };
    let target = Target::Reply {
        thread: thread.id.clone(),
        place: anchor.place(),
    };
    open(app, Composer::new(target, ""))
}

fn open(app: &mut App, composer: Composer) -> Vec<Cmd> {
    if let Some(s) = state(app) {
        s.composer = Some(composer);
    }
    keep_cursor_visible(app);
    app.mark_dirty();
    Vec::new()
}

fn keep_cursor_visible(app: &mut App) {
    let code = viewport(app).code;
    let Some(s) = app.diff.as_mut() else {
        return;
    };
    let Some(composer) = &s.composer else {
        return;
    };
    let dock = layout::composer_dock(code, composer.editor.line_count()).height;
    let height = usize::from(code.height.saturating_sub(dock)).max(1);
    let view = &mut s.view;
    let (top, bottom) = view.rows.reveal_span(view.cursor);
    view.scroll = super::diffview::reveal(view.scroll, top, bottom, height, view.rows.len());
}

/// `x`: requesting changes is explained rather than hidden, since the key is on the screen.
pub fn request_changes(app: &mut App) -> Vec<Cmd> {
    let Some((caps, kind)) = caps_and_kind(app) else {
        return Vec::new();
    };
    match caps.explain_unsupported(FeatureAction::RequestChanges, forge_name(kind)) {
        Some(text) => info(app, text),
        None => info(
            app,
            "Requesting changes isn't available yet. Approve with a, or leave a comment with c.",
        ),
    }
}

fn caps_and_kind(app: &App) -> Option<(Capabilities, ForgeKind)> {
    let s = app.diff.as_ref()?;
    Some((s.data.as_ref()?.caps, s.id.kind))
}

/// `a`: previews what approving will do.
pub fn open_approve(app: &mut App) -> Vec<Cmd> {
    if !ready(app) {
        return Vec::new();
    }
    if app.diff.as_ref().is_some_and(|s| s.submitting) {
        return info(app, "Still sending. One moment.");
    }
    let Some(s) = app.diff.as_ref() else {
        return Vec::new();
    };
    let Some(data) = s.data.as_ref() else {
        return Vec::new();
    };
    let plan = match plan_review(&data.draft, Verdict::Approve) {
        Ok(plan) => plan,
        Err(err) => return warn(app, plan_message(&err)),
    };
    let comments = data.draft.comments.iter().map(comment_line).collect();
    let confirm = Confirm::new(ConfirmKind::Approve {
        summary: plan.summary(),
        comments,
        verdict: plan.verdict,
    });
    set_confirm(app, confirm)
}

fn comment_line(c: &DraftComment) -> String {
    let anchor = Anchor {
        path: c.path.clone(),
        side: c.side,
        start_line: c.start_line,
        line: c.line,
    };
    let first = c.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    format!("{} · {first}", anchor.place())
}

fn set_confirm(app: &mut App, confirm: Confirm) -> Vec<Cmd> {
    if let Some(s) = state(app) {
        s.confirm = Some(confirm);
    }
    app.mark_dirty();
    Vec::new()
}

/// Keys while a composer or confirmation is open. They never reach the screen beneath.
pub fn on_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    let Some(s) = app.diff.as_ref() else {
        return Vec::new();
    };
    if s.confirm.is_some() {
        return on_confirm_key(app, key);
    }
    if s.composer.as_ref().is_some_and(|c| c.sending) {
        return Vec::new();
    }
    on_composer_key(app, key)
}

fn on_confirm_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    let Some(confirm) = app.diff.as_mut().and_then(|s| s.confirm.as_mut()) else {
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

pub(super) fn answer(app: &mut App, yes: bool) -> Vec<Cmd> {
    let Some(confirm) = state(app).and_then(|s| s.confirm.take()) else {
        return Vec::new();
    };
    app.mark_dirty();
    if !yes {
        return Vec::new();
    }
    match confirm.kind {
        ConfirmKind::Discard => {
            if let Some(s) = state(app) {
                s.composer = None;
            }
            Vec::new()
        }
        ConfirmKind::PostNow { .. } => send_composer(app),
        ConfirmKind::Approve { verdict, .. } => send_review(app, verdict),
    }
}

/// A click on the composer's text: puts the caret under the pointer.
pub fn place_cursor(app: &mut App, column: u16, row: u16, at: (u16, u16, usize, usize)) {
    let (x, y, first, across) = at;
    let Some(c) = state(app).and_then(|s| s.composer.as_mut()) else {
        return;
    };
    if c.sending {
        return;
    }
    let target = first + usize::from(row.saturating_sub(y));
    let target = target.min(c.editor.line_count() - 1);
    let cells = usize::from(column.saturating_sub(x));
    let col = char_at(&c.editor.lines()[target], across, cells);
    c.editor.set_cursor(target, col);
    app.mark_dirty();
}

/// The character index of the cell `cells` from the left edge of `line` once `across`
/// characters have scrolled off. Clicks past the end land after the last character.
fn char_at(line: &str, across: usize, cells: usize) -> usize {
    use unicode_width::UnicodeWidthChar;
    let mut used = 0;
    let mut index = across;
    for ch in line.chars().skip(across) {
        let w = if ch == '\t' {
            1
        } else {
            ch.width().unwrap_or(0)
        };
        if used + w > cells {
            break;
        }
        used += w;
        index += 1;
    }
    index
}

/// The wheel over the composer moves its caret a line, and the view follows the caret.
pub fn scroll_text(app: &mut App, down: bool) {
    let Some(c) = state(app).and_then(|s| s.composer.as_mut()) else {
        return;
    };
    if down {
        c.editor.down();
    } else {
        c.editor.up();
    }
    app.mark_dirty();
}

fn on_composer_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Esc => return close_or_ask(app),
        KeyCode::Enter if ctrl => return post_now(app),
        KeyCode::Char('p') if ctrl => return post_now(app),
        KeyCode::Enter if shift || alt => edit(app, Editor::newline),
        KeyCode::Char('j') if ctrl => edit(app, Editor::newline),
        KeyCode::Enter => return submit(app),
        _ => {
            if !edit_key(app, key.code, ctrl, alt) {
                return Vec::new();
            }
        }
    }
    keep_cursor_visible(app);
    app.mark_dirty();
    Vec::new()
}

fn edit_key(app: &mut App, code: KeyCode, ctrl: bool, alt: bool) -> bool {
    let word = ctrl || alt;
    match code {
        KeyCode::Char('b') if alt => edit(app, Editor::word_left),
        KeyCode::Char('f') if alt => edit(app, Editor::word_right),
        KeyCode::Char('w') if ctrl => edit(app, Editor::delete_word),
        KeyCode::Backspace if word => edit(app, Editor::delete_word),
        KeyCode::Left if word => edit(app, Editor::word_left),
        KeyCode::Right if word => edit(app, Editor::word_right),
        KeyCode::Home if ctrl => edit(app, Editor::start),
        KeyCode::End if ctrl => edit(app, Editor::finish),
        _ if word => return false,
        KeyCode::Char(c) => edit(app, |e| e.insert_char(c)),
        KeyCode::Tab => edit(app, |e| e.insert_char('\t')),
        KeyCode::Backspace => edit(app, Editor::backspace),
        KeyCode::Delete => edit(app, Editor::delete),
        KeyCode::Left => edit(app, Editor::left),
        KeyCode::Right => edit(app, Editor::right),
        KeyCode::Up => edit(app, Editor::up),
        KeyCode::Down => edit(app, Editor::down),
        KeyCode::Home => edit(app, Editor::home),
        KeyCode::End => edit(app, Editor::end),
        _ => return false,
    }
    true
}

fn edit(app: &mut App, f: impl FnOnce(&mut Editor)) {
    if let Some(c) = state(app).and_then(|s| s.composer.as_mut()) {
        f(&mut c.editor);
    }
}

/// Bracketed paste goes into the composer; anywhere else it's ignored.
pub fn on_paste(app: &mut App, text: &str) -> Vec<Cmd> {
    let open = app
        .diff
        .as_ref()
        .is_some_and(|s| s.confirm.is_none() && s.composer.as_ref().is_some_and(|c| !c.sending));
    if open {
        edit(app, |e| e.insert_str(text));
        keep_cursor_visible(app);
        app.mark_dirty();
    }
    Vec::new()
}

fn close_or_ask(app: &mut App) -> Vec<Cmd> {
    let dirty = app
        .diff
        .as_ref()
        .and_then(|s| s.composer.as_ref())
        .is_some_and(Composer::is_dirty);
    if dirty {
        return set_confirm(app, Confirm::new(ConfirmKind::Discard));
    }
    if let Some(s) = state(app) {
        s.composer = None;
    }
    app.mark_dirty();
    Vec::new()
}

fn composer_body(app: &App) -> Option<String> {
    let c = app.diff.as_ref()?.composer.as_ref()?;
    Some(c.editor.text().trim().to_string()).filter(|b| !b.is_empty())
}

/// `⏎`: a comment joins the pending review; a reply goes straight to the thread.
fn submit(app: &mut App) -> Vec<Cmd> {
    let is_reply = matches!(
        app.diff
            .as_ref()
            .and_then(|s| s.composer.as_ref())
            .map(|c| &c.target),
        Some(Target::Reply { .. })
    );
    if is_reply {
        return post_now(app);
    }
    add_to_review(app)
}

fn add_to_review(app: &mut App) -> Vec<Cmd> {
    let Some(body) = composer_body(app) else {
        return info(app, "Write something first, or press esc to close.");
    };
    let Some(anchor) = composer_anchor(app) else {
        return Vec::new();
    };
    let keep = app
        .diff
        .as_ref()
        .and_then(|s| s.view.rows.line_id(s.view.cursor));
    let Some(s) = state(app) else {
        return Vec::new();
    };
    s.composer = None;
    let Some(data) = s.data.as_mut() else {
        return Vec::new();
    };
    data.draft.comments.push(DraftComment {
        path: anchor.path,
        side: anchor.side,
        start_line: anchor.start_line,
        line: anchor.line,
        body,
    });
    let pending = data.draft.comments.len();
    diff::rebuild(app, keep);
    let text = match pending {
        1 => "Added to your pending review. 1 comment waiting. Press a to approve.".to_string(),
        n => format!("Added to your pending review. {n} comments waiting. Press a to approve."),
    };
    info(app, text)
}

fn composer_anchor(app: &App) -> Option<Anchor> {
    match &app.diff.as_ref()?.composer.as_ref()?.target {
        Target::Line(anchor) => Some(anchor.clone()),
        Target::Reply { .. } => None,
    }
}

/// `⌃⏎`: a preview first, unless `confirm_post_now` is off.
fn post_now(app: &mut App) -> Vec<Cmd> {
    let Some(body) = composer_body(app) else {
        return info(app, "Write something first, or press esc to close.");
    };
    let summary = match composer_anchor(app) {
        Some(anchor) => {
            let one = ReviewDraft {
                body: String::new(),
                comments: vec![DraftComment {
                    path: anchor.path,
                    side: anchor.side,
                    start_line: anchor.start_line,
                    line: anchor.line,
                    body: body.clone(),
                }],
            };
            match plan_review(&one, Verdict::Comment) {
                Ok(plan) => plan.summary(),
                Err(err) => return warn(app, plan_message(&err)),
            }
        }
        None => "Reply to this thread".to_string(),
    };
    if app.confirm_post_now {
        return set_confirm(app, Confirm::new(ConfirmKind::PostNow { summary, body }));
    }
    send_composer(app)
}

/// Sends the composer's text: one standalone comment, or a reply.
fn send_composer(app: &mut App) -> Vec<Cmd> {
    let Some(body) = composer_body(app) else {
        return Vec::new();
    };
    let Some(s) = app.diff.as_ref() else {
        return Vec::new();
    };
    let id = s.id.clone();
    let Some(composer) = s.composer.as_ref() else {
        return Vec::new();
    };
    let cmd = match &composer.target {
        Target::Reply { thread, .. } => Cmd::Reply {
            id,
            thread: thread.clone(),
            body,
        },
        Target::Line(anchor) => Cmd::SubmitReview {
            id,
            draft: ReviewDraft {
                body: String::new(),
                comments: vec![DraftComment {
                    path: anchor.path.clone(),
                    side: anchor.side,
                    start_line: anchor.start_line,
                    line: anchor.line,
                    body,
                }],
            },
            verdict: Verdict::Comment,
        },
    };
    if let Some(s) = state(app) {
        s.submitting = true;
        if let Some(c) = s.composer.as_mut() {
            c.sending = true;
        }
    }
    app.mark_dirty();
    vec![cmd]
}

fn send_review(app: &mut App, verdict: Verdict) -> Vec<Cmd> {
    let Some(s) = state(app) else {
        return Vec::new();
    };
    let Some(data) = s.data.as_ref() else {
        return Vec::new();
    };
    let cmd = Cmd::SubmitReview {
        id: s.id.clone(),
        draft: data.draft.clone(),
        verdict,
    };
    s.submitting = true;
    app.mark_dirty();
    vec![cmd]
}

fn demo_suffix(demo: bool) -> &'static str {
    if demo {
        " (demo)"
    } else {
        ""
    }
}

fn failure(what: &str, err: &str, next: &str) -> Notice {
    Notice::new(NoticeKind::Warning, format!("{what}: {err}. {next}"))
}

/// The result of [`Cmd::SubmitReview`].
pub fn on_submitted(
    app: &mut App,
    id: &ChangeId,
    verdict: Verdict,
    result: Result<(), String>,
    demo: bool,
) -> Vec<Cmd> {
    let here = app.diff.as_ref().is_some_and(|s| &s.id == id);
    if here {
        if let Some(s) = state(app) {
            s.submitting = false;
            if let Some(c) = s.composer.as_mut() {
                c.sending = false;
            }
        }
    }
    let approving = verdict == Verdict::Approve;
    match result {
        Ok(()) => {
            if here {
                finish_submit(app, approving);
            }
            let text = if approving {
                "Approved"
            } else {
                "Comment posted"
            };
            let mut cmds = push_toast(
                app,
                Notice::new(NoticeKind::Success, format!("{text}{}", demo_suffix(demo))),
            );
            cmds.push(Cmd::LoadChanges);
            if !demo {
                cmds.push(Cmd::LoadInfo(id.clone()));
            }
            cmds
        }
        Err(err) => {
            app.mark_dirty();
            let notice = if approving {
                failure(
                    "Couldn't submit your review",
                    err.trim_end_matches('.'),
                    "Your comments are still pending. Press a to try again.",
                )
            } else {
                failure(
                    "Couldn't post your comment",
                    err.trim_end_matches('.'),
                    "Your text is still in the box. Press ⌃⏎ to try again.",
                )
            };
            push_toast(app, notice)
        }
    }
}

fn finish_submit(app: &mut App, approving: bool) {
    let keep = app
        .diff
        .as_ref()
        .and_then(|s| s.view.rows.line_id(s.view.cursor));
    if let Some(s) = state(app) {
        if approving {
            if let Some(data) = s.data.as_mut() {
                data.draft = ReviewDraft::default();
            }
        } else {
            s.composer = None;
        }
    }
    if approving {
        diff::rebuild(app, keep);
    }
    app.mark_dirty();
}

/// The result of [`Cmd::Reply`].
pub fn on_replied(
    app: &mut App,
    id: &ChangeId,
    thread: &ThreadId,
    result: Result<Comment, String>,
    demo: bool,
) -> Vec<Cmd> {
    let here = app.diff.as_ref().is_some_and(|s| &s.id == id);
    if here {
        if let Some(s) = state(app) {
            s.submitting = false;
            if let Some(c) = s.composer.as_mut() {
                c.sending = false;
            }
        }
    }
    match result {
        Ok(comment) => {
            if here {
                add_reply(app, thread, comment);
            }
            let mut cmds = push_toast(
                app,
                Notice::new(
                    NoticeKind::Success,
                    format!("Reply posted{}", demo_suffix(demo)),
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
                failure(
                    "Couldn't post your reply",
                    err.trim_end_matches('.'),
                    "Your text is still in the box. Press ⏎ to try again.",
                ),
            )
        }
    }
}

fn add_reply(app: &mut App, thread: &ThreadId, comment: Comment) {
    let keep = app
        .diff
        .as_ref()
        .and_then(|s| s.view.rows.line_id(s.view.cursor));
    if let Some(s) = state(app) {
        s.composer = None;
        if let Some(t) = s
            .data
            .as_mut()
            .and_then(|d| d.threads.iter_mut().find(|t| &t.id == thread))
        {
            t.comments.push(comment);
        }
    }
    diff::rebuild(app, keep);
    let height = usize::from(viewport(app).code.height);
    if let Some(s) = state(app) {
        reveal_cursor(s, height);
    }
    app.mark_dirty();
}

#[cfg(test)]
mod tests {
    #[test]
    fn char_at_maps_cells_to_characters_through_scroll_and_wide_glyphs() {
        assert_eq!(char_at("hello", 0, 0), 0);
        assert_eq!(char_at("hello", 0, 3), 3);
        assert_eq!(char_at("hello", 0, 40), 5, "past the end");
        assert_eq!(char_at("hello", 2, 1), 3, "two characters scrolled off");
        assert_eq!(char_at("日本語", 0, 2), 1, "each glyph is two cells wide");
        assert_eq!(char_at("a\tb", 0, 2), 2, "a tab shows as one space");
    }

    use crossterm::event::KeyEvent;
    use rb_core::{CommentId, FilePatch, FileStatus, Timestamp};
    use rb_theme::ColourDepth;

    use super::*;
    use crate::app::diffview::tests::{draft, thread, PATCH};
    use crate::app::{update, AppConfig, DiffData, DiffState, Msg, Screen};

    fn id() -> ChangeId {
        ChangeId {
            source_id: rb_core::SourceId("s".into()),
            kind: ForgeKind::GitHub,
            repo: "o/r".into(),
            number: 1,
        }
    }

    fn patch(path: &str) -> FilePatch {
        FilePatch {
            path: path.into(),
            old_path: None,
            status: rb_core::FileStatus::Modified,
            adds: 2,
            dels: 1,
            patch: Some(PATCH.into()),
        }
    }

    fn app_with(drafts: Vec<DraftComment>, caps: Capabilities) -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        app.diff = Some(DiffState::loading(id()));
        app.screen = Screen::Diff;
        let mut t = thread(2, Side::New);
        t.path = Some("src/a.rs".into());
        let data = DiffData::new(
            vec![patch("src/a.rs")],
            vec![t],
            ReviewDraft {
                body: String::new(),
                comments: drafts,
            },
        )
        .with_capabilities(caps);
        update(
            &mut app,
            Msg::DiffLoaded {
                id: id(),
                result: Ok(Box::new(data)),
            },
        );
        app.clear_dirty();
        app
    }

    fn app() -> App {
        app_with(Vec::new(), Capabilities::all())
    }

    fn key(app: &mut App, code: KeyCode) -> Vec<Cmd> {
        update(app, Msg::Key(KeyEvent::from(code)))
    }

    fn with(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Vec<Cmd> {
        update(app, Msg::Key(KeyEvent::new(code, modifiers)))
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            key(app, KeyCode::Char(c));
        }
    }

    fn goto(app: &mut App, new_line: u32) {
        for _ in 0..20 {
            if cursor_anchor(app).is_some_and(|a| a.side == Side::New && a.line == new_line) {
                return;
            }
            key(app, KeyCode::Char('j'));
        }
        panic!("line {new_line} not reachable");
    }

    fn diff(app: &App) -> &DiffState {
        app.diff.as_ref().unwrap()
    }

    fn status(app: &App) -> String {
        app.status.as_ref().unwrap().notice.text.clone()
    }

    fn toast(app: &App) -> String {
        app.toasts.last().unwrap().notice.text.clone()
    }

    fn submit_cmd(cmds: &[Cmd]) -> (&ReviewDraft, Verdict) {
        match cmds {
            [Cmd::SubmitReview { draft, verdict, .. }] => (draft, *verdict),
            other => panic!("expected one SubmitReview, got {other:?}"),
        }
    }

    fn submitted(app: &mut App, verdict: Verdict, result: Result<(), String>) -> Vec<Cmd> {
        update(
            app,
            Msg::ReviewSubmitted {
                id: id(),
                verdict,
                result,
                demo: true,
            },
        )
    }

    #[test]
    fn c_opens_the_composer_on_the_cursor_line() {
        let mut a = app();
        goto(&mut a, 3);
        key(&mut a, KeyCode::Char('c'));
        let c = diff(&a).composer.as_ref().expect("open");
        assert_eq!(c.title(), "Comment · a.rs line 3");
        assert!(c.editor.is_blank());
        assert!(a.is_dirty());
    }

    #[test]
    fn typing_goes_to_the_composer_not_to_global_keys() {
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "q? T o y c");
        assert!(!a.should_quit() && !a.help);
        assert_eq!(a.theme_name(), "Liminal HQ");
        assert_eq!(
            diff(&a).composer.as_ref().unwrap().editor.text(),
            "q? T o y c"
        );
    }

    #[test]
    fn enter_adds_the_comment_to_the_pending_review() {
        let mut a = app();
        goto(&mut a, 3);
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "Nit: rename this");
        let cmds = key(&mut a, KeyCode::Enter);
        assert!(
            matches!(cmds.as_slice(), [Cmd::After { .. }]),
            "only a status expiry"
        );
        let s = diff(&a);
        assert!(s.composer.is_none());
        let drafts = &s.data.as_ref().unwrap().draft.comments;
        assert_eq!(drafts.len(), 1);
        assert_eq!(
            (drafts[0].line, drafts[0].side, drafts[0].body.as_str()),
            (3, Side::New, "Nit: rename this")
        );
        assert_eq!(drafts[0].path, "src/a.rs");
        assert!(a.has_unsent_drafts());
        assert!(status(&a).contains("1 comment waiting"));
        let titles = (0..s.view.rows.len())
            .filter_map(|r| match s.view.rows.row(r)? {
                crate::app::diffview::Row::Block { block, line: 0 } => {
                    Some(s.view.rows.block(block)?.title.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            titles.iter().any(|t| t == "pending comment · line 3"),
            "{titles:?}"
        );
    }

    #[test]
    fn enter_on_blank_text_keeps_the_composer_and_explains() {
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "  ");
        key(&mut a, KeyCode::Enter);
        assert!(diff(&a).composer.is_some());
        assert!(status(&a).starts_with("Write something first"));
    }

    #[test]
    fn shift_enter_alt_enter_and_ctrl_j_insert_newlines() {
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "a");
        with(&mut a, KeyCode::Enter, KeyModifiers::SHIFT);
        type_text(&mut a, "b");
        with(&mut a, KeyCode::Enter, KeyModifiers::ALT);
        type_text(&mut a, "c");
        with(&mut a, KeyCode::Char('j'), KeyModifiers::CONTROL);
        type_text(&mut a, "d");
        let c = diff(&a).composer.as_ref().unwrap();
        assert_eq!(c.editor.text(), "a\nb\nc\nd");
        assert!(c.editor.line_count() == 4 && diff(&a).data.as_ref().unwrap().draft.is_empty());
    }

    #[test]
    fn editing_keys_move_and_delete() {
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "one two");
        with(&mut a, KeyCode::Left, KeyModifiers::ALT);
        type_text(&mut a, "X");
        key(&mut a, KeyCode::Home);
        key(&mut a, KeyCode::Delete);
        key(&mut a, KeyCode::End);
        key(&mut a, KeyCode::Backspace);
        let c = diff(&a).composer.as_ref().unwrap();
        assert_eq!(c.editor.text(), "ne Xtw");
        with(&mut a, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(diff(&a).composer.as_ref().unwrap().editor.text(), "ne ");
    }

    #[test]
    fn paste_inserts_into_the_composer_and_is_ignored_elsewhere() {
        let mut a = app();
        update(&mut a, Msg::Paste("ignored".into()));
        assert!(diff(&a).composer.is_none());
        key(&mut a, KeyCode::Char('c'));
        update(&mut a, Msg::Paste("first\r\nsecond".into()));
        assert_eq!(
            diff(&a).composer.as_ref().unwrap().editor.text(),
            "first\nsecond"
        );
    }

    #[test]
    fn esc_on_an_untouched_composer_closes_without_asking() {
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        key(&mut a, KeyCode::Esc);
        assert!(diff(&a).composer.is_none() && diff(&a).confirm.is_none());
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "  ");
        key(&mut a, KeyCode::Esc);
        assert!(
            diff(&a).composer.is_none(),
            "whitespace alone isn't worth keeping"
        );
    }

    #[test]
    fn a_prefilled_body_alone_does_not_count_as_unsaved() {
        let mut a = app();
        open_with(&mut a, "```suggestion\nfoo\n```");
        let c = diff(&a).composer.as_ref().unwrap();
        assert!(!c.is_dirty());
        assert!(c.title().ends_with("with suggestion"));
        key(&mut a, KeyCode::Char('x'));
        assert!(diff(&a).composer.as_ref().unwrap().is_dirty());
    }

    #[test]
    fn discarding_text_asks_first_and_defaults_to_no() {
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "draft");
        key(&mut a, KeyCode::Esc);
        let confirm = diff(&a).confirm.clone().expect("asks first");
        assert_eq!(confirm.kind, ConfirmKind::Discard);
        assert!(!confirm.yes, "focus starts on the safe choice");
        assert_eq!(confirm.buttons(), ("No, keep editing", "Discard"));

        key(&mut a, KeyCode::Enter);
        assert!(diff(&a).confirm.is_none());
        assert_eq!(diff(&a).composer.as_ref().unwrap().editor.text(), "draft");

        key(&mut a, KeyCode::Esc);
        key(&mut a, KeyCode::Esc);
        assert!(diff(&a).composer.is_some(), "esc answers no");

        key(&mut a, KeyCode::Esc);
        key(&mut a, KeyCode::Tab);
        assert!(diff(&a).confirm.as_ref().unwrap().yes);
        key(&mut a, KeyCode::Enter);
        assert!(diff(&a).composer.is_none() && diff(&a).confirm.is_none());
        assert!(!a.has_unsent_drafts());
    }

    #[test]
    fn y_and_n_answer_the_discard_confirm_directly() {
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "x");
        key(&mut a, KeyCode::Esc);
        key(&mut a, KeyCode::Char('n'));
        assert!(diff(&a).composer.is_some());
        key(&mut a, KeyCode::Esc);
        key(&mut a, KeyCode::Char('y'));
        assert!(diff(&a).composer.is_none());
    }

    #[test]
    fn ctrl_enter_previews_then_posts_one_standalone_comment() {
        let mut a = app_with(vec![draft(4, "Already pending")], Capabilities::all());
        goto(&mut a, 3);
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "Ship it");
        let cmds = with(&mut a, KeyCode::Enter, KeyModifiers::CONTROL);
        assert!(
            cmds.is_empty(),
            "nothing is sent before the preview is confirmed"
        );
        let confirm = diff(&a).confirm.clone().unwrap();
        assert_eq!(
            confirm.kind,
            ConfirmKind::PostNow {
                summary: "Post review with 1 comment".into(),
                body: "Ship it".into()
            }
        );
        assert!(confirm.yes, "posting isn't destructive, so ⏎ confirms");

        let cmds = key(&mut a, KeyCode::Enter);
        let (draft, verdict) = submit_cmd(&cmds);
        assert_eq!(verdict, Verdict::Comment);
        assert_eq!(
            draft.comments.len(),
            1,
            "the pending review stays out of it"
        );
        assert_eq!(
            (draft.comments[0].line, draft.comments[0].body.as_str()),
            (3, "Ship it")
        );
        assert!(diff(&a).submitting && diff(&a).composer.as_ref().unwrap().sending);

        type_text(&mut a, "ignored while sending");
        assert_eq!(diff(&a).composer.as_ref().unwrap().editor.text(), "Ship it");

        let cmds = submitted(&mut a, Verdict::Comment, Ok(()));
        assert!(diff(&a).composer.is_none() && !diff(&a).submitting);
        assert_eq!(toast(&a), "Comment posted (demo)");
        assert!(cmds.iter().any(|c| matches!(c, Cmd::LoadChanges)));
        assert_eq!(diff(&a).data.as_ref().unwrap().draft.comments.len(), 1);
    }

    #[test]
    fn post_now_can_skip_the_preview() {
        let mut a = app();
        a.confirm_post_now = false;
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "Hi");
        let cmds = with(&mut a, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(submit_cmd(&cmds).1, Verdict::Comment);
        assert!(diff(&a).confirm.is_none());
    }

    #[test]
    fn cancelling_the_post_now_preview_keeps_the_text() {
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "Hi");
        with(&mut a, KeyCode::Enter, KeyModifiers::CONTROL);
        let cmds = key(&mut a, KeyCode::Esc);
        assert!(cmds.is_empty());
        assert!(!diff(&a).submitting);
        assert_eq!(diff(&a).composer.as_ref().unwrap().editor.text(), "Hi");
    }

    #[test]
    fn a_failed_post_keeps_the_text_and_says_what_to_do_next() {
        let mut a = app();
        a.confirm_post_now = false;
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "Hi");
        with(&mut a, KeyCode::Enter, KeyModifiers::CONTROL);
        let cmds = submitted(
            &mut a,
            Verdict::Comment,
            Err("couldn't reach github.com".into()),
        );
        assert!(!cmds.iter().any(|c| matches!(c, Cmd::LoadChanges)));
        let c = diff(&a).composer.as_ref().expect("still open");
        assert_eq!((c.editor.text().as_str(), c.sending), ("Hi", false));
        assert!(!diff(&a).submitting);
        let t = toast(&a);
        assert!(
            t.contains("couldn't reach github.com") && t.contains("Press ⌃⏎ to try again"),
            "{t}"
        );
        assert_eq!(a.toasts.last().unwrap().notice.kind, NoticeKind::Warning);
        let cmds = with(&mut a, KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(submit_cmd(&cmds).1, Verdict::Comment, "retry works");
    }

    #[test]
    fn replying_posts_to_the_thread_and_shows_the_new_comment() {
        let mut a = app();
        goto(&mut a, 2);
        key(&mut a, KeyCode::Char('r'));
        let c = diff(&a).composer.as_ref().expect("reply composer");
        assert_eq!(c.title(), "Reply · a.rs line 2");
        type_text(&mut a, "Agreed");
        key(&mut a, KeyCode::Enter);
        let confirm = diff(&a).confirm.clone().unwrap();
        assert!(
            matches!(confirm.kind, ConfirmKind::PostNow { ref summary, .. } if summary == "Reply to this thread")
        );
        let cmds = key(&mut a, KeyCode::Enter);
        let Some(Cmd::Reply { thread, body, .. }) = cmds.first().cloned() else {
            panic!("expected a Reply, got {cmds:?}");
        };
        assert_eq!(body, "Agreed");
        let before = diff(&a).view.rows.len();
        update(
            &mut a,
            Msg::ReplyPosted {
                id: id(),
                thread: thread.clone(),
                result: Ok(Comment {
                    id: CommentId("new".into()),
                    author: "me".into(),
                    body: "Agreed".into(),
                    created_at: Timestamp(0),
                    pending: false,
                }),
                demo: true,
            },
        );
        let s = diff(&a);
        assert!(s.composer.is_none() && !s.submitting);
        assert_eq!(toast(&a), "Reply posted (demo)");
        assert!(s.view.rows.len() > before, "the thread block grew");
        assert_eq!(
            s.data.as_ref().unwrap().threads[0]
                .comments
                .last()
                .unwrap()
                .body,
            "Agreed"
        );
    }

    #[test]
    fn a_failed_reply_keeps_the_composer() {
        let mut a = app();
        a.confirm_post_now = false;
        goto(&mut a, 2);
        key(&mut a, KeyCode::Char('r'));
        type_text(&mut a, "Agreed");
        let cmds = key(&mut a, KeyCode::Enter);
        let Some(Cmd::Reply { thread, .. }) = cmds.first().cloned() else {
            panic!("expected a Reply");
        };
        update(
            &mut a,
            Msg::ReplyPosted {
                id: id(),
                thread,
                result: Err("refused".into()),
                demo: false,
            },
        );
        assert_eq!(diff(&a).composer.as_ref().unwrap().editor.text(), "Agreed");
        assert!(toast(&a).contains("Couldn't post your reply: refused."));
    }

    #[test]
    fn r_without_a_thread_explains_itself() {
        let mut a = app();
        goto(&mut a, 3);
        key(&mut a, KeyCode::Char('r'));
        assert!(diff(&a).composer.is_none());
        assert!(status(&a).contains("Press c to comment instead"));
    }

    #[test]
    fn a_previews_the_approval_with_the_pending_comments() {
        let mut a = app_with(
            vec![draft(3, "First"), draft(4, "Second\nmore")],
            Capabilities::all(),
        );
        key(&mut a, KeyCode::Char('a'));
        let confirm = diff(&a).confirm.clone().expect("a preview");
        let ConfirmKind::Approve {
            summary,
            comments,
            verdict,
        } = &confirm.kind
        else {
            panic!("expected an approve preview");
        };
        assert_eq!(summary, "Approve with 2 comments");
        assert_eq!(*verdict, Verdict::Approve);
        assert_eq!(comments, &["a.rs line 3 · First", "a.rs line 4 · Second"]);
        assert!(confirm.yes);
        assert_eq!(confirm.title(), "Approve this change?");
    }

    #[test]
    fn approving_sends_the_draft_then_clears_it_on_success() {
        let mut a = app_with(vec![draft(3, "First")], Capabilities::all());
        key(&mut a, KeyCode::Char('a'));
        let cmds = key(&mut a, KeyCode::Enter);
        let (sent, verdict) = submit_cmd(&cmds);
        assert_eq!((verdict, sent.comments.len()), (Verdict::Approve, 1));
        assert!(diff(&a).submitting);

        key(&mut a, KeyCode::Char('a'));
        assert!(status(&a).starts_with("Still sending"));
        assert!(diff(&a).confirm.is_none());

        let cmds = submitted(&mut a, Verdict::Approve, Ok(()));
        assert!(cmds.iter().any(|c| matches!(c, Cmd::LoadChanges)));
        assert_eq!(toast(&a), "Approved (demo)");
        assert_eq!(a.toasts.last().unwrap().notice.kind, NoticeKind::Success);
        let s = diff(&a);
        assert!(!s.submitting && s.data.as_ref().unwrap().draft.is_empty());
        assert!(!a.has_unsent_drafts());
    }

    #[test]
    fn outside_demo_the_toast_has_no_suffix() {
        let mut a = app();
        key(&mut a, KeyCode::Char('a'));
        key(&mut a, KeyCode::Enter);
        update(
            &mut a,
            Msg::ReviewSubmitted {
                id: id(),
                verdict: Verdict::Approve,
                result: Ok(()),
                demo: false,
            },
        );
        assert_eq!(toast(&a), "Approved");
    }

    #[test]
    fn a_failed_approval_keeps_the_draft_and_says_what_to_do_next() {
        let mut a = app_with(vec![draft(3, "First")], Capabilities::all());
        key(&mut a, KeyCode::Char('a'));
        key(&mut a, KeyCode::Enter);
        let cmds = submitted(
            &mut a,
            Verdict::Approve,
            Err("GitHub refused the request.".into()),
        );
        assert!(!cmds.iter().any(|c| matches!(c, Cmd::LoadChanges)));
        let s = diff(&a);
        assert!(!s.submitting);
        assert_eq!(s.data.as_ref().unwrap().draft.comments.len(), 1);
        assert_eq!(
            toast(&a),
            "Couldn't submit your review: GitHub refused the request. Your comments are still pending. Press a to try again."
        );
        key(&mut a, KeyCode::Char('a'));
        assert!(diff(&a).confirm.is_some(), "can try again");
    }

    #[test]
    fn cancelling_the_approval_sends_nothing() {
        let mut a = app();
        key(&mut a, KeyCode::Char('a'));
        assert!(matches!(
            &diff(&a).confirm.as_ref().unwrap().kind,
            ConfirmKind::Approve { summary, .. } if summary == "Approve"
        ));
        key(&mut a, KeyCode::Tab);
        assert!(!diff(&a).confirm.as_ref().unwrap().yes);
        assert!(
            key(&mut a, KeyCode::Enter).is_empty(),
            "the Cancel button is focused"
        );
        assert!(diff(&a).confirm.is_none() && !diff(&a).submitting);
        key(&mut a, KeyCode::Char('a'));
        assert!(key(&mut a, KeyCode::Esc).is_empty());
        assert!(diff(&a).confirm.is_none());
    }

    #[test]
    fn an_invalid_draft_is_explained_before_the_preview() {
        let mut a = app_with(vec![draft(3, "  ")], Capabilities::all());
        key(&mut a, KeyCode::Char('a'));
        assert!(diff(&a).confirm.is_none());
        let text = status(&a);
        assert!(
            text.contains("is empty") && !text.contains("unexpected response"),
            "{text}"
        );
    }

    #[test]
    fn x_explains_when_the_forge_cannot_request_changes() {
        let caps = Capabilities {
            request_changes: false,
            ..Capabilities::all()
        };
        let mut a = app_with(Vec::new(), caps);
        key(&mut a, KeyCode::Char('x'));
        assert_eq!(
            status(&a),
            "GitHub doesn't support request changes. Leave a comment instead (c)."
        );
        let mut a = app();
        key(&mut a, KeyCode::Char('x'));
        assert!(status(&a).contains("Approve with a"));
    }

    #[test]
    fn quitting_with_text_in_the_composer_asks_first() {
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        type_text(&mut a, "half a thought");
        with(&mut a, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(!a.should_quit());
        assert!(status(&a).contains("unsent review comments"));
        with(&mut a, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(a.should_quit());
    }

    #[test]
    fn quitting_with_pending_comments_asks_first() {
        let mut a = app_with(vec![draft(3, "x")], Capabilities::all());
        assert!(a.has_unsent_drafts());
        with(&mut a, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(!a.should_quit());
    }

    #[test]
    fn mouse_clicks_do_nothing_while_an_overlay_is_open() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mut a = app();
        key(&mut a, KeyCode::Char('c'));
        a.hits.push(
            ratatui::layout::Rect::new(0, 0, 200, 50),
            crate::app::Action::CloseDiff,
        );
        update(
            &mut a,
            Msg::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 5,
                modifiers: KeyModifiers::NONE,
            }),
        );
        assert_eq!(a.screen, Screen::Diff);
    }

    #[test]
    fn the_cursor_line_stays_above_the_docked_composer() {
        let mut big = String::from("@@ -1,200 +1,200 @@\n");
        for i in 0..200 {
            big.push_str(&format!(" l{i}\n"));
        }
        let mut a = app();
        let data = DiffData::new(
            vec![FilePatch {
                path: "src/big.rs".into(),
                old_path: None,
                status: FileStatus::Modified,
                adds: 0,
                dels: 0,
                patch: Some(big),
            }],
            Vec::new(),
            ReviewDraft::default(),
        );
        update(
            &mut a,
            Msg::DiffLoaded {
                id: id(),
                result: Ok(Box::new(data)),
            },
        );
        for _ in 0..33 {
            key(&mut a, KeyCode::Char('j'));
        }
        key(&mut a, KeyCode::Char('c'));
        let code = viewport(&a).code;
        let s = diff(&a);
        let dock = layout::composer_dock(code, 1);
        let row_on_screen = s.view.cursor - s.view.scroll;
        assert!(
            row_on_screen < usize::from(code.height - dock.height),
            "{row_on_screen}"
        );
    }
}
