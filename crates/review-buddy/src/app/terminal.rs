//! The terminal pane: state, focus and the pure transitions that drive it.
//!
//! `rb-term` owns the emulator, the encoders and the focus chord. This module wires them into
//! `update`: output arrives as [`TermMsg`]s and is fed to the emulator, keys while the pane has
//! focus become [`TermCmd::Write`], and everything that touches the system (planning and creating
//! a worktree, the PTY itself) leaves as a [`TermCmd`] for the runtime. In demo mode nothing leaves:
//! the pane is the scripted transcript from `rb-term`, driven entirely in memory.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use rb_core::ChangeId;
use rb_term::worktree::{self, Plan};
use rb_term::{
    encode_key, encode_mouse, Chord, ColorScheme, EscapeAction, EscapeState, KeyContext,
    Pane as TermPane, PaneKind, ScriptContext, SpawnSpec,
};
use rb_theme::{Colour, Role};

use super::update::set_status;
use super::{Action, App, Cmd, Notice, NoticeKind, Screen};
use crate::config::{DetailPosition, TerminalConfig, TerminalStart};
use crate::ui::layout::Size;
use crate::ui::terminal as geometry;

/// Two presses on the seam this many ticks apart (a tick is 250 ms) reset the pane's size.
const DOUBLE_CLICK_TICKS: u64 = 2;
const NUDGE: i32 = 2;

/// Effects for the runtime. None of them runs inside `update`.
#[derive(Debug, Clone)]
pub enum TermCmd {
    /// Find a local clone of the change's repository and plan a worktree there.
    Plan {
        launch: Launch,
        checkouts: BTreeMap<String, String>,
        root: PathBuf,
    },
    /// Run the plan's git commands.
    CreateWorktree(Plan),
    /// Start the child. `gen` tags its output, so a closed pane's stragglers are ignored.
    Spawn {
        gen: u64,
        spec: SpawnSpec,
    },
    Write(Vec<u8>),
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Stop the child and drop the PTY.
    Close,
    /// Put text the child asked to copy (OSC 52) on the clipboard.
    Copy(String),
}

/// Results for `update`.
#[derive(Debug, Clone)]
pub enum TermMsg {
    /// The answer to [`TermCmd::Plan`]: a plan, or why a worktree can't be made.
    Planned(Result<Plan, String>),
    /// The answer to [`TermCmd::CreateWorktree`].
    WorktreeReady(Result<PathBuf, String>),
    Output {
        gen: u64,
        bytes: Vec<u8>,
    },
    Exited {
        gen: u64,
        code: Option<u32>,
    },
    SpawnFailed {
        gen: u64,
        reason: String,
    },
}

/// Something a mouse click or a key hint can trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermAction {
    /// `t`.
    Toggle,
    /// Press a button in the start prompt.
    Answer(Choice),
}

/// The change the pane was opened for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub id: ChangeId,
    pub url: String,
}

impl Launch {
    fn env(&self) -> Vec<(String, String)> {
        worktree::env_for(
            self.id.source_id.as_str(),
            &self.id.repo,
            self.id.number,
            &self.url,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Cancel,
    Worktree,
    Current,
}

#[derive(Debug, Clone)]
pub enum Phase {
    /// Looking for a clone.
    Planning,
    /// Showing the preview. `plan` is the worktree, or the reason there isn't one.
    Choose {
        plan: Result<Plan, String>,
        choice: Choice,
        /// Why the last attempt failed, shown under the preview.
        failed: Option<String>,
    },
    /// Running git.
    Creating(Plan),
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub launch: Launch,
    pub phase: Phase,
    /// `ui.terminal.start = "worktree"` leaves the current directory out.
    pub current_allowed: bool,
}

impl Prompt {
    /// The buttons on offer, safest first.
    pub fn choices(&self) -> Vec<Choice> {
        let mut out = vec![Choice::Cancel];
        if matches!(&self.phase, Phase::Choose { plan: Ok(_), .. }) {
            out.push(Choice::Worktree);
        }
        if self.current_allowed && matches!(&self.phase, Phase::Choose { .. }) {
            out.push(Choice::Current);
        }
        out
    }
}

/// What `[ui.terminal]` and the launch environment say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSettings {
    pub command: Vec<String>,
    pub chord: Chord,
    pub start: TerminalStart,
    pub scrollback: usize,
    pub checkouts: BTreeMap<String, String>,
    /// The folder managed worktrees go in. `None` in demo mode and when there is no state dir.
    pub worktrees_root: Option<PathBuf>,
    /// A problem with the configured escape chord, to say once the pane opens.
    pub chord_problem: Option<String>,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self::from_config(&TerminalConfig::default())
    }
}

