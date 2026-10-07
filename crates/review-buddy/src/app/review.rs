//! The review modal: pick a verdict, write an optional summary, see what will be sent, submit.
//! Pure state and transitions; the write leaves as a [`Cmd::SubmitReview`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_core::{FeatureAction, MyReview, ReviewDraft, Verdict};
use rb_github::plan_review;

use super::composer::{forge_name, info, plan_message, ready, warn};
use super::diff::DiffState;
use super::editor::Editor;
use super::{App, Cmd};

/// What has keyboard focus inside the modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewFocus {
    Verdict,
    Summary,
    Cancel,
    Submit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewModal {
    pub verdict: Verdict,
    /// The verdicts this source offers, in the order `1`/`2`/`3` pick them.
    pub verdicts: Vec<Verdict>,
    pub summary: Editor,
    pub focus: ReviewFocus,
    /// Why the last submit failed, shown under the buttons until the next keypress.
    pub error: Option<String>,
    /// The button a mouse press is holding: `false` is Cancel, `true` is Submit.
    pub armed: Option<bool>,
}

pub fn verdict_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Comment => "Comment",
        Verdict::Approve => "Approve",
        Verdict::RequestChanges => "Request changes",
    }
}

/// The confirming button's label: what pressing it does.
pub fn submit_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Comment => "Post review",
        Verdict::Approve => "Approve",
        Verdict::RequestChanges => "Request changes",
    }
}

/// The line under the buttons: the keys that work from `focus`, honest about whether this
/// terminal can send `⌃⏎`.
pub fn key_hint(focus: ReviewFocus, kitty_keys: bool) -> &'static str {
    match (focus, kitty_keys) {
        (ReviewFocus::Summary, true) => "⌃⏎ or ⌃P submits · tab → buttons · esc cancels",
        (ReviewFocus::Summary, false) => "tab then ⏎ submits · ⌃P submits now · esc cancels",
        (ReviewFocus::Submit, _) => "⏎ submits · tab → cancel · esc cancels",
        (ReviewFocus::Cancel, _) => "⏎ cancels · tab → verdict · ⌃P submits · esc cancels",
        (ReviewFocus::Verdict, _) => "← → choose · tab → summary · ⌃P submits · esc cancels",
    }
}

pub fn my_review_of(verdict: Verdict) -> MyReview {
    match verdict {
        Verdict::Approve => MyReview::Approved,
        Verdict::RequestChanges => MyReview::ChangesRequested,
        Verdict::Comment => MyReview::Commented,
    }
}

/// Why this verdict can't be submitted yet, in a calm sentence.
pub fn problem(verdict: Verdict, summary_blank: bool, pending: usize) -> Option<&'static str> {
    match verdict {
        Verdict::RequestChanges if summary_blank => {
            Some("Requesting changes needs a short summary. Add one above.")
        }
        Verdict::Comment if summary_blank && pending == 0 => {
            Some("Add a summary or a pending comment to post a review.")
        }
        _ => None,
    }
}

impl ReviewModal {
    fn new(verdicts: Vec<Verdict>, verdict: Verdict, summary: &str, pending: usize) -> Self {
        let summary = Editor::with_text(summary);
        let blocked = problem(verdict, summary.is_blank(), pending).is_some();
        let focus = match verdict {
            _ if blocked => ReviewFocus::Summary,
            Verdict::RequestChanges => ReviewFocus::Cancel,
            _ => ReviewFocus::Submit,
        };
        Self {
            verdict,
            verdicts,
            summary,
            focus,
            error: None,
            armed: None,
        }
    }

    pub fn problem(&self, pending: usize) -> Option<&'static str> {
        problem(self.verdict, self.summary.is_blank(), pending)
    }

    /// The draft as it would be sent: the pending comments plus the summary as the body.
    pub fn draft(&self, pending: &ReviewDraft) -> ReviewDraft {
        ReviewDraft {
            body: self.summary.text().trim().to_string(),
            comments: pending.comments.clone(),
        }
    }

    /// The one-line preview from `plan_review`. A summary that is still missing doesn't hide it.
    pub fn preview(&self, pending: &ReviewDraft) -> String {
        let mut draft = self.draft(pending);
        if draft.body.is_empty() {
            draft.body = "-".to_string();
        }
        match plan_review(&draft, self.verdict) {
            Ok(plan) => plan.summary(),
            Err(err) => plan_message(&err),
        }
    }
}

