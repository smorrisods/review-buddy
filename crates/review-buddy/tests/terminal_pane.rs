//! The terminal pane through `update`: opening it, the start prompt, focus and the chord, what
//! reaches the child, the mouse, resizing and demo mode's scripted pane. Frames at the two
//! reference sizes are pinned with `insta` in the default theme and Dusk.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use rb_term::worktree::{plan, Plan, Request};
use rb_theme::ColourDepth;
use review_buddy::app::terminal::{Choice, Phase, TermCmd, TermMsg, TerminalSettings};
use review_buddy::app::{update, App, AppConfig, Cmd, Msg};
use review_buddy::config::{DetailPosition, TerminalConfig, TerminalStart};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::layout::Size;

#[path = "support/render.rs"]
mod render_support;
use render_support::{render, text};

const SIZES: [(u16, u16); 2] = [(160, 40), (100, 30)];

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

fn dashboard(theme: &str, (w, h): (u16, u16), demo: bool) -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (w, h),
    });
    app.demo = demo;
    let snapshot = block_on(world.snapshot()).unwrap();
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    app
}

fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) -> Vec<Cmd> {
    update(app, Msg::Key(KeyEvent::new(code, mods)))
}

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    key(app, code, KeyModifiers::NONE)
}

fn chord(app: &mut App) -> Vec<Cmd> {
    key(app, KeyCode::Char('\\'), KeyModifiers::CONTROL)
}

fn type_text(app: &mut App, text: &str) -> Vec<Cmd> {
    text.chars()
        .flat_map(|c| press(app, KeyCode::Char(c)))
        .collect()
}

fn term_cmds(cmds: &[Cmd]) -> Vec<&TermCmd> {
    cmds.iter()
        .filter_map(|c| match c {
            Cmd::Term(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn screen_text(app: &App) -> String {
    app.term.pane.as_ref().expect("a pane").screen().text()
}

fn fixed_plan() -> Plan {
    fixed_plan_for("demo-gh", "acme/widgets", 214)
}

fn fixed_plan_for(source: &str, repo: &str, number: u64) -> Plan {
    plan(
        &Request {
            source: source.into(),
            repo: repo.into(),
            number,
            url: "https://github.com/acme/widgets/pull/214".into(),
            refspec: "pull/214/head".into(),
            remote: "origin".into(),
            clone_dir: "/src/widgets".into(),
            root: "/state/review-buddy/worktrees".into(),
        },
        false,
    )
}

/// A live (non-demo) app that is allowed to make worktrees.
fn live_app() -> App {
    let mut app = dashboard("liminal-hq", (160, 40), false);
    app.term.settings = TerminalSettings::from_config(&TerminalConfig::default())
        .with_worktrees_root("/state/review-buddy/worktrees".into());
    app
}

fn spawn_of(cmds: &[Cmd]) -> (u64, rb_term::SpawnSpec) {
    match term_cmds(cmds).as_slice() {
        [TermCmd::Spawn { gen, spec }] => (*gen, spec.clone()),
        other => panic!("expected one Spawn, got {other:?}"),
    }
}

#[test]
fn t_opens_a_scripted_pane_in_demo_and_sends_nothing_to_the_system() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    let cmds = press(&mut app, KeyCode::Char('t'));
    assert!(
        term_cmds(&cmds).is_empty(),
        "demo spawns and plans nothing: {cmds:?}"
    );
    let pane = app.term.pane.as_ref().unwrap();
    assert_eq!(pane.kind(), rb_term::PaneKind::Scripted);
    assert!(app.term.visible && app.term.focused);
    assert!(screen_text(&app).contains("(demo) This pane is scripted"));

    let cmds = type_text(&mut app, "env");
    assert!(cmds.iter().all(|c| !matches!(c, Cmd::Term(_))));
    let cmds = press(&mut app, KeyCode::Enter);
    assert!(term_cmds(&cmds).is_empty());
    let text = screen_text(&app);
    assert!(text.contains("RB_REPO="), "{text}");
}

#[test]
fn keys_go_to_the_pane_while_it_has_focus_and_ctrl_c_does_not_quit() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    press(&mut app, KeyCode::Char('t'));
    let before = app.dashboard.selected.clone();
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('q'));
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(!app.should_quit());
    assert_eq!(app.dashboard.selected, before, "j didn't move the queue");
    assert!(
        screen_text(&app).contains("jq"),
        "typed text reached the pane"
    );
}

#[test]
fn the_chord_then_esc_returns_focus_to_the_app() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    press(&mut app, KeyCode::Char('t'));
    chord(&mut app);
    assert!(app.term.focused, "the chord alone doesn't leave");
    press(&mut app, KeyCode::Esc);
    assert!(!app.term.focused && app.term.visible);
    press(&mut app, KeyCode::Char('?'));
    assert!(app.help, "keys reach the app again");
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('t'));
    assert!(app.term.focused, "t brings the keyboard back");
}