impl TerminalSettings {
    pub fn from_config(config: &TerminalConfig) -> Self {
        let (chord, chord_problem) = match Chord::parse(&config.escape) {
            Ok(chord) => (chord, None),
            Err(err) => (Chord::default(), Some(err.to_string())),
        };
        Self {
            command: config.command.clone(),
            chord,
            start: config.start,
            scrollback: config.scrollback.clamp(100, 200_000),
            checkouts: config.checkouts.clone(),
            worktrees_root: None,
            chord_problem,
        }
    }

    pub fn with_worktrees_root(mut self, root: PathBuf) -> Self {
        self.worktrees_root = Some(root);
        self
    }
}

/// The pane's whole state. `None` for the pane until the first `t`.
#[derive(Debug)]
pub struct TerminalState {
    pub settings: TerminalSettings,
    pub pane: Option<TermPane>,
    /// The change the running pane belongs to.
    pub launch: Option<Launch>,
    /// Where the pane started, for its title: `worktree`, `current directory` or `demo`.
    pub started_in: &'static str,
    /// The pane is on screen (it keeps running when it isn't).
    pub visible: bool,
    /// Keys go to the child.
    pub focused: bool,
    pub escape: EscapeState,
    pub prompt: Option<Prompt>,
    /// Where it sits and how big, as changed this session.
    pub position: DetailPosition,
    pub size: Option<Size>,
    pub(crate) gen: u64,
    pub(crate) drag: Option<Drag>,
    last_seam_click: Option<u64>,
    close_armed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drag {
    /// The boundary minus the pointer where the press landed, so the seam doesn't jump.
    offset: i32,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            settings: TerminalSettings::default(),
            pane: None,
            launch: None,
            started_in: "",
            visible: false,
            focused: false,
            escape: EscapeState::default(),
            prompt: None,
            position: DetailPosition::Auto,
            size: None,
            gen: 0,
            drag: None,
            last_seam_click: None,
            close_armed: false,
        }
    }
}

impl TerminalState {
    /// Whether the pane is drawn and keys can reach it on this screen.
    pub fn shown(&self, screen: Screen) -> bool {
        self.visible && self.pane.is_some() && matches!(screen, Screen::Dashboard | Screen::Diff)
    }

    /// The pane is focused and on screen.
    pub fn has_keys(&self, screen: Screen) -> bool {
        self.focused && self.shown(screen)
    }
}

/// Where the pane sits now, if it is on screen and there is room.
pub fn dock(app: &App) -> Option<geometry::Dock> {
    if !app.term.shown(app.screen) {
        return None;
    }
    geometry::dock(
        crate::ui::layout::body(app.size),
        app.term.position,
        app.term.size,
    )
}

/// The area the app's own screens use: the body, less the pane when it is on screen.
pub fn app_body(app: &App) -> Rect {
    match dock(app) {
        Some(d) => d.app,
        None => crate::ui::layout::body(app.size),
    }
}

fn launch_for(app: &App) -> Option<Launch> {
    let id = match (app.screen, app.diff.as_ref()) {
        (Screen::Diff, Some(state)) => state.id.clone(),
        _ => app.selected_change()?.id.clone(),
    };
    let url = super::links::change_url(app, &id).unwrap_or_default();
    Some(Launch { id, url })
}