fn pending_count(app: &App) -> usize {
    app.diff
        .as_ref()
        .and_then(|s| s.data.as_ref())
        .map_or(0, |d| d.draft.comments.len())
}

fn modal(app: &mut App) -> Option<&mut ReviewModal> {
    app.diff.as_mut()?.review.as_mut()
}

/// The verdicts on offer. The capability probe has the final say once it has answered; until
/// then the provider's own capabilities decide.
fn offered(app: &App, state: &DiffState) -> Vec<Verdict> {
    let can_request = match app.state.probes.get(&state.id.source_id) {
        Some(probe) => probe
            .outcome
            .capabilities
            .supports(FeatureAction::RequestChanges),
        None => state
            .data
            .as_ref()
            .is_none_or(|d| d.caps.supports(FeatureAction::RequestChanges)),
    };
    let mut all = vec![Verdict::Comment, Verdict::Approve];
    if can_request {
        all.push(Verdict::RequestChanges);
    }
    all
}

/// `a`, `x` and `R`: opens the modal with `verdict` selected. A verdict the source can't do is
/// explained in one line instead.
pub fn open(app: &mut App, verdict: Verdict) -> Vec<Cmd> {
    if !ready(app) {
        return Vec::new();
    }
    if app.diff.as_ref().is_some_and(|s| s.submitting) {
        return info(app, "Still sending. One moment.");
    }
    let Some(state) = app.diff.as_ref() else {
        return Vec::new();
    };
    let verdicts = offered(app, state);
    if !verdicts.contains(&verdict) {
        return explain_request_changes(app);
    }
    let Some(data) = state.data.as_ref() else {
        return Vec::new();
    };
    let mut check = data.draft.clone();
    check.body = "-".to_string();
    if let Err(err) = plan_review(&check, Verdict::Comment) {
        return warn(app, plan_message(&err));
    }
    let modal = ReviewModal::new(
        verdicts,
        verdict,
        &data.draft.body,
        data.draft.comments.len(),
    );
    if let Some(s) = app.diff.as_mut() {
        s.review = Some(modal);
    }
    app.mark_dirty();
    Vec::new()
}

fn explain_request_changes(app: &mut App) -> Vec<Cmd> {
    let Some(state) = app.diff.as_ref() else {
        return Vec::new();
    };
    let source = state.id.source_id.clone();
    let kind = state.id.kind;
    let caps = state.data.as_ref().map(|d| d.caps);
    let text = app
        .state
        .explain_unsupported(&source, FeatureAction::RequestChanges)
        .or_else(|| caps?.explain_unsupported(FeatureAction::RequestChanges, forge_name(kind)))
        .unwrap_or_else(|| {
            format!(
                "{} can't request changes here. Approve with a, or post a review with R.",
                forge_name(kind)
            )
        });
    info(app, text)
}

pub fn on_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    if app.diff.as_ref().is_some_and(|s| s.submitting) {
        return Vec::new();
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let Some(focus) = modal(app).map(|m| m.focus) else {
        return Vec::new();
    };
    if let Some(m) = modal(app) {
        if m.error.take().is_some() {
            app.mark_dirty();
        }
    }
    match key.code {
        KeyCode::Esc => return close(app),
        KeyCode::Enter if ctrl || alt => return submit(app),
        KeyCode::Char('p') if ctrl => return submit(app),
        _ => {}
    }
    let handled = match focus {
        ReviewFocus::Summary => summary_key(app, key, ctrl, alt),
        ReviewFocus::Verdict => verdict_key(app, key, ctrl || alt),
        ReviewFocus::Cancel | ReviewFocus::Submit => {
            if ctrl || alt {
                false
            } else {
                return button_key(app, key, focus);
            }
        }
    };
    if handled {
        app.mark_dirty();
    }
    Vec::new()
}

fn pick_number(app: &mut App, c: char) {
    let index = usize::from(c as u8 - b'1');
    let Some(verdict) = modal(app).and_then(|m| m.verdicts.get(index).copied()) else {
        return;
    };
    set_verdict(app, verdict);
}

