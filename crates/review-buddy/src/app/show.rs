//! The Show filters control: which roles, drafts and Noise the queue lets in. Changes apply to
//! the queue at once and last for the session; the configured `triage.show` is the starting point.

use crossterm::event::{KeyCode, KeyEvent};

use super::{dashboard, App, Cmd};
use crate::config::ShowFilter;

/// The filters in the order the control lists them.
pub const FILTERS: [ShowFilter; 5] = [
    ShowFilter::Reviewing,
    ShowFilter::Assigned,
    ShowFilter::Authored,
    ShowFilter::Drafts,
    ShowFilter::Noise,
];

/// What the control says about a filter, beside its checkbox.
pub fn describe(filter: ShowFilter) -> &'static str {
    match filter {
        ShowFilter::Reviewing => "changes waiting on your review",
        ShowFilter::Assigned => "changes assigned to you",
        ShowFilter::Authored => "changes you opened",
        ShowFilter::Drafts => "drafts, yours and others",
        ShowFilter::Noise => "bot updates, mixed into the buckets",
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShowControl {
    pub open: bool,
    pub cursor: usize,
}

pub fn is_on(app: &App, filter: ShowFilter) -> bool {
    app.state.queue_settings.show.contains(&filter)
}

pub fn open(app: &mut App) {
    app.show.open = true;
    app.mark_dirty();
}

pub fn close(app: &mut App) {
    app.show.open = false;
    app.mark_dirty();
}

pub fn toggle(app: &mut App, filter: ShowFilter) {
    if let Some(at) = FILTERS.iter().position(|f| *f == filter) {
        app.show.cursor = at;
    }
    let show = &mut app.state.queue_settings.show;
    match show.iter().position(|f| *f == filter) {
        Some(at) => {
            show.remove(at);
        }
        None => show.push(filter),
    }
    dashboard::reconcile(app);
    app.mark_dirty();
}

pub fn on_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    let last = FILTERS.len() - 1;
    match key.code {
        KeyCode::Esc | KeyCode::Char('s') | KeyCode::Char('q') => close(app),
        KeyCode::Down | KeyCode::Char('j') => app.show.cursor = (app.show.cursor + 1).min(last),
        KeyCode::Up | KeyCode::Char('k') => app.show.cursor = app.show.cursor.saturating_sub(1),
        KeyCode::Char('g') | KeyCode::Home => app.show.cursor = 0,
        KeyCode::Char('G') | KeyCode::End => app.show.cursor = last,
        KeyCode::Char(' ') | KeyCode::Enter => toggle(app, FILTERS[app.show.cursor.min(last)]),
        KeyCode::Char(c @ '1'..='5') => {
            let at = usize::from(c as u8 - b'1');
            app.show.cursor = at;
            toggle(app, FILTERS[at]);
        }
        _ => {}
    }
    app.mark_dirty();
    Vec::new()
}