fn scheme(app: &App) -> ColorScheme {
    let rgb = |c: Option<Colour>| match c? {
        Colour::Rgb(rgb) => Some((rgb.r, rgb.g, rgb.b)),
        Colour::Indexed(i) => {
            let rgb = rb_theme::indexed_rgb(i);
            Some((rgb.r, rgb.g, rgb.b))
        }
        _ => None,
    };
    let base = ColorScheme::default();
    ColorScheme {
        foreground: rgb(app.palette.colour(Role::Text)).unwrap_or(base.foreground),
        background: rgb(app.palette.background()).unwrap_or(base.background),
        ansi: base.ansi,
    }
}

fn say(app: &mut App, kind: NoticeKind, text: impl Into<String>) -> Vec<Cmd> {
    set_status(app, Notice::new(kind, text))
}

fn cmd(c: TermCmd) -> Cmd {
    Cmd::Term(c)
}

/// `t`: opens the pane, brings an open one back into focus, or starts a fresh one when the last
/// child has finished.
pub fn toggle(app: &mut App) -> Vec<Cmd> {
    if app.term.prompt.is_some() {
        return Vec::new();
    }
    if app.term.pane.as_ref().is_some_and(|p| p.exited().is_some()) {
        let mut cmds = close(app);
        cmds.extend(open(app));
        return cmds;
    }
    if app.term.pane.is_some() {
        app.term.visible = true;
        app.term.focused = true;
        app.mark_dirty();
        return Vec::new();
    }
    open(app)
}

fn open(app: &mut App) -> Vec<Cmd> {
    let Some(launch) = launch_for(app) else {
        return say(
            app,
            NoticeKind::Info,
            "Select a change first, then press t to open a terminal next to it.",
        );
    };
    if app.demo {
        start_scripted(app, launch);
        return Vec::new();
    }
    let root = app.term.settings.worktrees_root.clone();
    match (app.term.settings.start, root) {
        (TerminalStart::Current, _) | (_, None) => {
            start_live(app, launch, None, "current directory")
        }
        (_, Some(root)) => {
            let cmds = vec![cmd(TermCmd::Plan {
                launch: launch.clone(),
                checkouts: app.term.settings.checkouts.clone(),
                root,
            })];
            app.term.prompt = Some(Prompt {
                launch,
                phase: Phase::Planning,
                current_allowed: app.term.settings.start != TerminalStart::Worktree,
            });
            app.mark_dirty();
            cmds
        }
    }
}

/// Where the pane would be drawn, or a plain default when there is no room yet.
fn pane_size(app: &App) -> (u16, u16) {
    let body = crate::ui::layout::body(app.size);
    geometry::dock(body, app.term.position, app.term.size)
        .map_or((80, 24), |d| (d.inner.width.max(2), d.inner.height.max(1)))
}

fn start_scripted(app: &mut App, launch: Launch) {
    let (cols, rows) = pane_size(app);
    let ctx = ScriptContext {
        source: launch.id.source_id.as_str().to_string(),
        repo: launch.id.repo.clone(),
        number: launch.id.number,
        url: launch.url.clone(),
    };
    let mut pane = TermPane::scripted(cols, rows, app.term.settings.scrollback, ctx);
    pane.emulator_mut().set_scheme(scheme(app));
    app.term.pane = Some(pane);
    app.term.launch = Some(launch);
    app.term.started_in = "demo";
    show(app);
}

fn show(app: &mut App) {
    app.term.visible = true;
    app.term.focused = true;
    app.term.escape.reset();
    app.term.close_armed = false;
    app.mark_dirty();
}

