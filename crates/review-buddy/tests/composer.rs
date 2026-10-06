//! The composer, its confirmations and the approve flow, rendered headlessly from the frozen demo
//! data (pinned with `insta`) and driven through the demo provider.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_core::{MyReview, Provider, Verdict};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Cmd, Msg, Screen, Snapshot};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::runtime::{execute, Backend, Platform};
use review_buddy::ui::{self, HitMap};

fn world() -> DemoWorld {
    DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap()
}

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(f)
}

fn snapshot(world: &DemoWorld) -> Snapshot {
    block_on(world.snapshot()).unwrap()
}

fn open_in(world: &DemoWorld, theme: &str, width: u16, height: u16) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (width, height),
    });
    update(&mut app, Msg::Loaded(Box::new(snapshot(world))));
    let id = app.selected_change().unwrap().id.clone();
    assert_eq!(id.number, 214);
    let cmds = press(&mut app, KeyCode::Enter);
    assert!(matches!(cmds.as_slice(), [Cmd::LoadDiff(_)]));
    let data = block_on(world.diff_data(&id)).unwrap();
    update(
        &mut app,
        Msg::DiffLoaded {
            id,
            result: Ok(Box::new(data)),
        },
    );
    assert_eq!(app.screen, Screen::Diff);
    app
}

fn open_demo(theme: &str, width: u16, height: u16) -> App {
    open_in(&world(), theme, width, height)
}

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        match c {
            '\n' => {
                update(
                    app,
                    Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
                );
            }
            c => {
                press(app, KeyCode::Char(c));
            }
        }
    }
}

fn render(app: &mut App) -> Buffer {
    let (w, h) = app.size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|frame| hits = ui::draw(frame, app)).unwrap();
    app.hits = hits;
    terminal.backend().buffer().clone()
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

fn composing(theme: &str, width: u16, height: u16) -> App {
    let mut a = open_demo(theme, width, height);
    press(&mut a, KeyCode::Char('c'));
    type_text(&mut a, "This could return early.\nWorth a guard?");
    a
}

fn discarding(theme: &str, width: u16, height: u16) -> App {
    let mut a = composing(theme, width, height);
    press(&mut a, KeyCode::Esc);
    a
}

fn approving(theme: &str, width: u16, height: u16) -> App {
    let mut a = open_demo(theme, width, height);
    press(&mut a, KeyCode::Char('a'));
    a
}

macro_rules! snapshots {
    ($($name:ident: $scene:ident($theme:literal, $w:literal, $h:literal);)*) => {
        $(
            #[test]
            fn $name() {
                let mut a = $scene($theme, $w, $h);
                insta::assert_snapshot!(text(&render(&mut a)));
            }
        )*
    };
}

snapshots! {
    composer_160x40_default_theme: composing("liminal-hq", 160, 40);
    composer_160x40_dusk: composing("dusk", 160, 40);
    composer_100x30_default_theme: composing("liminal-hq", 100, 30);
    composer_100x30_dusk: composing("dusk", 100, 30);
    discard_confirm_160x40_default_theme: discarding("liminal-hq", 160, 40);
    discard_confirm_160x40_dusk: discarding("dusk", 160, 40);
    discard_confirm_100x30_default_theme: discarding("liminal-hq", 100, 30);
    discard_confirm_100x30_dusk: discarding("dusk", 100, 30);
    approve_preview_160x40_default_theme: approving("liminal-hq", 160, 40);
    approve_preview_160x40_dusk: approving("dusk", 160, 40);
    approve_preview_100x30_default_theme: approving("liminal-hq", 100, 30);
    approve_preview_100x30_dusk: approving("dusk", 100, 30);
}

#[test]
fn the_composer_shows_its_title_keys_and_the_cursor() {
    let mut a = composing("liminal-hq", 160, 40);
    let buffer = render(&mut a);
    let shown = text(&buffer);
    assert!(shown.contains("Comment · menus.rs line"), "{shown}");
    assert!(shown.contains("⏎ add to review · ⌃⏎ post now · ⇧⏎ newline · esc discard"));
    assert!(shown.contains("Worth a guard?"));
    let reversed = buffer
        .content()
        .iter()
        .filter(|c| c.modifier.contains(ratatui::style::Modifier::REVERSED))
        .count();
    assert_eq!(reversed, 1, "one caret");
}

#[test]
fn the_discard_default_is_the_safe_button() {
    let mut a = discarding("liminal-hq", 160, 40);
    let shown = text(&render(&mut a));
    assert!(shown.contains("› No, keep editing ‹"), "{shown}");
    assert!(shown.contains("Discard this comment?"));
    assert!(!shown.contains("› Discard ‹"));
}

#[test]
fn the_approve_preview_lists_the_pending_comment_and_the_verdict() {
    let mut a = approving("liminal-hq", 160, 40);
    let shown = text(&render(&mut a));
    assert!(shown.contains("Approve with 1 comment"), "{shown}");
    assert!(shown.contains("Verdict: approve"));
    assert!(shown.contains("› Approve ‹"));
}