#[test]
fn double_escape_is_the_second_way_out_and_the_first_still_reaches_the_child() {
    let mut app = dashboard("liminal-hq", (160, 40), false);
    app.term.settings = TerminalSettings::from_config(&TerminalConfig::default());
    app.term.settings.worktrees_root = None;
    let cmds = press(&mut app, KeyCode::Char('t'));
    let (gen, _) = spawn_of(&cmds);
    let writes = press(&mut app, KeyCode::Esc);
    assert_eq!(
        term_cmds(&writes).len(),
        1,
        "the first esc is written: {writes:?}"
    );
    assert!(matches!(term_cmds(&writes)[0], TermCmd::Write(b) if b == b"\x1b"));
    assert!(app.term.focused);
    let second = press(&mut app, KeyCode::Esc);
    assert!(term_cmds(&second).is_empty());
    assert!(!app.term.focused);
    let _ = gen;
}

#[test]
fn a_custom_escape_chord_is_honoured_and_a_bad_one_is_explained() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    app.term.settings = TerminalSettings::from_config(&TerminalConfig {
        escape: "ctrl-]".into(),
        ..TerminalConfig::default()
    });
    press(&mut app, KeyCode::Char('t'));
    key(&mut app, KeyCode::Char('\\'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Esc);
    assert!(app.term.focused, "ctrl-\\ is no longer the chord");
    key(&mut app, KeyCode::Char(']'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Esc);
    assert!(!app.term.focused);

    let bad = TerminalSettings::from_config(&TerminalConfig {
        escape: "a".into(),
        ..TerminalConfig::default()
    });
    assert!(bad
        .chord_problem
        .as_deref()
        .unwrap()
        .contains("needs ctrl or alt"));
    assert_eq!(bad.chord, rb_term::Chord::default());
}

#[test]
fn chord_t_hides_the_pane_and_t_brings_it_back() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    press(&mut app, KeyCode::Char('t'));
    chord(&mut app);
    press(&mut app, KeyCode::Char('t'));
    assert!(!app.term.visible && app.term.pane.is_some());
    let full = review_buddy::app::terminal::app_body(&app);
    assert_eq!(full.width, 160, "the app has the whole body back");
    press(&mut app, KeyCode::Char('t'));
    assert!(app.term.visible && app.term.focused);
    assert!(review_buddy::app::terminal::app_body(&app).width < 160);
}

#[test]
fn chord_x_closes_a_scripted_pane_at_once_and_a_live_one_only_when_asked_twice() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    press(&mut app, KeyCode::Char('t'));
    chord(&mut app);
    press(&mut app, KeyCode::Char('x'));
    assert!(app.term.pane.is_none() && !app.term.visible);

    let mut app = live_app();
    app.term.settings.start = TerminalStart::Current;
    press(&mut app, KeyCode::Char('t'));
    chord(&mut app);
    let first = press(&mut app, KeyCode::Char('x'));
    assert!(app.term.pane.is_some(), "still running, so it asks first");
    assert!(term_cmds(&first).is_empty());
    assert!(app
        .status
        .as_ref()
        .unwrap()
        .notice
        .text
        .contains("still running"));
    chord(&mut app);
    let second = press(&mut app, KeyCode::Char('x'));
    assert!(app.term.pane.is_none());
    assert!(matches!(term_cmds(&second).as_slice(), [TermCmd::Close]));
}

