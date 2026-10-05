//! Dashboard state and its pure transitions: focus, the source filter, the queue selection,
//! scrolling and the detail tabs.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rb_core::{ChangeId, ChangeSummary, Source, SourceId};

use super::queue::{item_span, total_height, Item, Queue};
use super::update::set_status;
use super::{App, Cmd, Notice, NoticeKind};
use crate::ui::{detail, layout};

const WHEEL_LINES: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sources,
    Queue,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Files,
    Checks,
    Conversation,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Overview, Tab::Files, Tab::Checks, Tab::Conversation];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Files => "Files",
            Tab::Checks => "Checks",
            Tab::Conversation => "Conversation",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1).min(Self::ALL.len() - 1)]
    }

    pub fn prev(self) -> Self {
        Self::ALL[self.index().saturating_sub(1)]
    }
}

/// The action chips under the detail title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Approve,
    RequestChanges,
    Comment,
    Diff,
    Merge,
}

impl Chip {
    pub const ALL: [Chip; 5] = [
        Chip::Approve,
        Chip::RequestChanges,
        Chip::Comment,
        Chip::Diff,
        Chip::Merge,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Chip::Approve => "a",
            Chip::RequestChanges => "x",
            Chip::Comment => "c",
            Chip::Diff => "⏎",
            Chip::Merge => "m",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Chip::Approve => "Approve",
            Chip::RequestChanges => "Request changes",
            Chip::Comment => "Comment",
            Chip::Diff => "Diff",
            Chip::Merge => "Merge",
        }
    }
}

/// What the queue cursor rests on. Tracks a change by id so it survives reloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selected {
    Change(ChangeId),
    Noise,
}

#[derive(Debug, Clone)]
pub struct Dashboard {
    pub focus: Pane,
    /// 0 is All, then each source in order.
    pub source: usize,
    pub selected: Option<Selected>,
    pub noise_open: bool,
    pub tab: Tab,
    pub queue_scroll: u16,
    pub detail_scroll: u16,
    /// Where the cursor last sat in the item list. When the selected change disappears
    /// (a refresh, a filter) the cursor settles on whatever now sits at this position.
    index: usize,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self {
            focus: Pane::Queue,
            source: 0,
            selected: None,
            noise_open: false,
            tab: Tab::Overview,
            queue_scroll: 0,
            detail_scroll: 0,
            index: 0,
        }
    }
}

impl App {
    /// The source being shown: `None` for All.
    pub fn active_source(&self) -> Option<&Source> {
        self.dashboard
            .source
            .checked_sub(1)
            .and_then(|i| self.state.sources.get(i))
    }

    pub fn queue(&self) -> Queue {
        let id: Option<&SourceId> = self.active_source().map(|s| &s.id);
        Queue::build(&self.state, id, self.dashboard.noise_open)
    }

    pub fn selected_change(&self) -> Option<&ChangeSummary> {
        match &self.dashboard.selected {
            Some(Selected::Change(id)) => self.state.changes.iter().find(|c| &c.id == id),
            _ => None,
        }
    }
}

/// The panes focus visits, in order.
pub fn focus_order(width: u16) -> Vec<Pane> {
    if layout::is_collapsed(width) {
        vec![Pane::Queue, Pane::Detail]
    } else {
        vec![Pane::Sources, Pane::Queue, Pane::Detail]
    }
}

fn selection_of(app: &App, item: Item) -> Option<Selected> {
    match item {
        Item::Noise => Some(Selected::Noise),
        Item::Change(i) => app
            .state
            .changes
            .get(i)
            .map(|c| Selected::Change(c.id.clone())),
    }
}

pub fn queue_view_height(app: &App) -> u16 {
    let l = layout::dashboard(layout::body(app.size));
    layout::inner(l.queue).height
}