#[test]
fn a_pending_comment_shows_under_its_line_and_in_the_review_block() {
    let mut a = open_demo("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('c'));
    type_text(&mut a, "Added in the test");
    press(&mut a, KeyCode::Enter);
    let shown = text(&render(&mut a));
    assert!(shown.contains("pending comment · line"), "{shown}");
    assert!(shown.contains("Added in the test"));
    assert!(shown.contains("1 suggestion · 1 comment"), "{shown}");
}

/// Runs the commands the way the event loop would and feeds their answers back in.
fn settle(app: &mut App, backend: &Backend, cmds: Vec<Cmd>) {
    block_on(async {
        let platform = Platform::system();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut pending = cmds;
        while !pending.is_empty() {
            for cmd in std::mem::take(&mut pending) {
                match cmd {
                    Cmd::SubmitReview { .. } | Cmd::Reply { .. } | Cmd::LoadChanges => {
                        execute(cmd, &tx, backend, &platform);
                        let msg = rx.recv().await.expect("an answer");
                        pending.extend(update(app, msg));
                    }
                    _ => {}
                }
            }
        }
    });
}

#[test]
fn approving_in_demo_mutates_memory_and_clears_the_draft() {
    let world = world();
    let backend = Backend::Demo(world.clone());
    let mut a = open_in(&world, "liminal-hq", 160, 40);
    let id = a.diff.as_ref().unwrap().id.clone();
    assert!(
        !world.draft(&id).unwrap().is_empty(),
        "the fixture's pending suggestion"
    );

    press(&mut a, KeyCode::Char('c'));
    type_text(&mut a, "Looks right to me");
    press(&mut a, KeyCode::Enter);
    press(&mut a, KeyCode::Char('a'));
    let cmds = press(&mut a, KeyCode::Enter);
    assert!(matches!(
        cmds.as_slice(),
        [Cmd::SubmitReview {
            verdict: Verdict::Approve,
            ..
        }]
    ));
    settle(&mut a, &backend, cmds);

    assert_eq!(a.toasts.last().unwrap().notice.text, "Approved (demo)");
    assert!(
        world.draft(&id).unwrap().is_empty(),
        "memory's draft is cleared"
    );
    assert!(!a.has_unsent_drafts());
    assert!(a
        .diff
        .as_ref()
        .unwrap()
        .data
        .as_ref()
        .unwrap()
        .draft
        .is_empty());
    let confirmations = world.confirmations();
    assert!(
        confirmations.last().unwrap().ends_with("(demo)"),
        "{confirmations:?}"
    );

    let provider = world.provider(id.kind);
    let threads = block_on(provider.threads(&id)).unwrap();
    assert!(threads
        .iter()
        .any(|t| t.comments.iter().any(|c| c.body == "Looks right to me")));
    let fresh = snapshot(&world);
    let change = fresh.changes.iter().find(|c| c.id == id).unwrap();
    assert_eq!(change.my_review, MyReview::Approved);
    assert_eq!(
        a.state
            .changes
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .my_review,
        MyReview::Approved,
        "the queue refreshed after the approval"
    );
}

#[test]
fn posting_now_in_demo_adds_a_thread_and_keeps_the_pending_draft() {
    let world = world();
    let backend = Backend::Demo(world.clone());
    let mut a = open_in(&world, "liminal-hq", 160, 40);
    let id = a.diff.as_ref().unwrap().id.clone();
    let before = block_on(world.provider(id.kind).threads(&id))
        .unwrap()
        .len();
    press(&mut a, KeyCode::Char('c'));
    type_text(&mut a, "One-off note");
    update(
        &mut a,
        Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
    );
    let shown = text(&render(&mut a));
    assert!(shown.contains("Post review with 1 comment"), "{shown}");
    let cmds = press(&mut a, KeyCode::Enter);
    settle(&mut a, &backend, cmds);

    assert_eq!(
        a.toasts.last().unwrap().notice.text,
        "Comment posted (demo)"
    );
    assert!(a.diff.as_ref().unwrap().composer.is_none());
    let after = block_on(world.provider(id.kind).threads(&id))
        .unwrap()
        .len();
    assert_eq!(after, before + 1);
}

#[test]
fn replying_in_demo_adds_the_comment_to_the_thread() {
    let world = world();
    let backend = Backend::Demo(world.clone());
    let mut a = open_in(&world, "liminal-hq", 160, 40);
    let id = a.diff.as_ref().unwrap().id.clone();
    for _ in 0..40 {
        press(&mut a, KeyCode::Char('r'));
        if a.diff.as_ref().unwrap().composer.is_some() {
            break;
        }
        press(&mut a, KeyCode::Char('j'));
    }
    assert!(
        a.diff.as_ref().unwrap().composer.is_some(),
        "a thread line exists"
    );
    type_text(&mut a, "Thanks, fixed");
    let cmds = press(&mut a, KeyCode::Enter);
    assert!(cmds.is_empty(), "previewed first");
    let cmds = press(&mut a, KeyCode::Enter);
    settle(&mut a, &backend, cmds);
    assert_eq!(a.toasts.last().unwrap().notice.text, "Reply posted (demo)");
    let threads = block_on(world.provider(id.kind).threads(&id)).unwrap();
    assert!(threads
        .iter()
        .any(|t| t.comments.iter().any(|c| c.body == "Thanks, fixed")));
}

#[test]
fn without_a_backend_the_failure_is_calm_and_keeps_the_draft() {
    let mut a = open_demo("liminal-hq", 160, 40);
    press(&mut a, KeyCode::Char('a'));
    let cmds = press(&mut a, KeyCode::Enter);
    settle(&mut a, &Backend::None, cmds);
    let toast = &a.toasts.last().unwrap().notice;
    assert!(
        toast.text.contains("Press a to try again"),
        "{}",
        toast.text
    );
    assert!(a.has_unsent_drafts());
}