#[test]
fn the_start_prompt_defaults_to_no_and_creates_nothing() {
    let mut app = live_app();
    let cmds = press(&mut app, KeyCode::Char('t'));
    assert!(matches!(
        term_cmds(&cmds).as_slice(),
        [TermCmd::Plan { .. }]
    ));
    assert!(matches!(
        app.term.prompt.as_ref().unwrap().phase,
        Phase::Planning
    ));
    // Keys are the prompt's while it is open.
    press(&mut app, KeyCode::Char('j'));
    update(&mut app, Msg::Term(TermMsg::Planned(Ok(fixed_plan()))));
    let prompt = app.term.prompt.as_ref().unwrap();
    assert!(matches!(
        prompt.phase,
        Phase::Choose {
            choice: Choice::Cancel,
            ..
        }
    ));
    assert_eq!(
        prompt.choices(),
        vec![Choice::Cancel, Choice::Worktree, Choice::Current]
    );
    let cmds = press(&mut app, KeyCode::Enter);
    assert!(cmds.is_empty(), "Enter on the default is No: {cmds:?}");
    assert!(app.term.prompt.is_none() && app.term.pane.is_none());
}

#[test]
fn confirming_creates_the_worktree_then_starts_the_child_with_the_change_in_its_environment() {
    let mut app = live_app();
    let id = app.selected_change().unwrap().id.clone();
    press(&mut app, KeyCode::Char('t'));
    let planned = fixed_plan_for(id.source_id.as_str(), &id.repo, id.number);
    update(&mut app, Msg::Term(TermMsg::Planned(Ok(planned))));
    press(&mut app, KeyCode::Right);
    let cmds = press(&mut app, KeyCode::Enter);
    let created = match term_cmds(&cmds).as_slice() {
        [TermCmd::CreateWorktree(p)] => p.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(created.steps.len(), 2);
    assert!(matches!(
        app.term.prompt.as_ref().unwrap().phase,
        Phase::Creating(_)
    ));

    let cmds = update(
        &mut app,
        Msg::Term(TermMsg::WorktreeReady(Ok(created.path.clone()))),
    );
    let (_, spec) = spawn_of(&cmds);
    assert_eq!(spec.cwd.as_deref(), Some(created.path.as_path()));
    let env: std::collections::BTreeMap<_, _> = spec.env.iter().cloned().collect();
    assert_eq!(env["RB_REPO"], id.repo);
    assert_eq!(env["RB_NUMBER"], id.number.to_string());
    assert_eq!(env["RB_SOURCE"], id.source_id.as_str());
    assert!(
        env["RB_URL"].ends_with(&format!("/{}/pull/{}", id.repo, id.number)),
        "{env:?}"
    );
    assert!(app.term.prompt.is_none());
    assert_eq!(app.term.started_in, "worktree");
    assert!(app.term.focused);
}

#[test]
fn a_failed_worktree_keeps_the_prompt_and_says_what_went_wrong() {
    let mut app = live_app();
    press(&mut app, KeyCode::Char('t'));
    update(&mut app, Msg::Term(TermMsg::Planned(Ok(fixed_plan()))));
    press(&mut app, KeyCode::Char('y'));
    update(
        &mut app,
        Msg::Term(TermMsg::WorktreeReady(Err(
            "`git fetch` failed: nope.".into()
        ))),
    );
    let prompt = app.term.prompt.as_ref().unwrap();
    match &prompt.phase {
        Phase::Choose { choice, failed, .. } => {
            assert_eq!(*choice, Choice::Cancel);
            assert!(failed.as_deref().unwrap().contains("nope"));
        }
        other => panic!("{other:?}"),
    }
    assert!(app.term.pane.is_none());
}

#[test]
fn without_a_clone_only_cancel_and_current_directory_are_offered() {
    let mut app = live_app();
    press(&mut app, KeyCode::Char('t'));
    update(
        &mut app,
        Msg::Term(TermMsg::Planned(Err(
            "Review Buddy didn't find a local clone.".into(),
        ))),
    );
    assert_eq!(
        app.term.prompt.as_ref().unwrap().choices(),
        vec![Choice::Cancel, Choice::Current]
    );
    press(&mut app, KeyCode::Char('y'));
    assert!(
        app.term.prompt.is_some(),
        "y does nothing without a worktree"
    );
    let cmds = press(&mut app, KeyCode::Char('c'));
    let (_, spec) = spawn_of(&cmds);
    assert_eq!(spec.cwd, None);
    assert_eq!(app.term.started_in, "current directory");
    assert_eq!(spec.env.len(), 4);
}

#[test]
fn start_current_skips_the_prompt_and_start_worktree_leaves_the_current_directory_out() {
    let mut app = live_app();
    app.term.settings.start = TerminalStart::Current;
    let cmds = press(&mut app, KeyCode::Char('t'));
    assert!(app.term.prompt.is_none());
    spawn_of(&cmds);

    let mut app = live_app();
    app.term.settings.start = TerminalStart::Worktree;
    press(&mut app, KeyCode::Char('t'));
    update(&mut app, Msg::Term(TermMsg::Planned(Ok(fixed_plan()))));
    assert_eq!(
        app.term.prompt.as_ref().unwrap().choices(),
        vec![Choice::Cancel, Choice::Worktree]
    );
}

#[test]
fn esc_cancels_the_prompt_and_nothing_is_started() {
    let mut app = live_app();
    press(&mut app, KeyCode::Char('t'));
    update(&mut app, Msg::Term(TermMsg::Planned(Ok(fixed_plan()))));
    let cmds = press(&mut app, KeyCode::Esc);
    assert!(cmds.is_empty() && app.term.prompt.is_none() && app.term.pane.is_none());
}

fn started(app: &mut App) -> u64 {
    app.term.settings.start = TerminalStart::Current;
    let cmds = press(app, KeyCode::Char('t'));
    spawn_of(&cmds).0
}

#[test]
fn typed_keys_become_write_commands_with_the_childs_modes_applied() {
    let mut app = live_app();
    let gen = started(&mut app);
    let write = |cmds: &[Cmd]| match term_cmds(cmds).as_slice() {
        [TermCmd::Write(b)] => b.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(write(&press(&mut app, KeyCode::Char('a'))), b"a");
    assert_eq!(write(&press(&mut app, KeyCode::Up)), b"\x1b[A");
    assert_eq!(
        write(&key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL)),
        [3]
    );
    update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen,
            bytes: b"\x1b[?1h".to_vec(),
        }),
    );
    assert_eq!(
        write(&press(&mut app, KeyCode::Up)),
        b"\x1bOA",
        "application cursor keys"
    );
}

