//! The Show filters control: which roles, drafts and Noise the queue lets in, and which projects
//! it shows. Changes apply to the queue at once and last for the session; the configured
//! `triage.show` and each source's `hide_repos` are the starting point, and `w` saves the
//! project choice back to the config file.

use crossterm::event::{KeyCode, KeyEvent};
use rb_core::SourceId;

use super::projects::{self, Project};
use super::{dashboard, update, App, Cmd, Notice, NoticeKind};
use crate::config::ShowFilter;

/// The filters in the order the control lists them.
pub const FILTERS: [ShowFilter; 5] = [
    ShowFilter::Reviewing,
    ShowFilter::Assigned,
    ShowFilter::Authored,
    ShowFilter::Drafts,
    ShowFilter::Noise,
];

/// Lines of the overlay that aren't project rows: the border, the five kinds and their gaps, the
/// Projects heading and search line, the note and the hints.
const FIXED: u16 = 15;

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

/// The control's cursor runs over the kinds first, then over the projects the search lets
/// through. `scroll` is the first line of the project list on screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShowControl {
    pub open: bool,
    pub cursor: usize,
    pub scroll: usize,
    pub search: String,
    /// Typing goes to the search field.
    pub searching: bool,
}

/// One line of the project list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListRow {
    Heading {
        label: String,
        projects: usize,
        changes: usize,
    },
    Project {
        source: SourceId,
        repo: String,
        count: usize,
    },
}

/// The project list the search lets through, grouped under its source headings.
pub fn list_rows(app: &App, search: &str) -> Vec<ListRow> {
    let needle = search.trim().to_lowercase();
    let mut rows = Vec::new();
    for group in projects::catalog(&app.state) {
        let kept: Vec<_> = group
            .projects
            .iter()
            .filter(|p| needle.is_empty() || p.repo.to_lowercase().contains(&needle))
            .collect();
        if kept.is_empty() {
            continue;
        }
        rows.push(ListRow::Heading {
            label: group.label.clone(),
            projects: kept.len(),
            changes: kept.iter().map(|p| p.count).sum(),
        });
        rows.extend(kept.into_iter().map(|p| ListRow::Project {
            source: group.source.clone(),
            repo: p.repo.clone(),
            count: p.count,
        }));
    }
    rows
}

/// The projects on screen for the current search, in order.
pub fn matches(app: &App) -> Vec<Project> {
    list_rows(app, &app.show.search)
        .into_iter()
        .filter_map(|row| match row {
            ListRow::Project { source, repo, .. } => Some((source, repo)),
            ListRow::Heading { .. } => None,
        })
        .collect()
}

/// How big the overlay is: its height, and how many project lines it shows. The size follows
/// the whole project list, not the search, so it doesn't jump as you type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    pub height: u16,
    pub list: usize,
}

pub fn metrics(app: &App) -> Metrics {
    let body = app.size.1.saturating_sub(2);
    let lines = list_rows(app, "").len().max(1) as u16;
    let height = (FIXED + lines).min(body);
    Metrics {
        height,
        list: usize::from(height.saturating_sub(FIXED)).max(1),
    }
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
    app.show.searching = false;
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

/// Ticks or clears one project, moving the cursor onto it.
pub fn toggle_project(app: &mut App, source: &SourceId, repo: &str) {
    if let Some(at) = matches(app)
        .iter()
        .position(|(s, r)| s == source && r == repo)
    {
        app.show.cursor = FILTERS.len() + at;
        ensure_visible(app);
    }
    app.state.queue_settings.projects.toggle(source, repo);
    dashboard::reconcile(app);
    app.mark_dirty();
}

/// Ticks (`true`) or clears (`false`) every project the search lets through.
pub fn set_visible_projects(app: &mut App, shown: bool) {
    let rows = matches(app);
    app.state.queue_settings.projects.set_all(&rows, shown);
    dashboard::reconcile(app);
    app.mark_dirty();
}

pub fn focus_search(app: &mut App) {
    app.show.searching = true;
    app.mark_dirty();
}

pub fn scroll(app: &mut App, down: bool) {
    let lines = list_rows(app, &app.show.search).len();
    let max = lines.saturating_sub(metrics(app).list);
    app.show.scroll = if down {
        (app.show.scroll + 3).min(max)
    } else {
        app.show.scroll.saturating_sub(3)
    };
    app.mark_dirty();
}

fn last_cursor(app: &App) -> usize {
    FILTERS.len() - 1 + matches(app).len()
}

/// Scrolls the project list just enough to show the cursor's row, and its heading when the
/// cursor is on the first project of a group.
fn ensure_visible(app: &mut App) {
    let list = metrics(app).list;
    let rows = list_rows(app, &app.show.search);
    let max = rows.len().saturating_sub(list);
    app.show.scroll = app.show.scroll.min(max);
    let Some(project) = app.show.cursor.checked_sub(FILTERS.len()) else {
        app.show.scroll = 0;
        return;
    };
    let Some(line) = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r, ListRow::Project { .. }))
        .nth(project)
        .map(|(i, _)| i)
    else {
        return;
    };
    let line_top = if line > 0 && matches!(rows[line - 1], ListRow::Heading { .. }) {
        line - 1
    } else {
        line
    };
    if line_top < app.show.scroll {
        app.show.scroll = line_top;
    } else if line >= app.show.scroll + list {
        app.show.scroll = line + 1 - list;
    }
}