fn start_live(
    app: &mut App,
    launch: Launch,
    cwd: Option<PathBuf>,
    started_in: &'static str,
) -> Vec<Cmd> {
    let (cols, rows) = pane_size(app);
    let mut pane = TermPane::live(cols, rows, app.term.settings.scrollback);
    pane.emulator_mut().set_scheme(scheme(app));
    let (program, args) = match app.term.settings.command.split_first() {
        Some((program, args)) => (program.clone(), args.to_vec()),
        None => (
            rb_term::default_shell(&|k| std::env::var(k).ok()),
            Vec::new(),
        ),
    };
    app.term.gen += 1;
    let spec = SpawnSpec {
        program,
        args,
        cwd,
        env: launch.env(),
        cols,
        rows,
    };
    app.term.pane = Some(pane);
    app.term.launch = Some(launch);
    app.term.started_in = started_in;
    show(app);
    let mut cmds = vec![cmd(TermCmd::Spawn {
        gen: app.term.gen,
        spec,
    })];
    if let Some(problem) = app.term.settings.chord_problem.take() {
        cmds.extend(say(
            app,
            NoticeKind::Warning,
            format!("{problem}. Using ⌃\\ for now."),
        ));
    }
    cmds
}

pub fn on_msg(app: &mut App, msg: TermMsg) -> Vec<Cmd> {
    match msg {
        TermMsg::Planned(result) => {
            let Some(prompt) = app.term.prompt.as_mut() else {
                return Vec::new();
            };
            if matches!(prompt.phase, Phase::Planning) {
                prompt.phase = Phase::Choose {
                    plan: result,
                    choice: Choice::Cancel,
                    failed: None,
                };
                app.mark_dirty();
            }
            Vec::new()
        }
        TermMsg::WorktreeReady(result) => worktree_ready(app, result),
        TermMsg::Output { gen, bytes } if gen == app.term.gen => output(app, &bytes),
        TermMsg::Exited { gen, code } if gen == app.term.gen => {
            if let Some(pane) = app.term.pane.as_mut() {
                pane.mark_exited(code);
            }
            app.mark_dirty();
            if app.term.shown(app.screen) {
                return Vec::new();
            }
            say(
                app,
                NoticeKind::Info,
                "The terminal finished. Press t to close it.",
            )
        }
        TermMsg::SpawnFailed { gen, reason } if gen == app.term.gen => {
            let mut cmds = close(app);
            cmds.extend(say(app, NoticeKind::Warning, reason));
            cmds
        }
        _ => Vec::new(),
    }
}

fn output(app: &mut App, bytes: &[u8]) -> Vec<Cmd> {
    let Some(pane) = app.term.pane.as_mut() else {
        return Vec::new();
    };
    let out = pane.feed(bytes);
    app.mark_dirty();
    let mut cmds = Vec::new();
    if !out.reply.is_empty() {
        cmds.push(cmd(TermCmd::Write(out.reply)));
    }
    for event in out.events {
        if let rb_term::Event::Clipboard(text) = event {
            cmds.push(cmd(TermCmd::Copy(text)));
        }
    }
    cmds
}

fn worktree_ready(app: &mut App, result: Result<PathBuf, String>) -> Vec<Cmd> {
    let Some(prompt) = app.term.prompt.take() else {
        return Vec::new();
    };
    let Phase::Creating(plan) = prompt.phase else {
        app.term.prompt = Some(prompt);
        return Vec::new();
    };
    match result {
        Ok(dir) => {
            app.mark_dirty();
            start_live(app, prompt.launch, Some(dir), "worktree")
        }
        Err(reason) => {
            app.term.prompt = Some(Prompt {
                launch: prompt.launch,
                phase: Phase::Choose {
                    plan: Ok(plan),
                    choice: Choice::Cancel,
                    failed: Some(reason),
                },
                current_allowed: prompt.current_allowed,
            });
            app.mark_dirty();
            Vec::new()
        }
    }
}

/// Stops the child and removes the pane.
pub fn close(app: &mut App) -> Vec<Cmd> {
    let had = app.term.pane.take().is_some();
    app.term.launch = None;
    app.term.visible = false;
    app.term.focused = false;
    app.term.escape.reset();
    app.term.close_armed = false;
    app.term.gen += 1;
    app.mark_dirty();
    if had {
        vec![cmd(TermCmd::Close)]
    } else {
        Vec::new()
    }
}