#[test]
fn kitty_keys_are_used_only_when_the_host_reported_support_and_the_child_asked() {
    let mut app = live_app();
    let gen = started(&mut app);
    update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen,
            bytes: b"\x1b[>1u".to_vec(),
        }),
    );
    let enter = |app: &mut App| match term_cmds(&key(app, KeyCode::Enter, KeyModifiers::SHIFT))
        .as_slice()
    {
        [TermCmd::Write(b)] => b.clone(),
        other => panic!("{other:?}"),
    };
    app.kitty_keys = false;
    assert_eq!(enter(&mut app), b"\r");
    app.kitty_keys = true;
    assert_eq!(enter(&mut app), b"\x1b[13;2u");
}

#[test]
fn output_is_fed_to_the_emulator_and_queries_are_answered() {
    let mut app = live_app();
    let gen = started(&mut app);
    let cmds = update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen,
            bytes: b"hello\x1b[c".to_vec(),
        }),
    );
    assert!(screen_text(&app).starts_with("hello"));
    assert!(matches!(term_cmds(&cmds).as_slice(), [TermCmd::Write(r)] if r == b"\x1b[?6c"));
    let cmds = update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen,
            bytes: b"\x1b]52;c;aGk=\x07\x1b]0;mytool\x07".to_vec(),
        }),
    );
    assert!(matches!(term_cmds(&cmds).as_slice(), [TermCmd::Copy(t)] if t == "hi"));
    assert_eq!(app.term.pane.as_ref().unwrap().title(), Some("mytool"));
}

#[test]
fn output_from_a_closed_pane_is_ignored() {
    let mut app = live_app();
    let old = started(&mut app);
    chord(&mut app);
    press(&mut app, KeyCode::Char('x'));
    chord(&mut app);
    press(&mut app, KeyCode::Char('x'));
    assert!(app.term.pane.is_none());
    let cmds = update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen: old,
            bytes: b"late".to_vec(),
        }),
    );
    assert!(cmds.is_empty());
    let cmds = press(&mut app, KeyCode::Char('t'));
    let (new, _) = spawn_of(&cmds);
    assert_ne!(old, new);
    update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen: old,
            bytes: b"stale".to_vec(),
        }),
    );
    assert!(!screen_text(&app).contains("stale"));
}