/// Changes the verdict. Landing on one that asks for changes moves focus off Submit, so ⏎
/// can't send it by accident.
pub fn set_verdict(app: &mut App, verdict: Verdict) {
    let Some(m) = modal(app) else {
        return;
    };
    if !m.verdicts.contains(&verdict) {
        return;
    }
    m.verdict = verdict;
    if verdict == Verdict::RequestChanges && m.focus == ReviewFocus::Submit {
        m.focus = ReviewFocus::Cancel;
    }
    app.mark_dirty();
}

fn cycle(app: &mut App, forward: bool) {
    let Some(m) = modal(app) else {
        return;
    };
    let at = m.verdicts.iter().position(|v| *v == m.verdict).unwrap_or(0);
    let len = m.verdicts.len();
    let next = if forward {
        (at + 1) % len
    } else {
        (at + len - 1) % len
    };
    let verdict = m.verdicts[next];
    set_verdict(app, verdict);
}

fn verdict_key(app: &mut App, key: KeyEvent, modified: bool) -> bool {
    if modified {
        return false;
    }
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => cycle(app, false),
        KeyCode::Right | KeyCode::Char('l') => cycle(app, true),
        KeyCode::Tab => focus(app, ReviewFocus::Summary),
        KeyCode::BackTab => focus(app, ReviewFocus::Cancel),
        KeyCode::Char(c @ '1'..='3') => pick_number(app, c),
        KeyCode::Down | KeyCode::Enter | KeyCode::Char('j') => focus(app, ReviewFocus::Summary),
        _ => return false,
    }
    true
}

fn button_key(app: &mut App, key: KeyEvent, at: ReviewFocus) -> Vec<Cmd> {
    match key.code {
        KeyCode::Enter => {
            return if at == ReviewFocus::Submit {
                submit(app)
            } else {
                close(app)
            };
        }
        KeyCode::Char('n' | 'N') => return close(app),
        KeyCode::Tab => {
            let next = if at == ReviewFocus::Submit {
                ReviewFocus::Cancel
            } else {
                ReviewFocus::Verdict
            };
            focus(app, next);
        }
        KeyCode::BackTab => {
            let prev = if at == ReviewFocus::Submit {
                ReviewFocus::Summary
            } else {
                ReviewFocus::Submit
            };
            focus(app, prev);
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l') => {
            let other = if at == ReviewFocus::Submit {
                ReviewFocus::Cancel
            } else {
                ReviewFocus::Submit
            };
            focus(app, other);
        }
        KeyCode::Up | KeyCode::Char('k' | 'e') => focus(app, ReviewFocus::Summary),
        KeyCode::Char(c @ '1'..='3') => pick_number(app, c),
        _ => return Vec::new(),
    }
    app.mark_dirty();
    Vec::new()
}

fn focus(app: &mut App, to: ReviewFocus) {
    if let Some(m) = modal(app) {
        m.focus = to;
    }
}

fn summary_key(app: &mut App, key: KeyEvent, ctrl: bool, alt: bool) -> bool {
    let Some(m) = modal(app) else {
        return false;
    };
    let word = ctrl || alt;
    let last = m.summary.line_count() - 1;
    let (row, _) = m.summary.cursor();
    let e = &mut m.summary;
    match key.code {
        KeyCode::Tab => m.focus = ReviewFocus::Submit,
        KeyCode::BackTab => m.focus = ReviewFocus::Verdict,
        KeyCode::Up if row == 0 && !word => m.focus = ReviewFocus::Verdict,
        KeyCode::Down if row == last && !word => m.focus = ReviewFocus::Cancel,
        KeyCode::Enter => e.newline(),
        KeyCode::Char('j') if ctrl => e.newline(),
        KeyCode::Char('b') if alt => e.word_left(),
        KeyCode::Char('f') if alt => e.word_right(),
        KeyCode::Char('w') if ctrl => e.delete_word(),
        KeyCode::Backspace if word => e.delete_word(),
        KeyCode::Left if word => e.word_left(),
        KeyCode::Right if word => e.word_right(),
        KeyCode::Home if ctrl => e.start(),
        KeyCode::End if ctrl => e.finish(),
        _ if word => return false,
        KeyCode::Char(c) => e.insert_char(c),
        KeyCode::Backspace => e.backspace(),
        KeyCode::Delete => e.delete(),
        KeyCode::Left => e.left(),
        KeyCode::Right => e.right(),
        KeyCode::Up => e.up(),
        KeyCode::Down => e.down(),
        KeyCode::Home => e.home(),
        KeyCode::End => e.end(),
        _ => return false,
    }
    true
}

