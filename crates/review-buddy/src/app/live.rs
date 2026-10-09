//! Pure transitions for sources that load over the network: refreshing, per-source results,
//! and fetching a change's details the first time it is selected.

use rb_core::{ChangeSummary, SourceId, Timestamp};

use super::update::{push_toast, set_status};
use super::{
    dashboard, diff, App, ChangeInfo, Cmd, FailureKind, Notice, NoticeKind, SourceFailure,
    SourceStatus,
};
use rb_core::ChangeId;

/// How a refresh was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Auto,
    Manual,
    Focus,
}

/// Asks for every source to be refreshed, unless a refresh is already under way. `announce` is
/// for a refresh you asked for with `r`.
pub fn request_refresh(app: &mut App, announce: bool) -> Vec<Cmd> {
    ask(app, if announce { Ask::Manual } else { Ask::Auto })
}

/// The terminal regained focus. The runtime skips it if a refresh just ran.
pub fn request_focus_refresh(app: &mut App) -> Vec<Cmd> {
    ask(app, Ask::Focus)
}

fn ask(app: &mut App, how: Ask) -> Vec<Cmd> {
    if app.state.sources.is_empty() {
        return Vec::new();
    }
    if app.state.pending_sources > 0 {
        if how == Ask::Manual {
            return set_status(app, Notice::new(NoticeKind::Info, "Already refreshing."));
        }
        return Vec::new();
    }
    let state = &mut app.state;
    state.pending_sources = state.sources.len();
    state.refreshing = state.sources.iter().map(|s| s.id.clone()).collect();
    state
        .info_requested
        .retain(|id| state.details.contains_key(id));
    app.mark_dirty();
    let mut cmds = vec![match how {
        Ask::Auto => Cmd::LoadChanges,
        Ask::Manual => Cmd::LoadChangesNow,
        Ask::Focus => Cmd::LoadChangesOnFocus,
    }];
    if how == Ask::Manual {
        cmds.extend(set_status(
            app,
            Notice::new(NoticeKind::Info, "Refreshing…"),
        ));
    }
    cmds
}

/// The interval elapsed. Only refreshes a focused app that has finished loading.
pub fn on_refresh_due(app: &mut App) -> Vec<Cmd> {
    if app.focused && app.state.loaded {
        return request_refresh(app, false);
    }
    Vec::new()
}

/// A refresh was skipped because one ran moments ago.
pub fn on_refresh_skipped(app: &mut App) -> Vec<Cmd> {
    let state = &mut app.state;
    state.pending_sources = 0;
    state.refreshing.clear();
    app.mark_dirty();
    Vec::new()
}

pub fn on_source_status(app: &mut App, source: SourceId, status: SourceStatus) -> Vec<Cmd> {
    app.state.statuses.insert(source, status);
    app.mark_dirty();
    Vec::new()
}

pub fn on_source_loaded(
    app: &mut App,
    source: SourceId,
    result: Result<Vec<ChangeSummary>, SourceFailure>,
    now: Timestamp,
) -> Vec<Cmd> {
    if app.state.sources.iter().any(|s| s.id == source) {
        let state = &mut app.state;
        state.pending_sources = state.pending_sources.saturating_sub(1);
        state.refreshing.remove(&source);
    }
    apply_result(app, source, result, now)
}

pub fn on_source_updated(
    app: &mut App,
    source: SourceId,
    result: Result<Vec<ChangeSummary>, SourceFailure>,
    now: Timestamp,
) -> Vec<Cmd> {
    apply_result(app, source, result, now)
}