/// Handles a key on the dashboard. `None` means the key isn't one of ours.
pub fn on_key(app: &mut App, key: KeyEvent) -> Option<Vec<Cmd>> {
    if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    let focus = app.dashboard.focus;
    let cmds = match key.code {
        KeyCode::Tab => cycle_focus(app, 1),
        KeyCode::BackTab => cycle_focus(app, -1),
        KeyCode::Char('l') => cycle_focus(app, 1),
        KeyCode::Char('h') => cycle_focus(app, -1),
        KeyCode::Right if focus == Pane::Detail => switch_tab(app, app.dashboard.tab.next()),
        KeyCode::Left if focus == Pane::Detail => switch_tab(app, app.dashboard.tab.prev()),
        KeyCode::Right => cycle_focus(app, 1),
        KeyCode::Left => cycle_focus(app, -1),
        KeyCode::Char(']') => switch_tab(app, app.dashboard.tab.next()),
        KeyCode::Char('[') => switch_tab(app, app.dashboard.tab.prev()),
        KeyCode::Down | KeyCode::Char('j') => step(app, 1),
        KeyCode::Up | KeyCode::Char('k') => step(app, -1),
        KeyCode::Char('g') | KeyCode::Home => jump(app, false),
        KeyCode::Char('G') | KeyCode::End => jump(app, true),
        KeyCode::Enter => activate(app),
        KeyCode::Char('d') => open_diff(app),
        KeyCode::Char(c @ '1'..='9') => {
            let n = usize::from(c as u8 - b'0') - 1;
            if n <= app.state.sources.len() {
                select_source(app, n);
            }
            Vec::new()
        }
        _ => return None,
    };
    app.mark_dirty();
    Some(cmds)
}

pub fn on_action(app: &mut App, action: super::Action) -> Vec<Cmd> {
    use super::Action;
    let cmds = match action {
        Action::FocusPane(pane) => {
            app.dashboard.focus = pane;
            Vec::new()
        }
        Action::SelectSource(n) => {
            app.dashboard.focus = Pane::Sources;
            select_source(app, n);
            Vec::new()
        }
        Action::SelectItem(n) => {
            app.dashboard.focus = Pane::Queue;
            let items = app.queue().items();
            if let Some(item) = items.get(n) {
                select_item(app, *item, n);
            }
            Vec::new()
        }
        Action::SelectTab(tab) => switch_tab(app, tab),
        Action::Chip(chip) => chip_pressed(app, chip),
        _ => return Vec::new(),
    };
    app.mark_dirty();
    cmds
}

pub fn on_scroll(app: &mut App, column: u16, row: u16, down: bool) {
    let Some(pane) = app.hits.pane_at(column, row) else {
        return;
    };
    match pane {
        Pane::Sources => {}
        Pane::Queue => {
            let rows = app.queue().rows();
            let max = total_height(&rows).saturating_sub(queue_view_height(app));
            let s = &mut app.dashboard.queue_scroll;
            *s = if down {
                s.saturating_add(WHEEL_LINES).min(max)
            } else {
                s.saturating_sub(WHEEL_LINES)
            };
        }
        Pane::Detail => {
            let max = detail::max_scroll(app);
            let s = &mut app.dashboard.detail_scroll;
            *s = if down {
                s.saturating_add(WHEEL_LINES).min(max)
            } else {
                s.saturating_sub(WHEEL_LINES)
            };
        }
    }
    app.mark_dirty();
}

pub fn on_resize(app: &mut App) {
    if !focus_order(app.size.0).contains(&app.dashboard.focus) {
        app.dashboard.focus = Pane::Queue;
    }
    clamp_scrolls(app);
}

/// Called whenever the data or the filter changes: keeps the selection if it is still
/// listed, otherwise settles on the nearest position.
pub fn reconcile(app: &mut App) {
    let items = app.queue().items();
    if app.dashboard.source > app.state.sources.len() {
        app.dashboard.source = 0;
    }
    let kept = app.dashboard.selected.as_ref().and_then(|sel| {
        items
            .iter()
            .position(|i| selection_of(app, *i).as_ref() == Some(sel))
    });
    match kept {
        Some(pos) => app.dashboard.index = pos,
        None if items.is_empty() => {
            app.dashboard.selected = None;
            app.dashboard.index = 0;
        }
        None => {
            let pos = app.dashboard.index.min(items.len() - 1);
            let sel = selection_of(app, items[pos]);
            set_selected(app, sel, pos);
        }
    }
    clamp_scrolls(app);
    ensure_visible(app);
}