#[test]
fn when_the_child_exits_the_pane_says_so_and_any_key_closes_it() {
    let mut app = live_app();
    let gen = started(&mut app);
    update(&mut app, Msg::Term(TermMsg::Exited { gen, code: Some(1) }));
    assert!(screen_text(&app).contains("process exited with status 1"));
    let cmds = press(&mut app, KeyCode::Char('z'));
    assert!(matches!(term_cmds(&cmds).as_slice(), [TermCmd::Close]));
    assert!(app.term.pane.is_none() && !app.term.focused);
}

#[test]
fn t_after_the_child_finished_starts_a_fresh_one() {
    let mut app = live_app();
    let gen = started(&mut app);
    update(&mut app, Msg::Term(TermMsg::Exited { gen, code: Some(0) }));
    let d = review_buddy::app::terminal::dock(&app).unwrap();
    click(&mut app, d.app.x + 3, d.app.y + 3, KeyModifiers::NONE);
    assert!(!app.term.focused);
    let cmds = press(&mut app, KeyCode::Char('t'));
    assert!(term_cmds(&cmds).iter().any(|c| matches!(c, TermCmd::Close)));
    assert!(term_cmds(&cmds)
        .iter()
        .any(|c| matches!(c, TermCmd::Spawn { .. })));
}

#[test]
fn a_failed_spawn_closes_the_pane_and_says_why() {
    let mut app = live_app();
    let gen = started(&mut app);
    update(
        &mut app,
        Msg::Term(TermMsg::SpawnFailed {
            gen,
            reason: "Couldn't start nope.".into(),
        }),
    );
    assert!(app.term.pane.is_none());
    assert_eq!(
        app.status.as_ref().unwrap().notice.text,
        "Couldn't start nope."
    );
}

#[test]
fn pastes_are_bracketed_only_when_the_child_asked() {
    let mut app = live_app();
    let gen = started(&mut app);
    let paste = |app: &mut App| match term_cmds(&update(app, Msg::Paste("a\nb".into()))).as_slice()
    {
        [TermCmd::Write(b)] => b.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(paste(&mut app), b"a\rb");
    update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen,
            bytes: b"\x1b[?2004h".to_vec(),
        }),
    );
    assert_eq!(paste(&mut app), b"\x1b[200~a\nb\x1b[201~");
}

#[test]
fn a_pane_resize_follows_the_terminal_and_reaches_the_child() {
    let mut app = live_app();
    started(&mut app);
    let (cols, rows) = app.term.pane.as_ref().unwrap().size();
    let d = review_buddy::app::terminal::dock(&app).unwrap();
    assert_eq!((cols, rows), (d.inner.width, d.inner.height));
    let cmds = update(&mut app, Msg::Resize(200, 50));
    let d = review_buddy::app::terminal::dock(&app).unwrap();
    assert!(matches!(
        term_cmds(&cmds).as_slice(),
        [TermCmd::Resize { cols, rows }] if (*cols, *rows) == (d.inner.width, d.inner.height)
    ));
    assert!(update(&mut app, Msg::Tick).is_empty());
}

fn mouse(
    app: &mut App,
    kind: MouseEventKind,
    column: u16,
    row: u16,
    mods: KeyModifiers,
) -> Vec<Cmd> {
    update(
        app,
        Msg::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: mods,
        }),
    )
}

fn click(app: &mut App, column: u16, row: u16, mods: KeyModifiers) -> Vec<Cmd> {
    mouse(
        app,
        MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        mods,
    )
}

#[test]
fn clicking_the_pane_focuses_it_and_clicking_the_app_gives_focus_back() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    press(&mut app, KeyCode::Char('t'));
    chord(&mut app);
    press(&mut app, KeyCode::Esc);
    let d = review_buddy::app::terminal::dock(&app).unwrap();
    click(&mut app, d.inner.x + 2, d.inner.y + 2, KeyModifiers::NONE);
    assert!(app.term.focused);
    click(&mut app, d.app.x + 3, d.app.y + 3, KeyModifiers::NONE);
    assert!(!app.term.focused);
}