/// Runs `Action::Terminal`.
pub fn run(app: &mut App, action: TermAction) -> Vec<Cmd> {
    match action {
        TermAction::Toggle => toggle(app),
        TermAction::Answer(choice) => answer(app, choice),
    }
}

fn answer(app: &mut App, choice: Choice) -> Vec<Cmd> {
    let Some(prompt) = app.term.prompt.take() else {
        return Vec::new();
    };
    app.mark_dirty();
    match (choice, prompt.phase) {
        (Choice::Cancel, _) => Vec::new(),
        (Choice::Worktree, Phase::Choose { plan: Ok(plan), .. }) => {
            app.term.prompt = Some(Prompt {
                launch: prompt.launch,
                phase: Phase::Creating(plan.clone()),
                current_allowed: prompt.current_allowed,
            });
            vec![cmd(TermCmd::CreateWorktree(plan))]
        }
        (Choice::Current, Phase::Choose { .. }) => {
            start_live(app, prompt.launch, None, "current directory")
        }
        (_, phase) => {
            app.term.prompt = Some(Prompt {
                launch: prompt.launch,
                phase,
                current_allowed: prompt.current_allowed,
            });
            Vec::new()
        }
    }
}

fn prompt_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && key.code == KeyCode::Char('c') {
        app.term.prompt = None;
        app.mark_dirty();
        return Vec::new();
    }
    let Some(prompt) = app.term.prompt.as_mut() else {
        return Vec::new();
    };
    let choices = prompt.choices();
    let Phase::Choose { choice, .. } = &mut prompt.phase else {
        if key.code == KeyCode::Esc && matches!(prompt.phase, Phase::Planning) {
            app.term.prompt = None;
            app.mark_dirty();
        }
        return Vec::new();
    };
    let at = choices.iter().position(|c| c == choice).unwrap_or(0);
    let step =
        |delta: isize| choices[(at as isize + delta).rem_euclid(choices.len() as isize) as usize];
    match key.code {
        KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => *choice = step(-1),
        KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => *choice = step(1),
        KeyCode::Enter => {
            let picked = *choice;
            return answer(app, picked);
        }
        KeyCode::Char('y' | 'w') if choices.contains(&Choice::Worktree) => {
            return answer(app, Choice::Worktree);
        }
        KeyCode::Char('c') if choices.contains(&Choice::Current) => {
            return answer(app, Choice::Current);
        }
        KeyCode::Esc | KeyCode::Char('n' | 'q') => return answer(app, Choice::Cancel),
        _ => return Vec::new(),
    }
    app.mark_dirty();
    Vec::new()
}

/// Keys that belong to the terminal feature before anything else sees them: the start prompt, and
/// the child while the pane has focus. `None` leaves the key to the app.
pub fn on_key(app: &mut App, key: KeyEvent) -> Option<Vec<Cmd>> {
    if app.term.prompt.is_some() {
        if key.kind == KeyEventKind::Release {
            return Some(Vec::new());
        }
        return Some(prompt_key(app, key));
    }
    if !app.term.has_keys(app.screen) {
        return None;
    }
    Some(pane_key(app, key))
}