fn apply_result(
    app: &mut App,
    source: SourceId,
    result: Result<Vec<ChangeSummary>, SourceFailure>,
    now: Timestamp,
) -> Vec<Cmd> {
    let Some(label) = app
        .state
        .sources
        .iter()
        .find(|s| s.id == source)
        .map(|s| s.label.clone())
    else {
        return Vec::new();
    };
    let state = &mut app.state;
    state.now = Some(now);
    let before = state.failures.get(&source).map(|f| f.kind);
    let mut cmds = Vec::new();
    match result {
        Ok(items) => {
            let old: Vec<ChangeSummary> = state
                .changes
                .iter()
                .filter(|c| c.id.source_id == source)
                .cloned()
                .collect();
            state.changes.retain(|c| c.id.source_id != source);
            for item in &items {
                if old
                    .iter()
                    .find(|o| o.id == item.id)
                    .is_none_or(|o| o != item)
                {
                    state.details.remove(&item.id);
                    state.info_requested.remove(&item.id);
                }
            }
            state
                .details
                .retain(|id, _| id.source_id != source || items.iter().any(|i| &i.id == id));
            state.changes.extend(items);
            state.failures.remove(&source);
            state.statuses.insert(source, SourceStatus::Ok);
            state.last_refreshed = Some(now);
            if before.is_some() {
                cmds = push_toast(
                    app,
                    Notice::new(NoticeKind::Success, format!("{label} is back.")),
                );
            }
        }
        Err(failure) => {
            let kept = state.statuses.get(&source).copied();
            let status = match failure.kind {
                FailureKind::Offline => match kept {
                    Some(s @ SourceStatus::Offline { .. }) => s,
                    _ => SourceStatus::Offline { since: now },
                },
                FailureKind::RateLimited => match kept {
                    Some(s @ SourceStatus::RateLimited { .. }) => s,
                    _ => SourceStatus::Failed,
                },
                FailureKind::SignIn => SourceStatus::AuthFailed,
                _ => SourceStatus::Failed,
            };
            state.statuses.insert(source.clone(), status);
            let text = failure.toast(&label);
            let changed = before != Some(failure.kind);
            state.failures.insert(source, failure);
            if changed {
                cmds = push_toast(app, Notice::new(NoticeKind::Warning, text));
            }
        }
    }
    app.change_count = app.state.changes.len();
    dashboard::reconcile(app);
    app.mark_dirty();
    cmds
}

pub fn on_cache_time(app: &mut App, at: Timestamp) -> Vec<Cmd> {
    app.state.last_refreshed.get_or_insert(at);
    app.mark_dirty();
    Vec::new()
}

pub fn on_info_loaded(
    app: &mut App,
    id: ChangeId,
    result: Result<Box<ChangeInfo>, String>,
) -> Vec<Cmd> {
    app.mark_dirty();
    match result {
        Ok(info) => {
            diff::refresh_threads(app, &id, &info.threads);
            app.state.details.insert(id, *info);
            app.images.checked = None;
            Vec::new()
        }
        Err(message) => push_toast(
            app,
            Notice::new(
                NoticeKind::Warning,
                format!(
                    "Couldn't load the details for {}. {message}",
                    id.short_ref()
                ),
            ),
        ),
    }
}

