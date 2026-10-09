//! How the interface shows refresh state: the offline banner, per-source notes, the spinner and
//! the last-refreshed time. Built from messages alone, so no network or clock is involved.

use ratatui::{backend::TestBackend, Terminal};
use rb_core::{
    AuthMode, ChangeId, ChangeState, ChangeSummary, CiState, Error, ForgeKind, MyReview, MyRole,
    Scope, Source, SourceId, Timestamp,
};
use rb_theme::ColourDepth;
use review_buddy::app::{
    update, App, AppConfig, FailureKind, Msg, Snapshot, SourceFailure, SourceStatus,
};
use review_buddy::ui::{self, HitMap};

const CACHED: Timestamp = Timestamp(20 * 86_400 + 10 * 3600 + 42 * 60);
const RESETS: Timestamp = Timestamp(20 * 86_400 + 10 * 3600 + 58 * 60);

fn source(name: &str, host: &str) -> Source {
    Source {
        id: SourceId::new(name),
        kind: ForgeKind::GitHub,
        host: host.into(),
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
        created_at: Timestamp(CACHED.0 - 7200),
        updated_at: Timestamp(CACHED.0 - 3600),
        branch: "feat".into(),
        base: "main".into(),
        head_sha: "abc".into(),
        base_sha: "def".into(),
        adds: 4,
        dels: 2,
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

fn started(theme: &str, size: (u16, u16)) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size,
    });
    app.reduced_motion = true;
    let cached = Snapshot {
        label: "live".into(),
        sources: vec![source("work", "github.com"), source("oss", "gitlab.com")],
        changes: vec![
            change("work", 1, "Add retry to sync"),
            change("oss", 2, "Fix the flaky upload test"),
        ],
        now: CACHED,
        details: Default::default(),
    };
    update(&mut app, Msg::Cached(Box::new(cached)));
    update(&mut app, Msg::CacheTime(CACHED));
    app
}

fn loaded(app: &mut App, name: &str, result: Result<Vec<ChangeSummary>, SourceFailure>) {
    update(
        app,
        Msg::SourceLoaded {
            source: SourceId::new(name),
            result,
            now: CACHED,
        },
    );
}

fn offline(theme: &str, size: (u16, u16)) -> App {
    let mut app = started(theme, size);
    let down = || {
        SourceFailure::from_error(
            &Error::Network {
                host: "github.com".into(),
                reason: "connection refused".into(),
            },
            "github.com",
        )
    };
    loaded(&mut app, "work", Err(down()));
    loaded(&mut app, "oss", Err(down()));
    app
}

fn paused(theme: &str, size: (u16, u16)) -> App {
    let mut app = started(theme, size);
    update(
        &mut app,
        Msg::SourceStatus {
            source: SourceId::new("work"),
            status: SourceStatus::RateLimited { until: RESETS },
        },
    );
    let limited = SourceFailure::from_error(
        &Error::RateLimited {
            host: "github.com".into(),
            retry_after_secs: Some(960),
        },
        "github.com",
    );
    loaded(&mut app, "work", Err(limited));
    loaded(
        &mut app,
        "oss",
        Ok(vec![change("oss", 2, "Fix the flaky upload test")]),
    );
    app
}

fn refreshing(theme: &str, size: (u16, u16)) -> App {
    started(theme, size)
}