#[test]
fn mouse_events_reach_a_child_that_asked_and_shift_bypasses_it() {
    let mut app = live_app();
    let gen = started(&mut app);
    update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen,
            bytes: b"\x1b[?1000h\x1b[?1006h".to_vec(),
        }),
    );
    let d = review_buddy::app::terminal::dock(&app).unwrap();
    let cmds = click(&mut app, d.inner.x + 4, d.inner.y + 1, KeyModifiers::NONE);
    assert!(
        matches!(term_cmds(&cmds).as_slice(), [TermCmd::Write(b)] if b == b"\x1b[<0;5;2M"),
        "{cmds:?}"
    );
    let cmds = click(&mut app, d.inner.x + 4, d.inner.y + 1, KeyModifiers::SHIFT);
    assert!(
        term_cmds(&cmds).is_empty(),
        "shift stays with the host terminal"
    );
    let cmds = mouse(
        &mut app,
        MouseEventKind::ScrollDown,
        d.inner.x,
        d.inner.y,
        KeyModifiers::NONE,
    );
    assert!(matches!(term_cmds(&cmds).as_slice(), [TermCmd::Write(b)] if b == b"\x1b[<65;1;1M"));
}

#[test]
fn the_wheel_scrolls_history_when_the_child_is_not_listening() {
    let mut app = live_app();
    let gen = started(&mut app);
    let lines: String = (0..80).map(|n| format!("line {n}\r\n")).collect();
    update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen,
            bytes: lines.into_bytes(),
        }),
    );
    let d = review_buddy::app::terminal::dock(&app).unwrap();
    mouse(
        &mut app,
        MouseEventKind::ScrollUp,
        d.inner.x,
        d.inner.y,
        KeyModifiers::NONE,
    );
    assert_eq!(app.term.pane.as_ref().unwrap().screen().scrolled, 3);
    mouse(
        &mut app,
        MouseEventKind::ScrollDown,
        d.inner.x,
        d.inner.y,
        KeyModifiers::NONE,
    );
    assert_eq!(app.term.pane.as_ref().unwrap().screen().scrolled, 0);
    let frame = text(&render_support::render(&mut app));
    assert!(frame.contains("line 79"), "{frame}");
}

#[test]
fn the_wheel_becomes_arrow_keys_on_the_alternate_screen() {
    let mut app = live_app();
    let gen = started(&mut app);
    update(
        &mut app,
        Msg::Term(TermMsg::Output {
            gen,
            bytes: b"\x1b[?1049h".to_vec(),
        }),
    );
    let d = review_buddy::app::terminal::dock(&app).unwrap();
    let cmds = mouse(
        &mut app,
        MouseEventKind::ScrollUp,
        d.inner.x,
        d.inner.y,
        KeyModifiers::NONE,
    );
    assert!(
        matches!(term_cmds(&cmds).as_slice(), [TermCmd::Write(b)] if b == b"\x1b[A\x1b[A\x1b[A")
    );
}

#[test]
fn dragging_the_seam_resizes_the_pane_and_a_double_click_resets_it() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    press(&mut app, KeyCode::Char('t'));
    let d = review_buddy::app::terminal::dock(&app).unwrap();
    assert_eq!(d.side, review_buddy::ui::terminal::Side::Right);
    let (x, y) = (d.boundary, d.hit.y + 3);
    click(&mut app, x, y, KeyModifiers::NONE);
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        x - 10,
        y,
        KeyModifiers::NONE,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        x - 10,
        y,
        KeyModifiers::NONE,
    );
    assert_eq!(app.term.size, Some(Size::Cells(d.outer.width + 10)));
    let grown = review_buddy::app::terminal::dock(&app).unwrap();
    assert_eq!(
        app.term.pane.as_ref().unwrap().size(),
        (grown.inner.width, grown.inner.height)
    );
    let x = grown.boundary;
    click(&mut app, x, y, KeyModifiers::NONE);
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        x,
        y,
        KeyModifiers::NONE,
    );
    click(&mut app, x, y, KeyModifiers::NONE);
    assert_eq!(app.term.size, None, "double-click goes back to automatic");
}