fn pane_key(app: &mut App, key: KeyEvent) -> Vec<Cmd> {
    if app.term.pane.as_ref().is_some_and(|p| p.exited().is_some()) {
        return if key.kind == KeyEventKind::Release {
            Vec::new()
        } else {
            close(app)
        };
    }
    let chord = app.term.settings.chord;
    let action = app.term.escape.on_key(&key, &chord, app.ticks);
    app.mark_dirty();
    match action {
        EscapeAction::Forward => write_key(app, &key),
        EscapeAction::Swallow => Vec::new(),
        EscapeAction::SendChord => {
            let literal = KeyEvent::new(chord.code, chord.mods);
            write_key(app, &literal)
        }
        EscapeAction::Leave => {
            app.term.focused = false;
            say(
                app,
                NoticeKind::Info,
                "Back in the app. Press t to return to the terminal.",
            )
        }
        EscapeAction::Hide => {
            app.term.visible = false;
            app.term.focused = false;
            say(
                app,
                NoticeKind::Info,
                "Terminal hidden. It keeps running; press t to bring it back.",
            )
        }
        EscapeAction::Close => close_asked(app),
        EscapeAction::CyclePlacement => {
            app.term.position = next_position(app.term.position);
            let text = format!("Terminal placement: {}", app.term.position.as_str());
            say(app, NoticeKind::Info, text)
        }
        EscapeAction::Grow => nudge(app, NUDGE),
        EscapeAction::Shrink => nudge(app, -NUDGE),
        EscapeAction::ResetSize => {
            app.term.size = None;
            say(app, NoticeKind::Info, "Terminal size: automatic")
        }
    }
}

fn close_asked(app: &mut App) -> Vec<Cmd> {
    let running = app
        .term
        .pane
        .as_ref()
        .is_some_and(|p| p.kind() == PaneKind::Live && p.exited().is_none());
    if running && !app.term.close_armed {
        app.term.close_armed = true;
        let chord = app.term.settings.chord.label();
        return say(
            app,
            NoticeKind::Warning,
            format!("The terminal is still running. Press {chord} then x again to stop it, or {chord} then esc to leave it running."),
        );
    }
    let mut cmds = close(app);
    cmds.extend(say(app, NoticeKind::Info, "Terminal closed."));
    cmds
}

fn write_key(app: &mut App, key: &KeyEvent) -> Vec<Cmd> {
    let Some(pane) = app.term.pane.as_mut() else {
        return Vec::new();
    };
    let ctx = KeyContext::new(&pane.modes(), app.kitty_keys);
    let bytes = encode_key(key, &ctx);
    send(app, bytes)
}

fn send(app: &mut App, bytes: Vec<u8>) -> Vec<Cmd> {
    let Some(pane) = app.term.pane.as_mut() else {
        return Vec::new();
    };
    app.term.close_armed = false;
    let out = pane.write(&bytes);
    app.mark_dirty();
    if out.is_empty() {
        Vec::new()
    } else {
        vec![cmd(TermCmd::Write(out))]
    }
}

/// Whether a paste belongs to the child.
pub fn on_paste_wanted(app: &App) -> bool {
    app.term.has_keys(app.screen)
}

/// A paste while the pane has focus goes to the child, bracketed if it asked for that.
pub fn on_paste(app: &mut App, text: &str) -> Option<Vec<Cmd>> {
    if !app.term.has_keys(app.screen) {
        return None;
    }
    let bracketed = app.term.pane.as_ref()?.modes().bracketed_paste;
    Some(send(app, rb_term::keys::encode_paste(text, bracketed)))
}

/// The ring `P` uses for the panes: auto, bottom, left, top, right.
fn next_position(now: DetailPosition) -> DetailPosition {
    match now {
        DetailPosition::Auto => DetailPosition::Bottom,
        DetailPosition::Bottom => DetailPosition::Left,
        DetailPosition::Left => DetailPosition::Top,
        DetailPosition::Top => DetailPosition::Right,
        DetailPosition::Right => DetailPosition::Auto,
    }
}

fn nudge(app: &mut App, delta: i32) -> Vec<Cmd> {
    let Some(d) = dock(app) else {
        return Vec::new();
    };
    let now = if d.side.vertical_seam() {
        d.outer.width
    } else {
        d.outer.height
    };
    let next = (i32::from(now) + delta).clamp(1, i32::from(u16::MAX)) as u16;
    app.term.size = Some(Size::Cells(next));
    let now = dock(app).map_or(next, |d| {
        if d.side.vertical_seam() {
            d.outer.width
        } else {
            d.outer.height
        }
    });
    let unit = if matches!(d.side, geometry::Side::Left | geometry::Side::Right) {
        "columns"
    } else {
        "rows"
    };
    say(app, NoticeKind::Info, format!("Terminal {now} {unit}"))
}