fn text(app: &mut App) -> String {
    let (w, h) = app.size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|frame| hits = ui::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    buffer
        .content()
        .chunks(usize::from(buffer.area.width))
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

macro_rules! frames {
    ($($name:ident: $scene:ident($theme:literal, $w:literal, $h:literal);)*) => {
        $(
            #[test]
            fn $name() {
                let mut app = $scene($theme, ($w, $h));
                insta::assert_snapshot!(text(&mut app));
            }
        )*
    };
}

frames! {
    offline_banner_liminal_hq_160x40: offline("liminal-hq", 160, 40);
    offline_banner_dusk_160x40: offline("dusk", 160, 40);
    offline_banner_liminal_hq_100x30: offline("liminal-hq", 100, 30);
    offline_banner_dusk_100x30: offline("dusk", 100, 30);
    rate_limit_note_liminal_hq_160x40: paused("liminal-hq", 160, 40);
    rate_limit_note_dusk_160x40: paused("dusk", 160, 40);
    rate_limit_note_liminal_hq_100x30: paused("liminal-hq", 100, 30);
    rate_limit_note_dusk_100x30: paused("dusk", 100, 30);
    offline_banner_afterglow_dark_160x40: offline("afterglow-dark", 160, 40);
    offline_banner_afterglow_dark_100x30: offline("afterglow-dark", 100, 30);
    rate_limit_note_afterglow_dark_160x40: paused("afterglow-dark", 160, 40);
    rate_limit_note_afterglow_dark_100x30: paused("afterglow-dark", 100, 30);
    refreshing_afterglow_dark_160x40: refreshing("afterglow-dark", 160, 40);
    refreshing_liminal_hq_160x40: refreshing("liminal-hq", 160, 40);
    refreshing_liminal_hq_100x30: refreshing("liminal-hq", 100, 30);
    refreshing_dusk_160x40: refreshing("dusk", 160, 40);
    refreshing_afterglow_dark_100x30: refreshing("afterglow-dark", 100, 30);
    refreshing_dusk_100x30: refreshing("dusk", 100, 30);
}

#[test]
fn the_banner_needs_every_source_offline_and_something_cached() {
    let mut app = started("liminal-hq", (160, 40));
    assert!(app.state.offline_since_cache().is_none());
    let down = SourceFailure::from_error(
        &Error::Network {
            host: "h".into(),
            reason: "x".into(),
        },
        "h",
    );
    loaded(&mut app, "work", Err(down.clone()));
    assert!(
        app.state.offline_since_cache().is_none(),
        "one source is fine"
    );
    loaded(&mut app, "oss", Err(down));
    assert_eq!(app.state.offline_since_cache(), Some(CACHED));
    assert!(text(&mut app).contains("offline · cached 10:42"));
    assert_eq!(
        app.state.failures[&SourceId::new("work")].kind,
        FailureKind::Offline
    );
}

#[test]
fn toasts_come_from_changes_of_state_not_every_poll() {
    let mut app = offline("liminal-hq", (160, 40));
    assert_eq!(app.toasts.len(), 2, "one per source, on going offline");
    let down = app.state.failures[&SourceId::new("work")].clone();
    update(
        &mut app,
        Msg::SourceUpdated {
            source: SourceId::new("work"),
            result: Err(down),
            now: Timestamp(CACHED.0 + 60),
        },
    );
    assert_eq!(app.toasts.len(), 2, "still offline is not news");
    update(
        &mut app,
        Msg::SourceUpdated {
            source: SourceId::new("work"),
            result: Ok(vec![change("work", 1, "Add retry to sync")]),
            now: Timestamp(CACHED.0 + 120),
        },
    );
    assert_eq!(app.toasts.len(), 3);
    assert!(app.toasts[2].notice.text.contains("work is back"));
    assert_eq!(app.state.last_refreshed, Some(Timestamp(CACHED.0 + 120)));
    assert!(app.state.offline_since_cache().is_none());
    assert!(text(&mut app).contains("refreshed 10:44"));
}

#[test]
fn a_skipped_refresh_clears_the_spinner() {
    let mut app = refreshing("liminal-hq", (160, 40));
    assert_eq!(app.state.pending_sources, 2);
    update(&mut app, Msg::RefreshSkipped);
    assert_eq!(app.state.pending_sources, 0);
    assert!(app.state.refreshing.is_empty());
}

#[test]
fn the_interval_only_refreshes_a_focused_idle_app() {
    let mut app = started("liminal-hq", (160, 40));
    assert!(update(&mut app, Msg::RefreshDue).is_empty(), "already busy");
    update(&mut app, Msg::RefreshSkipped);
    update(&mut app, Msg::FocusLost);
    assert!(update(&mut app, Msg::RefreshDue).is_empty(), "unfocused");
    update(&mut app, Msg::FocusGained);
    update(&mut app, Msg::RefreshSkipped);
    assert!(!update(&mut app, Msg::RefreshDue).is_empty());
}
