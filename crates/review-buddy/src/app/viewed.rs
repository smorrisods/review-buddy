//! Files marked viewed. `m` ticks or clears the current file. Where the forge keeps the mark
//! (GitHub) it is sent there, so the change's web page agrees; elsewhere (GitLab) it is kept on
//! this machine, per commit. Pure: writes leave as [`Cmd`]s.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use rb_core::{ChangeId, ChangeSummary};

use super::update::set_status;
use super::{App, Cmd, DiffData, Notice, NoticeKind};
use crate::viewed::Stored;

/// Marks kept on this machine, one entry per change.
#[derive(Debug, Clone, Default)]
pub struct Local {
    /// Where they are written; `None` keeps them in memory only (demo mode).
    path: Option<PathBuf>,
    kept: HashMap<ChangeId, (String, BTreeSet<String>)>,
}

impl Local {
    pub fn new(path: Option<PathBuf>, loaded: Vec<Stored>) -> Self {
        Self {
            path,
            kept: loaded
                .into_iter()
                .map(|s| (s.id, (s.head_sha, s.paths)))
                .collect(),
        }
    }

    fn set(&mut self, id: &ChangeId, head_sha: &str, path: &str, viewed: bool) -> Option<Cmd> {
        let entry = self
            .kept
            .entry(id.clone())
            .or_insert_with(|| (head_sha.to_string(), BTreeSet::new()));
        if entry.0 != head_sha {
            *entry = (head_sha.to_string(), BTreeSet::new());
        }
        if viewed {
            entry.1.insert(path.to_string());
        } else {
            entry.1.remove(path);
        }
        if entry.1.is_empty() {
            self.kept.remove(id);
        }
        let path = self.path.clone()?;
        let mut entries: Vec<Stored> = self
            .kept
            .iter()
            .map(|(id, (sha, paths))| Stored {
                id: id.clone(),
                head_sha: sha.clone(),
                paths: paths.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Some(Cmd::SaveViewed { path, entries })
    }
}

/// Gives a freshly loaded diff the marks kept here, when the forge keeps none of its own and
/// the change is still on the commit they were made on.
pub fn seed_local(local: &Local, id: &ChangeId, changes: &[ChangeSummary], data: &mut DiffData) {
    if data.caps.viewed_files {
        return;
    }
    let head = changes
        .iter()
        .find(|c| &c.id == id)
        .map(|c| c.head_sha.as_str());
    if let (Some((sha, paths)), Some(head)) = (local.kept.get(id), head) {
        if sha == head {
            data.viewed.clone_from(paths);
        }
    }
}

/// `m`: marks the current file viewed, or clears the mark.
pub fn toggle(app: &mut App) -> Vec<Cmd> {
    let Some((id, path, synced)) = app.diff.as_ref().and_then(|s| {
        let file = s.current()?;
        Some((
            s.id.clone(),
            file.diff.path.clone(),
            s.data.as_ref()?.caps.viewed_files,
        ))
    }) else {
        return Vec::new();
    };
    let head = app
        .state
        .changes
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.head_sha.clone())
        .unwrap_or_default();
    let Some(data) = app.diff.as_mut().and_then(|s| s.data.as_mut()) else {
        return Vec::new();
    };
    let viewed = !data.viewed.contains(&path);
    if viewed {
        data.viewed.insert(path.clone());
    } else {
        data.viewed.remove(&path);
    }
    app.mark_dirty();
    let mut cmds = Vec::new();
    let note = if synced {
        cmds.push(Cmd::SetFileViewed { id, path, viewed });
        if viewed {
            "Marked viewed."
        } else {
            "Marked not viewed."
        }
    } else {
        cmds.extend(app.viewed.set(&id, &head, &path, viewed));
        if viewed {
            "Marked viewed on this machine. This forge keeps no viewed marks."
        } else {
            "Marked not viewed."
        }
    };
    let demo = if app.demo { " (demo)" } else { "" };
    cmds.extend(set_status(
        app,
        Notice::new(NoticeKind::Info, format!("{note}{demo}")),
    ));
    cmds
}

/// The forge's answer to a mark. A refusal puts the box back as it was and says what happened.
pub fn on_set(
    app: &mut App,
    id: &ChangeId,
    path: &str,
    viewed: bool,
    result: Result<(), String>,
) -> Vec<Cmd> {
    let Err(message) = result else {
        return Vec::new();
    };
    if let Some(data) = app
        .diff
        .as_mut()
        .filter(|s| &s.id == id)
        .and_then(|s| s.data.as_mut())
    {
        if viewed {
            data.viewed.remove(path);
        } else {
            data.viewed.insert(path.to_string());
        }
    }
    app.mark_dirty();
    let text = format!("The forge didn't take that mark ({message}). It is back as it was.");
    set_status(app, Notice::new(NoticeKind::Warning, text))
}