/// Mouse input for the pane, its seam and the start prompt. `None` leaves it to the app.
pub fn on_mouse(app: &mut App, mouse: MouseEvent) -> Option<Vec<Cmd>> {
    if app.term.prompt.is_some() {
        let on_button = matches!(
            app.hits.at(mouse.column, mouse.row),
            Some(Action::Terminal(_) | Action::DismissToast(_))
        );
        return if on_button { None } else { Some(Vec::new()) };
    }
    let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
    let d = dock(app)?;
    let left = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left));
    if let Some(drag) = app.term.drag {
        return Some(match mouse.kind {
            MouseEventKind::Drag(MouseButton::Left) => drag_seam(app, &d, drag, mouse),
            MouseEventKind::Up(MouseButton::Left) => {
                app.term.drag = None;
                Vec::new()
            }
            _ => Vec::new(),
        });
    }
    let inside = |r: Rect| {
        mouse.column >= r.x
            && mouse.column < r.right()
            && mouse.row >= r.y
            && mouse.row < r.bottom()
    };
    if left && !shift && inside(d.hit) {
        return Some(press_seam(app, &d, mouse));
    }
    if !inside(d.outer) {
        if left && app.term.focused {
            app.term.focused = false;
            app.mark_dirty();
        }
        return None;
    }
    if left && !app.term.focused {
        app.term.focused = true;
        app.mark_dirty();
        if !inside(d.inner) {
            return Some(Vec::new());
        }
    }
    if !inside(d.inner) || shift {
        return Some(Vec::new());
    }
    let pane = app.term.pane.as_mut()?;
    let modes = pane.modes();
    let (col, row) = (mouse.column - d.inner.x, mouse.row - d.inner.y);
    if modes.mouse != rb_term::MouseMode::Off {
        let bytes = encode_mouse(&mouse, col, row, &modes)?;
        return Some(send(app, bytes));
    }
    match mouse.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let up = mouse.kind == MouseEventKind::ScrollUp;
            if modes.alt_screen && modes.alternate_scroll {
                let wheel = if up {
                    rb_term::Wheel::Up
                } else {
                    rb_term::Wheel::Down
                };
                let bytes = rb_term::mouse::wheel_as_arrows(wheel, modes.app_cursor);
                return Some(send(app, bytes));
            }
            pane.emulator_mut().scroll(if up { 3 } else { -3 });
            app.mark_dirty();
            Some(Vec::new())
        }
        _ => Some(Vec::new()),
    }
}

fn press_seam(app: &mut App, d: &geometry::Dock, mouse: MouseEvent) -> Vec<Cmd> {
    let double = app
        .term
        .last_seam_click
        .is_some_and(|tick| app.ticks.saturating_sub(tick) <= DOUBLE_CLICK_TICKS);
    if double {
        app.term.last_seam_click = None;
        app.term.drag = None;
        app.term.size = None;
        return say(app, NoticeKind::Info, "Terminal size: automatic");
    }
    app.term.last_seam_click = Some(app.ticks);
    let pointer = if d.side.vertical_seam() {
        mouse.column
    } else {
        mouse.row
    };
    app.term.drag = Some(Drag {
        offset: i32::from(d.boundary) - i32::from(pointer),
    });
    app.mark_dirty();
    Vec::new()
}

fn drag_seam(app: &mut App, d: &geometry::Dock, drag: Drag, mouse: MouseEvent) -> Vec<Cmd> {
    let pointer = if d.side.vertical_seam() {
        mouse.column
    } else {
        mouse.row
    };
    let boundary = (i32::from(pointer) + drag.offset).clamp(0, i32::from(u16::MAX)) as u16;
    let body = crate::ui::layout::body(app.size);
    let cells = geometry::size_for(d.side, body, boundary);
    app.term.size = Some(Size::Cells(cells.max(1)));
    app.mark_dirty();
    Vec::new()
}