#[test]
fn chord_keys_move_and_size_the_pane_and_the_placement_is_remembered() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    press(&mut app, KeyCode::Char('t'));
    assert_eq!(
        app.session_snapshot().terminal,
        (DetailPosition::Auto, None)
    );
    chord(&mut app);
    press(&mut app, KeyCode::Char('p'));
    assert_eq!(app.term.position, DetailPosition::Bottom);
    assert_eq!(
        review_buddy::app::terminal::dock(&app).unwrap().side,
        review_buddy::ui::terminal::Side::Bottom
    );
    let before = review_buddy::app::terminal::dock(&app)
        .unwrap()
        .outer
        .height;
    chord(&mut app);
    press(&mut app, KeyCode::Char('>'));
    assert_eq!(
        review_buddy::app::terminal::dock(&app)
            .unwrap()
            .outer
            .height,
        before + 2
    );
    chord(&mut app);
    press(&mut app, KeyCode::Char('='));
    assert_eq!(app.term.size, None);
    assert_eq!(app.session_snapshot().terminal.0, DetailPosition::Bottom);
}

#[test]
fn the_pane_gives_way_when_there_is_no_room_and_comes_back() {
    let mut app = dashboard("liminal-hq", (100, 30), true);
    press(&mut app, KeyCode::Char('t'));
    assert!(app.term.visible);
    update(&mut app, Msg::Resize(100, 20));
    assert!(!app.term.visible && !app.term.focused && app.term.pane.is_some());
    assert!(app
        .status
        .as_ref()
        .unwrap()
        .notice
        .text
        .contains("isn't room"));
    update(&mut app, Msg::Resize(100, 30));
    press(&mut app, KeyCode::Char('t'));
    assert!(app.term.visible && app.term.focused);
}

#[test]
fn t_without_a_selected_change_says_what_to_do() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    app.dashboard.selected = None;
    let cmds = press(&mut app, KeyCode::Char('t'));
    assert!(cmds.iter().all(|c| !matches!(c, Cmd::Term(_))));
    assert!(app
        .status
        .as_ref()
        .unwrap()
        .notice
        .text
        .contains("Select a change first"));
    assert!(app.term.pane.is_none());
}

fn frame(theme: &str, size: (u16, u16)) -> String {
    let mut app = dashboard(theme, size, true);
    press(&mut app, KeyCode::Char('t'));
    type_text(&mut app, "git log");
    press(&mut app, KeyCode::Enter);
    text(&render(&mut app))
}

#[test]
fn frames_with_the_scripted_pane() {
    for theme in ["liminal-hq", "dusk"] {
        for (w, h) in SIZES {
            insta::assert_snapshot!(
                format!("pane_{}_{w}x{h}", theme.replace('-', "_")),
                frame(theme, (w, h))
            );
        }
    }
}

#[test]
fn frame_with_the_chord_half_typed() {
    let mut app = dashboard("liminal-hq", (160, 40), true);
    press(&mut app, KeyCode::Char('t'));
    chord(&mut app);
    insta::assert_snapshot!("pane_armed_160x40", text(&render(&mut app)));
}

#[cfg(unix)]
#[test]
fn frames_with_the_start_prompt() {
    for (w, h) in SIZES {
        let mut app = dashboard("liminal-hq", (w, h), false);
        app.term.settings = TerminalSettings::from_config(&TerminalConfig::default())
            .with_worktrees_root("/state/review-buddy/worktrees".into());
        press(&mut app, KeyCode::Char('t'));
        update(&mut app, Msg::Term(TermMsg::Planned(Ok(fixed_plan()))));
        insta::assert_snapshot!(format!("prompt_worktree_{w}x{h}"), text(&render(&mut app)));
    }
    let mut app = dashboard("dusk", (160, 40), false);
    app.term.settings = TerminalSettings::from_config(&TerminalConfig::default())
        .with_worktrees_root("/state/review-buddy/worktrees".into());
    press(&mut app, KeyCode::Char('t'));
    update(
        &mut app,
        Msg::Term(TermMsg::Planned(Err(
            "Review Buddy didn't find a local clone of acme/widgets. Start it from inside a clone, or list one under [ui.terminal.checkouts] in your config. Starting in the current directory still works.".into(),
        ))),
    );
    insta::assert_snapshot!("prompt_no_clone_160x40", text(&render(&mut app)));
}
