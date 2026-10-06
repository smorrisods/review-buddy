//! Pure transitions for sources that load over the network: refreshing, per-source results,
//! and fetching a change's details the first time it is selected.

use rb_core::{ChangeSummary, SourceId, Timestamp};

use super::update::{push_toast, set_status};
use super::{dashboard, diff, App, ChangeInfo, Cmd, Notice, NoticeKind, SourceFailure};
use rb_core::ChangeId;

/// Asks for every source to be refreshed, unless a refresh is already under way.
pub fn request_refresh(app: &mut App, announce: bool) -> Vec<Cmd> {
    if app.state.sources.is_empty() {
        return Vec::new();
    }
    if app.state.pending_sources > 0 {
        if announce {
            return set_status(app, Notice::new(NoticeKind::Info, "Already refreshing."));
        }
        return Vec::new();
    }
    app.state.pending_sources = app.state.sources.len();
    let state = &mut app.state;
    state
        .info_requested
        .retain(|id| state.details.contains_key(id));
    app.mark_dirty();
    let mut cmds = vec![Cmd::LoadChanges];
    if announce {
        cmds.extend(set_status(
            app,
            Notice::new(NoticeKind::Info, "Refreshing…"),
        ));
    }
    cmds
}

pub fn on_source_loaded(
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
    state.pending_sources = state.pending_sources.saturating_sub(1);
    state.now = Some(now);
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
        }
        Err(failure) => {
            let text = failure.toast(&label);
            state.failures.insert(source, failure);
            cmds = push_toast(app, Notice::new(NoticeKind::Warning, text));
        }
    }
    app.change_count = app.state.changes.len();
    dashboard::reconcile(app);
    app.mark_dirty();
    cmds
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
        assert!(matches!(cmds.first(), Some(Cmd::LoadChanges)));
        assert!(update(&mut a, Msg::FocusGained).is_empty());
    }
}