/// The command that fetches the selected change's details, if they haven't been asked for.
pub fn ensure_info(app: &mut App) -> Option<Cmd> {
    let id = app.selected_change()?.id.clone();
    if app.state.details.contains_key(&id) || !app.state.info_requested.insert(id.clone()) {
        return None;
    }
    Some(Cmd::LoadInfo(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{update, AppConfig, Msg, Snapshot};
    use rb_core::{AuthMode, ChangeState, CiState, ForgeKind, MyReview, MyRole, Scope, Source};
    use rb_theme::ColourDepth;

    fn source(name: &str) -> Source {
        Source {
            id: SourceId::new(name),
            kind: ForgeKind::GitHub,
            host: "github.com".into(),
            label: name.into(),
            scope: Scope::everything(),
            auth: AuthMode::Cli,
            in_all: true,
            include_drafts: true,
            tag_colour: None,
        }
    }

    fn change(source: &str, number: u64, title: &str) -> ChangeSummary {
        ChangeSummary {
            id: ChangeId {
                source_id: SourceId::new(source),
                kind: ForgeKind::GitHub,
                repo: "acme/web".into(),
                number,
            },
            title: title.into(),
            author: "mira".into(),
            author_is_bot: false,
            state: ChangeState::Open,
            draft: false,
            created_at: Timestamp(1),
            updated_at: Timestamp(2),
            branch: "feat".into(),
            base: "main".into(),
            head_sha: "abc".into(),
            base_sha: "def".into(),
            adds: 1,
            dels: 1,
            files: 1,
            ci: CiState::Pass,
            labels: Vec::new(),
            reviewers: Vec::new(),
            my_role: MyRole::Reviewing,
            my_review: MyReview::None,
            my_reviewed_sha: None,
            i_commented: false,
            has_new_activity: false,
            signals: Default::default(),
        }
    }

    fn started() -> (App, Vec<Cmd>) {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        let cached = Snapshot {
            label: "live".into(),
            sources: vec![source("a"), source("b")],
            changes: vec![change("a", 1, "cached one")],
            now: Timestamp(10),
            details: Default::default(),
        };
        let cmds = update(&mut app, Msg::Cached(Box::new(cached)));
        (app, cmds)
    }

    fn app() -> App {
        started().0
    }

    #[test]
    fn cached_rows_paint_then_a_refresh_is_requested() {
        let (a, cmds) = started();
        assert!(a.state.loaded);
        assert_eq!(a.state.changes.len(), 1);
        assert_eq!(a.state.pending_sources, 2);
        assert!(matches!(cmds.first(), Some(Cmd::LoadChanges)));
    }

    #[test]
    fn a_source_result_replaces_only_that_sources_rows() {
        let mut a = app();
        update(
            &mut a,
            Msg::SourceLoaded {
                source: SourceId::new("a"),
                result: Ok(vec![change("a", 2, "live two")]),
                now: Timestamp(20),
            },
        );
        assert_eq!(a.state.changes.len(), 1);
        assert_eq!(a.state.changes[0].title, "live two");
        assert_eq!(a.state.pending_sources, 1);
        assert_eq!(a.state.now, Some(Timestamp(20)));
    }

    #[test]
    fn a_failure_keeps_cached_rows_and_toasts_a_next_step() {
        let mut a = app();
        let cmds = update(
            &mut a,
            Msg::SourceLoaded {
                source: SourceId::new("a"),
                result: Err(SourceFailure::sign_in("github.com")),
                now: Timestamp(20),
            },
        );
        assert_eq!(a.state.changes.len(), 1);
        assert!(a.state.failures.contains_key(&SourceId::new("a")));
        assert!(a.toasts[0].notice.text.contains("gh auth login"));
        assert!(!cmds.is_empty());
        update(
            &mut a,
            Msg::SourceLoaded {
                source: SourceId::new("a"),
                result: Ok(Vec::new()),
                now: Timestamp(30),
            },
        );
        assert!(a.state.failures.is_empty());
    }

    #[test]
    fn selecting_a_change_asks_for_its_details_once() {
        let (mut a, cmds) = started();
        let id = a.state.changes[0].id.clone();
        assert!(cmds
            .iter()
            .any(|c| matches!(c, Cmd::LoadInfo(i) if *i == id)));
        assert!(update(&mut a, Msg::Tick).is_empty());
        update(
            &mut a,
            Msg::InfoLoaded {
                id: id.clone(),
                result: Ok(Box::new(ChangeInfo::default())),
            },
        );
        assert!(a.state.details.contains_key(&id));
    }

    #[test]
    fn refresh_is_not_stacked() {
        let mut a = app();
        assert_eq!(a.state.pending_sources, 2);
        assert!(request_refresh(&mut a, false).is_empty());
        let cmds = request_refresh(&mut a, true);
        assert!(!cmds.is_empty() && !matches!(cmds[0], Cmd::LoadChanges));
    }

    #[test]
    fn focus_regain_refreshes_only_when_enabled_and_idle() {
        let (mut a, _) = started();
        for n in 0..2 {
            update(
                &mut a,
                Msg::SourceLoaded {
                    source: SourceId::new(if n == 0 { "a" } else { "b" }),
                    result: Ok(Vec::new()),
                    now: Timestamp(20),
                },
            );
        }
        a.refresh_on_focus = false;
        assert!(update(&mut a, Msg::FocusGained).is_empty());
        a.refresh_on_focus = true;
        let cmds = update(&mut a, Msg::FocusGained);
        assert!(matches!(cmds.first(), Some(Cmd::LoadChangesOnFocus)));
        assert!(update(&mut a, Msg::FocusGained).is_empty());
    }
}