/// Run after every `update`: keeps the pane's grid the size of its box, and lets a half-typed
/// chord expire.
pub fn after(app: &mut App) -> Vec<Cmd> {
    let cmds = fit_pane(app);
    // The pane opening, closing or resizing changes the diff's width, so its lines re-wrap.
    if app.screen == Screen::Diff {
        super::diff::refit(app);
    }
    cmds
}

fn fit_pane(app: &mut App) -> Vec<Cmd> {
    app.term.escape.tick(app.ticks);
    if app.term.pane.is_none() {
        return Vec::new();
    }
    let wanted = app.term.visible && matches!(app.screen, Screen::Dashboard | Screen::Diff);
    if wanted && dock(app).is_none() {
        app.term.visible = false;
        app.term.focused = false;
        return say(
            app,
            NoticeKind::Info,
            "There isn't room for the terminal at this size. Make the window bigger and press t.",
        );
    }
    let Some(d) = dock(app) else {
        return Vec::new();
    };
    let (cols, rows) = (d.inner.width.max(2), d.inner.height.max(1));
    let Some(pane) = app.term.pane.as_mut() else {
        return Vec::new();
    };
    if pane.size() == (cols, rows) {
        return Vec::new();
    }
    pane.resize(cols, rows);
    let live = pane.kind() == PaneKind::Live;
    app.mark_dirty();
    if live {
        vec![cmd(TermCmd::Resize { cols, rows })]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn terminal(toml: &str) -> TerminalConfig {
        toml::from_str::<Config>(toml).unwrap().ui.terminal
    }

    #[test]
    fn ui_terminal_parses_every_key() {
        let t = terminal(
            r#"[ui.terminal]
command = ["claude", "--resume"]
escape = "ctrl-]"
position = "left"
size = "35%"
scrollback = 500
start = "worktree"
[ui.terminal.checkouts]
"acme/widgets" = "~/src/widgets"
"#,
        );
        assert_eq!(t.command, ["claude", "--resume"]);
        assert_eq!(t.position, DetailPosition::Left);
        assert_eq!(t.size, Some(Size::Percent(35)));
        assert_eq!(t.start, TerminalStart::Worktree);
        assert_eq!(t.checkouts["acme/widgets"], "~/src/widgets");
        let s = TerminalSettings::from_config(&t);
        assert_eq!(s.chord.label(), "⌃]");
        assert_eq!(s.scrollback, 500);
        assert!(s.chord_problem.is_none());
        assert!(
            s.worktrees_root.is_none(),
            "demo and tests never get a root by default"
        );
    }

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let t = terminal("");
        assert!(t.command.is_empty());
        assert_eq!(t.escape, "ctrl-\\");
        assert_eq!(t.position, DetailPosition::Auto);
        assert_eq!(t.size, None);
        assert_eq!(t.scrollback, 10_000);
        assert_eq!(t.start, TerminalStart::Ask);
        assert_eq!(TerminalSettings::from_config(&t).chord, Chord::default());
    }

    #[test]
    fn an_unknown_key_is_refused_rather_than_ignored() {
        let err = toml::from_str::<Config>("[ui.terminal]\nshell = \"zsh\"\n").unwrap_err();
        assert!(err.to_string().contains("shell"), "{err}");
    }

    #[test]
    fn scrollback_is_kept_within_sensible_bounds() {
        let at = |n| {
            TerminalSettings::from_config(&TerminalConfig {
                scrollback: n,
                ..TerminalConfig::default()
            })
            .scrollback
        };
        assert_eq!(at(0), 100);
        assert_eq!(at(10_000_000), 200_000);
    }

    #[test]
    fn the_placement_ring_matches_the_pane_rotation() {
        let mut p = DetailPosition::Auto;
        let mut seen = Vec::new();
        for _ in 0..5 {
            p = next_position(p);
            seen.push(p.as_str());
        }
        assert_eq!(seen, ["bottom", "left", "top", "right", "auto"]);
    }
}