fn set_selected(app: &mut App, selected: Option<Selected>, index: usize) {
    if app.dashboard.selected != selected {
        app.dashboard.detail_scroll = 0;
    }
    app.dashboard.selected = selected;
    app.dashboard.index = index;
}

fn select_item(app: &mut App, item: Item, index: usize) {
    let sel = selection_of(app, item);
    set_selected(app, sel, index);
    ensure_visible(app);
}

fn select_source(app: &mut App, n: usize) {
    app.dashboard.source = n.min(app.state.sources.len());
    reconcile(app);
}

fn cycle_focus(app: &mut App, step: isize) -> Vec<Cmd> {
    let order = focus_order(app.size.0);
    let at = order
        .iter()
        .position(|p| *p == app.dashboard.focus)
        .unwrap_or(0) as isize;
    let next = (at + step).rem_euclid(order.len() as isize) as usize;
    app.dashboard.focus = order[next];
    Vec::new()
}

fn switch_tab(app: &mut App, tab: Tab) -> Vec<Cmd> {
    if app.dashboard.tab != tab {
        app.dashboard.tab = tab;
        app.dashboard.detail_scroll = 0;
    }
    Vec::new()
}

fn step(app: &mut App, delta: isize) -> Vec<Cmd> {
    match app.dashboard.focus {
        Pane::Sources => {
            let last = app.state.sources.len();
            let next = app.dashboard.source.saturating_add_signed(delta).min(last);
            select_source(app, next);
        }
        Pane::Queue => {
            let items = app.queue().items();
            if !items.is_empty() {
                let next = app
                    .dashboard
                    .index
                    .saturating_add_signed(delta)
                    .min(items.len() - 1);
                select_item(app, items[next], next);
            }
        }
        Pane::Detail => {
            let max = detail::max_scroll(app);
            let s = &mut app.dashboard.detail_scroll;
            *s = s.saturating_add_signed(delta as i16).min(max);
        }
    }
    Vec::new()
}

fn jump(app: &mut App, to_end: bool) -> Vec<Cmd> {
    match app.dashboard.focus {
        Pane::Sources => {
            let target = if to_end { app.state.sources.len() } else { 0 };
            select_source(app, target);
        }
        Pane::Queue => {
            let items = app.queue().items();
            if let Some(pos) = if to_end {
                items.len().checked_sub(1)
            } else {
                (!items.is_empty()).then_some(0)
            } {
                select_item(app, items[pos], pos);
            }
        }
        Pane::Detail => {
            app.dashboard.detail_scroll = if to_end { detail::max_scroll(app) } else { 0 };
        }
    }
    Vec::new()
}

fn activate(app: &mut App) -> Vec<Cmd> {
    match (&app.dashboard.selected, app.dashboard.focus) {
        (Some(Selected::Noise), Pane::Queue) => {
            app.dashboard.noise_open = !app.dashboard.noise_open;
            reconcile(app);
            Vec::new()
        }
        (Some(Selected::Change(_)), Pane::Queue | Pane::Detail) => open_diff(app),
        _ => Vec::new(),
    }
}

fn open_diff(app: &mut App) -> Vec<Cmd> {
    if app.selected_change().is_none() {
        return Vec::new();
    }
    chip_pressed(app, Chip::Diff)
}

fn chip_pressed(app: &mut App, chip: Chip) -> Vec<Cmd> {
    if app.selected_change().is_none() {
        return Vec::new();
    }
    if chip == Chip::Diff {
        return super::diff::open(app);
    }
    let text = format!("{} isn't available yet in this build.", chip.label());
    set_status(app, Notice::new(NoticeKind::Info, text))
}

