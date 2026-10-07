//! What the interface shows once a source's capability probe has answered: actions the source
//! can't do leave the chips and the review block, and their keys explain instead of failing.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_core::{Capabilities, ForgeKind, ProbeOutcome, Provider, Timestamp};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Msg, Screen, Snapshot};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::{self, HitMap};

fn world() -> DemoWorld {
    DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap()
}

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

fn press(app: &mut App, code: KeyCode) {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn text(buffer: &Buffer) -> String {
    let width = usize::from(buffer.area.width);
    buffer
        .content()
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render(app: &mut App) -> String {
    let (w, h) = app.size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|frame| hits = ui::draw(frame, app)).unwrap();
    app.hits = hits;
    text(terminal.backend().buffer())
}

fn probe(app: &mut App, source: &rb_core::SourceId, request_changes: bool, reason: Option<&str>) {
    let mut outcome = ProbeOutcome::new(Capabilities {
        request_changes,
        ..Capabilities::all()
    });
    outcome.complete = true;
    outcome.version = Some("16.9.2".into());
    if let Some(reason) = reason {
        outcome
            .reasons
            .push((rb_core::FeatureAction::RequestChanges, reason.into()));
    }
    update(
        app,
        Msg::Probed {
            source: source.clone(),
            outcome: Box::new(outcome),
            at: Timestamp(0),
        },
    );
}

/// The demo app with its first GitLab change selected, and that change's source.
fn on_gitlab() -> (App, rb_core::SourceId) {
    let snapshot: Snapshot = block_on(world().snapshot()).unwrap();
    let slot = snapshot
        .sources
        .iter()
        .position(|s| s.kind == ForgeKind::GitLab)
        .unwrap();
    let mut app = App::new(AppConfig {
        theme_id: "afterglow-dark".into(),
        depth: ColourDepth::TrueColour,
        no_color: true,
        size: (160, 40),
    });
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    let key = char::from_digit(u32::try_from(slot).unwrap() + 2, 10).unwrap();
    press(&mut app, KeyCode::Char(key));
    for _ in 0..40 {
        if app
            .selected_change()
            .is_some_and(|c| c.id.kind == ForgeKind::GitLab)
        {
            break;
        }
        press(&mut app, KeyCode::Char('j'));
    }
    assert_eq!(app.selected_change().unwrap().id.kind, ForgeKind::GitLab);
    let source = app.selected_change().unwrap().id.source_id.clone();
    (app, source)
}

const REASON: &str =
    "GitLab 16.9 on gitlab.example.com doesn't support requesting changes (it needs 17.3 or newer).";

#[test]
fn unprobed_sources_show_every_chip() {
    let (mut app, _) = on_gitlab();
    assert!(render(&mut app).contains("x Request changes"));
}

#[test]
fn a_source_without_request_changes_loses_the_chip_and_x_explains() {
    let (mut app, source) = on_gitlab();
    probe(&mut app, &source, false, Some(REASON));
    let screen = render(&mut app);
    assert!(!screen.contains("Request changes"), "{screen}");
    assert!(screen.contains("a Approve") && screen.contains("c Comment"));
    insta::assert_snapshot!("dashboard_chips_without_request_changes", screen);

    press(&mut app, KeyCode::Char('x'));
    let screen = render(&mut app);
    assert!(
        screen.contains(&format!("{REASON} Approve or comment instead.")),
        "{screen}"
    );
}

#[test]
fn a_source_that_can_request_changes_keeps_the_chip() {
    let (mut app, source) = on_gitlab();
    probe(&mut app, &source, true, None);
    assert!(render(&mut app).contains("x Request changes"));
}

#[test]
fn the_review_block_lists_only_available_verdicts_and_x_explains_in_the_diff() {
    let (mut app, source) = on_gitlab();
    probe(&mut app, &source, false, Some(REASON));
    let id = app.selected_change().unwrap().id.clone();
    let data = block_on(world().diff_data(&id)).unwrap();
    press(&mut app, KeyCode::Enter);
    update(
        &mut app,
        Msg::DiffLoaded {
            id,
            result: Ok(Box::new(data)),
        },
    );
    assert_eq!(app.screen, Screen::Diff);
    let screen = render(&mut app);
    assert!(screen.contains("a approve · c comment"), "{screen}");
    assert!(screen.contains("R post a review"), "{screen}");
    assert!(!screen.contains("x request changes"), "{screen}");

    press(&mut app, KeyCode::Char('x'));
    let screen = render(&mut app);
    assert!(
        screen.contains("doesn't support requesting changes"),
        "{screen}"
    );
    insta::assert_snapshot!("diff_x_explains_without_request_changes", screen);
}

#[test]
fn probes_for_one_source_leave_others_alone() {
    let (mut app, source) = on_gitlab();
    probe(&mut app, &rb_core::SourceId::new("elsewhere"), false, None);
    assert!(app
        .state
        .supports(&source, rb_core::FeatureAction::RequestChanges));
    let _ = world().provider(ForgeKind::GitLab).capabilities();
}