fn move_cursor(app: &mut App, to: usize) {
    app.show.cursor = to.min(last_cursor(app));
    ensure_visible(app);
}

fn clamp_after_search(app: &mut App) {
    app.show.scroll = 0;
    app.show.cursor = app.show.cursor.min(last_cursor(app));
    ensure_visible(app);
}

fn act_on_cursor(app: &mut App) {
    match app.show.cursor.checked_sub(FILTERS.len()) {
        None => toggle(app, FILTERS[app.show.cursor.min(FILTERS.len() - 1)]),
        Some(at) => {
            if let Some((source, repo)) = matches(app).into_iter().nth(at) {
                toggle_project(app, &source, &repo);
            }
        }
    }
}

pub fn can_save(app: &App) -> bool {
    app.demo || app.project_save.is_some()
}

fn save(app: &mut App) -> Vec<Cmd> {
    if app.demo {
        return update::push_toast(
            app,
            Notice::new(
                NoticeKind::Info,
                "Demo mode doesn't write your config, so nothing was saved (demo).",
            ),
        );
    }
    let Some(target) = app.project_save.clone() else {
        return update::push_toast(
            app,
            Notice::new(
                NoticeKind::Info,
                "There's no config file to save to. Run review-buddy --setup to create one.",
            ),
        );
    };
    let groups = projects::catalog(&app.state);
    let filter = &app.state.queue_settings.projects;
    let sources = app
        .state
        .sources
        .iter()
        .map(|s| {
            let known: Vec<String> = groups
                .iter()
                .filter(|g| g.source == s.id)
                .flat_map(|g| g.projects.iter().map(|p| p.repo.clone()))
                .collect();
            (s.id.to_string(), filter.entries_for(&s.id, &known))
        })
        .collect();
    vec![Cmd::SaveHiddenProjects { target, sources }]
}

pub fn on_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    let mut cmds = Vec::new();
    if app.show.searching {
        on_search_key(app, key);
    } else {
        cmds = on_list_key(app, key);
    }
    app.mark_dirty();
    cmds
}

fn on_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.show.search.clear();
            app.show.searching = false;
            clamp_after_search(app);
        }
        KeyCode::Enter => app.show.searching = false,
        KeyCode::Backspace => {
            app.show.search.pop();
            clamp_after_search(app);
        }
        KeyCode::Down => move_cursor(app, app.show.cursor + 1),
        KeyCode::Up => move_cursor(app, app.show.cursor.saturating_sub(1)),
        KeyCode::Char(c) => {
            app.show.search.push(c);
            app.show.cursor = app.show.cursor.max(FILTERS.len());
            clamp_after_search(app);
        }
        _ => {}
    }
}