fn clamp_scrolls(app: &mut App) {
    let rows = app.queue().rows();
    let max = total_height(&rows).saturating_sub(queue_view_height(app));
    app.dashboard.queue_scroll = app.dashboard.queue_scroll.min(max);
    app.dashboard.detail_scroll = app.dashboard.detail_scroll.min(detail::max_scroll(app));
}

/// Scrolls the queue just enough to show the selected item (and its heading when it is the
/// first in its bucket).
fn ensure_visible(app: &mut App) {
    let items = app.queue().items();
    let Some(item) = items.get(app.dashboard.index).copied() else {
        app.dashboard.queue_scroll = 0;
        return;
    };
    let rows = app.queue().rows();
    let Some((top, height)) = item_span(&rows, item) else {
        return;
    };
    let view = queue_view_height(app);
    let max = total_height(&rows).saturating_sub(view);
    let scroll = app.dashboard.queue_scroll;
    let head = top.saturating_sub(if top == 1 { 1 } else { 0 });
    app.dashboard.queue_scroll = if head < scroll {
        head
    } else if top + height > scroll + view {
        (top + height).saturating_sub(view)
    } else {
        scroll
    }
    .min(max);
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEvent, KeyEventKind};
    use rb_core::Timestamp;
    use rb_theme::ColourDepth;

    use super::*;
    use crate::app::queue::tests::{change, source};
    use crate::app::{update, AppConfig, Msg, Snapshot};
    use rb_core::MyRole;

    fn loaded(width: u16, changes: Vec<ChangeSummary>) -> App {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (width, 40),
        });
        update(
            &mut app,
            Msg::Loaded(Box::new(Snapshot {
                label: "t".into(),
                sources: vec![source("s1", true), source("s2", true)],
                changes,
                now: Timestamp(1_000_000),
                details: Default::default(),
            })),
        );
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        update(app, Msg::Key(KeyEvent::from(code)));
    }

    fn sample() -> Vec<ChangeSummary> {
        let mut bot = change(5, MyRole::Reviewing, 1);
        bot.author_is_bot = true;
        bot.author = "renovate[bot]".into();
        let mut other = change(4, MyRole::Reviewing, 40);
        other.id.source_id = SourceId::new("s2");
        vec![
            change(1, MyRole::Reviewing, 30),
            change(2, MyRole::Reviewing, 20),
            change(3, MyRole::Mentioned, 10),
            other,
            bot,
        ]
    }

    fn selected_number(app: &App) -> Option<u64> {
        app.selected_change().map(|c| c.id.number)
    }

    #[test]
    fn first_load_selects_the_first_row() {
        let app = loaded(160, sample());
        assert_eq!(app.dashboard.focus, Pane::Queue);
        assert_eq!(selected_number(&app), Some(4));
    }

    #[test]
    fn j_k_move_and_clamp_at_the_ends() {
        let mut app = loaded(160, sample());
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(selected_number(&app), Some(1));
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_number(&app), Some(2));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        assert_eq!(selected_number(&app), Some(4));
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.dashboard.selected, Some(Selected::Noise));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.dashboard.selected, Some(Selected::Noise));
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(selected_number(&app), Some(4));
    }

    #[test]
    fn enter_on_the_noise_row_expands_and_collapses() {
        let mut app = loaded(160, sample());
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.queue().items().len(), 5);
        press(&mut app, KeyCode::Enter);
        assert!(app.dashboard.noise_open);
        assert_eq!(app.queue().items().len(), 6);
        assert_eq!(app.dashboard.selected, Some(Selected::Noise));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(selected_number(&app), Some(5));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Enter);
        assert!(!app.dashboard.noise_open);
    }

    #[test]
    fn collapsing_noise_with_a_noise_change_selected_moves_to_the_noise_row() {
        let mut app = loaded(160, sample());
        app.dashboard.noise_open = true;
        reconcile(&mut app);
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(selected_number(&app), Some(5));
        app.dashboard.noise_open = false;
        reconcile(&mut app);
        assert_eq!(app.dashboard.selected, Some(Selected::Noise));
    }

    #[test]
    fn focus_cycles_with_tab_and_h_l() {
        let mut app = loaded(160, sample());
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.dashboard.focus, Pane::Detail);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.dashboard.focus, Pane::Sources);
        update(
            &mut app,
            Msg::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
        );
        assert_eq!(app.dashboard.focus, Pane::Detail);
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(app.dashboard.focus, Pane::Queue);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.dashboard.focus, Pane::Detail);
    }

    #[test]
    fn collapsed_layout_skips_sources_and_recovers_focus_on_resize() {
        let mut app = loaded(120, sample());
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.dashboard.focus, Pane::Queue);
        let mut wide = loaded(160, sample());
        press(&mut wide, KeyCode::Tab);
        press(&mut wide, KeyCode::Tab);
        assert_eq!(wide.dashboard.focus, Pane::Sources);
        update(&mut wide, Msg::Resize(110, 40));
        assert_eq!(wide.dashboard.focus, Pane::Queue);
    }

    #[test]
    fn keys_in_the_sources_pane_filter_the_queue() {
        let mut app = loaded(160, sample());
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(app.dashboard.focus, Pane::Sources);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.dashboard.source, 1);
        assert_eq!(app.queue().items().len(), 4);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.queue().items(), vec![Item::Change(3)]);
        assert_eq!(selected_number(&app), Some(4));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.dashboard.source, 2);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.dashboard.source, 2);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.dashboard.source, 0);
    }

    #[test]
    fn number_keys_switch_source_and_ignore_missing_ones() {
        let mut app = loaded(160, sample());
        press(&mut app, KeyCode::Char('3'));
        assert_eq!(app.dashboard.source, 2);
        press(&mut app, KeyCode::Char('9'));
        assert_eq!(app.dashboard.source, 2);
        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.dashboard.source, 0);
    }

    #[test]
    fn selection_follows_its_change_when_the_queue_reorders() {
        let mut app = loaded(160, sample());
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(selected_number(&app), Some(2));
        let mut changes = sample();
        changes[1].updated_at = Timestamp(999_000);
        app.state.changes = changes;
        reconcile(&mut app);
        assert_eq!(selected_number(&app), Some(2));
        assert_eq!(app.dashboard.index, 0);
    }

    #[test]
    fn selection_settles_on_the_same_position_when_its_change_leaves() {
        let mut app = loaded(160, sample());
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        app.state.changes.retain(|c| c.id.number != 2);
        reconcile(&mut app);
        assert_eq!(selected_number(&app), Some(3));
        app.state.changes.clear();
        reconcile(&mut app);
        assert_eq!(app.dashboard.selected, None);
    }

    #[test]
    fn tabs_move_with_brackets_and_arrows_in_detail_and_clamp() {
        let mut app = loaded(160, sample());
        press(&mut app, KeyCode::Char(']'));
        assert_eq!(app.dashboard.tab, Tab::Files);
        press(&mut app, KeyCode::Char('['));
        press(&mut app, KeyCode::Char('['));
        assert_eq!(app.dashboard.tab, Tab::Overview);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Right);
        assert_eq!(app.dashboard.tab, Tab::Conversation);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.dashboard.tab, Tab::Checks);
        assert_eq!(app.dashboard.focus, Pane::Detail);
    }

    #[test]
    fn enter_on_a_change_opens_the_diff_and_asks_for_its_patches() {
        let mut app = loaded(160, sample());
        let cmds = update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Enter)));
        assert_eq!(app.screen, crate::app::Screen::Diff);
        assert!(
            matches!(cmds.as_slice(), [Cmd::LoadDiff(id)] if Some(id) == app.selected_change().map(|c| &c.id))
        );
        assert_eq!(
            app.diff.as_ref().map(|d| d.phase.clone()),
            Some(crate::app::Phase::Loading)
        );
    }

    #[test]
    fn d_and_the_diff_chip_open_the_diff_too() {
        let mut app = loaded(160, sample());
        let cmds = update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Char('d'))));
        assert_eq!(cmds.len(), 1);
        assert_eq!(app.screen, crate::app::Screen::Diff);
        let mut app = loaded(160, sample());
        let cmds = on_action(&mut app, crate::app::Action::Chip(Chip::Diff));
        assert_eq!(cmds.len(), 1);
        assert_eq!(app.screen, crate::app::Screen::Diff);
    }

    #[test]
    fn leaving_the_diff_keeps_the_dashboard_selection() {
        let mut app = loaded(160, sample());
        let before = app.dashboard.selected.clone();
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Enter)));
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Esc)));
        assert_eq!(app.screen, crate::app::Screen::Dashboard);
        assert_eq!(app.dashboard.selected, before);
    }

    #[test]
    fn other_chips_still_explain_themselves() {
        let mut app = loaded(160, sample());
        on_action(&mut app, crate::app::Action::Chip(Chip::Merge));
        assert!(app.status.as_ref().unwrap().notice.text.contains("Merge"));
        assert_eq!(app.screen, crate::app::Screen::Dashboard);
    }

    #[test]
    fn release_events_and_modified_keys_are_ignored() {
        let mut app = loaded(160, sample());
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..KeyEvent::from(KeyCode::Char('j'))
        };
        update(&mut app, Msg::Key(release));
        assert_eq!(selected_number(&app), Some(4));
        update(
            &mut app,
            Msg::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
        );
        assert_eq!(selected_number(&app), Some(4));
    }

    #[test]
    fn many_rows_scroll_to_keep_the_selection_visible() {
        let changes: Vec<_> = (1..=30)
            .map(|n| change(n, MyRole::Reviewing, 100 + n as i64))
            .collect();
        let mut app = loaded(160, changes);
        assert_eq!(app.dashboard.queue_scroll, 0);
        for _ in 0..29 {
            press(&mut app, KeyCode::Char('j'));
        }
        let rows = app.queue().rows();
        let item = app.queue().items()[29];
        let (top, h) = item_span(&rows, item).unwrap();
        let view = queue_view_height(&app);
        assert!(top >= app.dashboard.queue_scroll);
        assert!(top + h <= app.dashboard.queue_scroll + view);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.dashboard.queue_scroll, 0);
    }

    #[test]
    fn clicking_actions_select_and_focus() {
        let mut app = loaded(160, sample());
        update(&mut app, Msg::Key(KeyEvent::from(KeyCode::Tab)));
        on_action(&mut app, super::super::Action::SelectItem(2));
        assert_eq!(app.dashboard.focus, Pane::Queue);
        assert_eq!(selected_number(&app), Some(2));
        on_action(&mut app, super::super::Action::SelectSource(2));
        assert_eq!(app.dashboard.focus, Pane::Sources);
        assert_eq!(app.dashboard.source, 2);
        on_action(&mut app, super::super::Action::SelectTab(Tab::Checks));
        assert_eq!(app.dashboard.tab, Tab::Checks);
        on_action(&mut app, super::super::Action::FocusPane(Pane::Detail));
        assert_eq!(app.dashboard.focus, Pane::Detail);
    }

    #[test]
    fn the_wheel_scrolls_the_pane_under_the_pointer() {
        use ratatui::layout::Rect;
        let changes: Vec<_> = (1..=30)
            .map(|n| change(n, MyRole::Reviewing, 100 + n as i64))
            .collect();
        let mut app = loaded(160, changes);
        app.hits.set_panes(vec![
            (Rect::new(26, 1, 48, 38), Pane::Queue),
            (Rect::new(74, 1, 86, 38), Pane::Detail),
        ]);
        on_scroll(&mut app, 30, 5, true);
        assert_eq!(app.dashboard.queue_scroll, 3);
        on_scroll(&mut app, 30, 5, false);
        on_scroll(&mut app, 30, 5, false);
        assert_eq!(app.dashboard.queue_scroll, 0);
        on_scroll(&mut app, 0, 0, true);
        assert_eq!(app.dashboard.queue_scroll, 0);
    }
}