/// Bracketed paste lands in the summary when it has focus.
pub fn on_paste(app: &mut App, text: &str) {
    if app.diff.as_ref().is_some_and(|s| s.submitting) {
        return;
    }
    if let Some(m) = modal(app).filter(|m| m.focus == ReviewFocus::Summary) {
        m.summary.insert_str(text);
        app.mark_dirty();
    }
}

/// Closes the modal, keeping the summary with the pending review so nothing is lost.
pub fn close(app: &mut App) -> Vec<Cmd> {
    let Some(s) = app.diff.as_mut() else {
        return Vec::new();
    };
    if s.submitting {
        return Vec::new();
    }
    if let Some(m) = s.review.take() {
        if let Some(data) = s.data.as_mut() {
            data.draft.body = m.summary.text().trim().to_string();
        }
    }
    app.mark_dirty();
    Vec::new()
}

/// A mouse press on a button arms it; releasing on the same button fires it.
pub fn arm(app: &mut App, submit_button: bool) {
    if let Some(m) = modal(app) {
        m.armed = Some(submit_button);
    }
}

/// A release over `over` (a button, or nothing). It fires the armed button when it is the
/// same one, and also when nothing was armed, for terminals that only report the release.
pub fn release(app: &mut App, over: Option<bool>) -> Vec<Cmd> {
    let armed = modal(app).and_then(|m| m.armed.take());
    match (armed, over) {
        (Some(a), Some(b)) if a == b => press(app, b),
        (None, Some(b)) => press(app, b),
        _ => Vec::new(),
    }
}

/// A click on a button: Cancel closes, Submit sends.
pub fn press(app: &mut App, submit_button: bool) -> Vec<Cmd> {
    if app.diff.as_ref().is_some_and(|s| s.submitting) {
        return Vec::new();
    }
    if let Some(m) = modal(app) {
        m.error = None;
    }
    if submit_button {
        submit(app)
    } else {
        close(app)
    }
}

/// A click on the summary: focuses it and puts the caret under the pointer.
pub fn place_cursor(app: &mut App, column: u16, row: u16, at: (u16, u16, usize, usize)) {
    let (x, y, first, across) = at;
    if app.diff.as_ref().is_some_and(|s| s.submitting) {
        return;
    }
    let Some(m) = modal(app) else {
        return;
    };
    m.focus = ReviewFocus::Summary;
    let target = (first + usize::from(row.saturating_sub(y))).min(m.summary.line_count() - 1);
    let cells = usize::from(column.saturating_sub(x));
    let col = super::composer::char_at(&m.summary.lines()[target], across, cells);
    m.summary.set_cursor(target, col);
    app.mark_dirty();
}

pub fn scroll_text(app: &mut App, down: bool) {
    let Some(m) = modal(app) else {
        return;
    };
    if down {
        m.summary.down();
    } else {
        m.summary.up();
    }
    app.mark_dirty();
}

fn submit(app: &mut App) -> Vec<Cmd> {
    let pending = pending_count(app);
    let Some(s) = app.diff.as_ref() else {
        return Vec::new();
    };
    let (Some(m), Some(data)) = (s.review.as_ref(), s.data.as_ref()) else {
        return Vec::new();
    };
    if let Some(text) = m.problem(pending) {
        return warn(app, text);
    }
    let draft = m.draft(&data.draft);
    let verdict = m.verdict;
    if let Err(err) = plan_review(&draft, verdict) {
        return warn(app, plan_message(&err));
    }
    let id = s.id.clone();
    if let Some(s) = app.diff.as_mut() {
        s.submitting = true;
    }
    app.mark_dirty();
    vec![Cmd::SubmitReview { id, draft, verdict }]
}

/// Keeps the modal open after a failed submit: focus on Submit and the reason under the buttons.
pub fn show_failure(app: &mut App, message: String) {
    if let Some(m) = modal(app) {
        m.error = Some(message);
        m.focus = ReviewFocus::Submit;
        m.armed = None;
    }
    app.mark_dirty();
}