fn on_list_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    let page = metrics(app).list;
    match key.code {
        KeyCode::Esc if !app.show.search.is_empty() => {
            app.show.search.clear();
            clamp_after_search(app);
        }
        KeyCode::Esc | KeyCode::Char('s') | KeyCode::Char('q') => close(app),
        KeyCode::Char('/') => {
            focus_search(app);
            app.show.cursor = app.show.cursor.max(FILTERS.len().min(last_cursor(app)));
            ensure_visible(app);
        }
        KeyCode::Down | KeyCode::Char('j') => move_cursor(app, app.show.cursor + 1),
        KeyCode::Up | KeyCode::Char('k') => move_cursor(app, app.show.cursor.saturating_sub(1)),
        KeyCode::PageDown => move_cursor(app, app.show.cursor + page),
        KeyCode::PageUp => move_cursor(app, app.show.cursor.saturating_sub(page)),
        KeyCode::Char('g') | KeyCode::Home => move_cursor(app, 0),
        KeyCode::Char('G') | KeyCode::End => move_cursor(app, last_cursor(app)),
        KeyCode::Char(' ') | KeyCode::Enter => act_on_cursor(app),
        KeyCode::Char('a') => set_visible_projects(app, true),
        KeyCode::Char('n') => set_visible_projects(app, false),
        KeyCode::Char('w') => return save(app),
        KeyCode::Char(c @ '1'..='5') => {
            let at = usize::from(c as u8 - b'1');
            app.show.cursor = at;
            toggle(app, FILTERS[at]);
        }
        _ => {}
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::queue::tests::change;
    use crate::app::{update, AppConfig, Msg};
    use crossterm::event::KeyModifiers;
    use rb_core::MyRole;
    use rb_theme::ColourDepth;

    fn app_with(repos: &[(&str, &str)]) -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (100, 30),
        });
        for (i, (source, repo)) in repos.iter().enumerate() {
            let mut c = change(i as u64 + 1, MyRole::Reviewing, 10 + i as i64);
            c.id.source_id = SourceId::new(*source);
            c.id.repo = (*repo).to_string();
            app.state.changes.push(c);
        }
        app.state.loaded = true;
        app.show.open = true;
        app
    }

    fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
        update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    fn hidden(app: &App, source: &str, repo: &str) -> bool {
        app.state
            .queue_settings
            .projects
            .is_hidden(&SourceId::new(source), repo)
    }

    const REPOS: &[(&str, &str)] = &[
        ("s1", "o/alpha"),
        ("s1", "o/beta"),
        ("s1", "o/alphabet"),
        ("s2", "g/sub/gamma"),
    ];

    #[test]
    fn space_ticks_the_project_under_the_cursor_and_the_queue_follows() {
        let mut app = app_with(REPOS);
        assert_eq!(crate::app::queue::count_in(&app.state, None), 4);
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.show.cursor, 5 + 3);
        press(&mut app, KeyCode::Char(' '));
        assert!(hidden(&app, "s2", "g/sub/gamma"));
        assert_eq!(crate::app::queue::count_in(&app.state, None), 3);
        let split = crate::app::queue::hidden_split(&app.state, None);
        assert_eq!((split.by_kind, split.by_project), (0, 1));
        press(&mut app, KeyCode::Enter);
        assert!(!hidden(&app, "s2", "g/sub/gamma"));
    }

    #[test]
    fn a_and_n_apply_to_the_projects_the_search_shows() {
        let mut app = app_with(REPOS);
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "alpha");
        press(&mut app, KeyCode::Enter);
        assert_eq!(matches(&app).len(), 2);
        press(&mut app, KeyCode::Char('n'));
        assert!(hidden(&app, "s1", "o/alpha") && hidden(&app, "s1", "o/alphabet"));
        assert!(!hidden(&app, "s1", "o/beta") && !hidden(&app, "s2", "g/sub/gamma"));
        press(&mut app, KeyCode::Char('a'));
        assert!(!hidden(&app, "s1", "o/alpha"));
    }

    #[test]
    fn typing_into_the_search_does_not_trigger_keys() {
        let mut app = app_with(REPOS);
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "an");
        assert_eq!(app.show.search, "an");
        assert!(app.show.open);
        assert!(app.state.queue_settings.projects.is_clear());
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.show.search, "a");
    }

    #[test]
    fn escape_clears_the_search_before_it_closes() {
        let mut app = app_with(REPOS);
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "bet");
        press(&mut app, KeyCode::Enter);
        assert!(!app.show.searching && app.show.search == "bet");
        press(&mut app, KeyCode::Esc);
        assert!(app.show.open && app.show.search.is_empty());
        press(&mut app, KeyCode::Esc);
        assert!(!app.show.open);

        let mut app = app_with(REPOS);
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "x");
        press(&mut app, KeyCode::Esc);
        assert!(app.show.open && app.show.search.is_empty() && !app.show.searching);
        press(&mut app, KeyCode::Esc);
        assert!(!app.show.open);
    }

    #[test]
    fn the_list_scrolls_to_keep_the_cursor_in_view() {
        let repos: Vec<(String, String)> = (0..40)
            .map(|i| ("s1".to_string(), format!("o/repo-{i:02}")))
            .collect();
        let pairs: Vec<(&str, &str)> = repos
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let mut app = app_with(&pairs);
        let list = metrics(&app).list;
        assert!(list < 41, "the overlay caps at the body height");
        press(&mut app, KeyCode::Char('G'));
        assert!(app.show.scroll > 0);
        let rows = list_rows(&app, "");
        assert_eq!(app.show.scroll + list, rows.len());
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.show.scroll, 0);
        for _ in 0..8 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::PageDown);
        assert!(app.show.scroll > 0);
    }

    #[test]
    fn new_projects_after_a_refresh_stay_shown_when_the_list_was_narrowed() {
        let mut app = app_with(REPOS);
        app.state
            .queue_settings
            .projects
            .hide(&SourceId::new("s1"), "o/beta");
        let mut fresh = change(99, MyRole::Reviewing, 5);
        fresh.id.repo = "o/newcomer".into();
        fresh.id.source_id = SourceId::new("s1");
        app.state.changes.push(fresh);
        assert!(!hidden(&app, "s1", "o/newcomer"));
        let groups = projects::catalog(&app.state);
        assert_eq!(projects::tally(&app.state, &groups), (4, 5));
    }

    #[test]
    fn the_end_note_says_what_each_filter_hid() {
        let mut app = app_with(REPOS);
        app.state.changes[0].draft = true;
        app.state
            .queue_settings
            .projects
            .hide(&SourceId::new("s1"), "o/beta");
        let hidden = crate::app::queue::hidden_split(&app.state, None);
        assert_eq!(
            hidden.note().as_deref(),
            Some("2 hidden by your Show filters: 1 by kind, 1 by project.")
        );
    }

    #[test]
    fn w_asks_for_a_save_only_with_a_config_to_write_to() {
        let mut app = app_with(REPOS);
        app.state.sources = vec![crate::app::queue::tests::source("s1", true)];
        app.state
            .queue_settings
            .projects
            .hide(&SourceId::new("s1"), "o/beta");
        let cmds = press(&mut app, KeyCode::Char('w'));
        assert!(!cmds
            .iter()
            .any(|c| matches!(c, Cmd::SaveHiddenProjects { .. })));
        assert!(app.toasts[0].notice.text.contains("no config file"));

        app.project_save = Some("/c/config.toml".into());
        let cmds = press(&mut app, KeyCode::Char('w'));
        let [Cmd::SaveHiddenProjects { target, sources }] = &cmds[..] else {
            panic!("{cmds:?}");
        };
        assert_eq!(target, std::path::Path::new("/c/config.toml"));
        assert_eq!(sources, &[("s1".to_string(), vec!["o/beta".to_string()])]);
    }

    #[test]
    fn demo_mode_never_asks_for_a_write() {
        let mut app = app_with(REPOS);
        app.demo = true;
        app.project_save = Some("/c/config.toml".into());
        let cmds = press(&mut app, KeyCode::Char('w'));
        assert!(!cmds
            .iter()
            .any(|c| matches!(c, Cmd::SaveHiddenProjects { .. })));
        assert!(app.toasts[0].notice.text.contains("(demo)"));
    }
}
