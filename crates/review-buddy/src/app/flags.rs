//! Changes flagged to come back to. `f` flags or clears the selected change, `F` jumps to the
//! next flagged one in the queue. Pure: a write leaves as [`Cmd::SaveFlags`].

use std::collections::BTreeSet;
use std::path::PathBuf;

use rb_core::ChangeId;

use super::queue::Item;
use super::update::set_status;
use super::{dashboard, App, Cmd, Notice, NoticeKind};

#[derive(Debug, Clone, Default)]
pub struct Flags {
    /// Where flags are written; `None` keeps them in memory only (demo mode).
    path: Option<PathBuf>,
    set: BTreeSet<ChangeId>,
}

impl Flags {
    pub fn new(path: Option<PathBuf>, loaded: Vec<ChangeId>) -> Self {
        Self {
            path,
            set: loaded.into_iter().collect(),
        }
    }

    pub fn contains(&self, id: &ChangeId) -> bool {
        self.set.contains(id)
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Flags `id`, or clears it when it is flagged. Returns whether it is flagged now.
    fn toggle(&mut self, id: &ChangeId) -> bool {
        if self.set.remove(id) {
            false
        } else {
            self.set.insert(id.clone());
            true
        }
    }

    fn save_cmd(&self) -> Option<Cmd> {
        self.path.as_ref().map(|path| Cmd::SaveFlags {
            path: path.clone(),
            ids: self.set.iter().cloned().collect(),
        })
    }
}

/// `f`: flag or clear the selected change.
pub fn toggle_selected(app: &mut App) -> Vec<Cmd> {
    let Some(id) = app.selected_change().map(|c| c.id.clone()) else {
        return set_status(
            app,
            Notice::new(NoticeKind::Info, "Select a change to flag it."),
        );
    };
    toggle(app, &id)
}

/// Flags or clears `id`, with a status line that says what happened and how to undo it.
pub fn toggle(app: &mut App, id: &ChangeId) -> Vec<Cmd> {
    let on = app.flags.toggle(id);
    let demo = if app.demo { " (demo)" } else { "" };
    let text = if on {
        format!("Flagged to come back to{demo}. f clears it, F jumps to the next one.")
    } else {
        format!("Flag cleared{demo}.")
    };
    app.mark_dirty();
    let mut cmds = app.flags.save_cmd().into_iter().collect::<Vec<_>>();
    cmds.extend(set_status(app, Notice::new(NoticeKind::Info, text)));
    cmds
}

/// `F`: move the queue cursor to the next flagged change after it, wrapping round.
pub fn jump_next(app: &mut App) -> Vec<Cmd> {
    if app.flags.is_empty() {
        return set_status(
            app,
            Notice::new(
                NoticeKind::Info,
                "Nothing flagged yet. f flags the selected change.",
            ),
        );
    }
    let items = app.queue().items();
    let from = app.dashboard.index();
    let flagged = |item: &Item| match item {
        Item::Change(i) => app
            .state
            .changes
            .get(*i)
            .is_some_and(|c| app.flags.contains(&c.id)),
        _ => false,
    };
    let found = (1..=items.len())
        .map(|step| (from + step) % items.len())
        .find(|at| flagged(&items[*at]));
    match found {
        Some(at) => {
            dashboard::select_at(app, items[at], at);
            Vec::new()
        }
        None => set_status(
            app,
            Notice::new(
                NoticeKind::Info,
                "Flagged changes aren't in this view. Try another source or the Show filters.",
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::{ForgeKind, SourceId};

    fn id(n: u64) -> ChangeId {
        ChangeId {
            source_id: SourceId("s".into()),
            kind: ForgeKind::GitHub,
            repo: "o/r".into(),
            number: n,
        }
    }

    #[test]
    fn toggle_flags_then_clears() {
        let mut flags = Flags::new(None, Vec::new());
        assert!(flags.toggle(&id(1)));
        assert!(flags.contains(&id(1)));
        assert!(!flags.toggle(&id(1)));
        assert!(flags.is_empty());
    }

    #[test]
    fn only_a_persisting_list_writes() {
        let mut memory = Flags::new(None, Vec::new());
        memory.toggle(&id(1));
        assert!(memory.save_cmd().is_none());
        let mut disk = Flags::new(Some("/f.json".into()), Vec::new());
        disk.toggle(&id(2));
        assert!(matches!(disk.save_cmd(), Some(Cmd::SaveFlags { ids, .. }) if ids == vec![id(2)]));
    }
}
