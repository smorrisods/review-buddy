//! Web addresses for what the user is looking at, derived from the loaded sources.

use rb_core::{ChangeId, ForgeKind};

use super::{App, Screen};

/// The change's page on its forge, if its source is loaded.
pub fn change_url(app: &App, id: &ChangeId) -> Option<String> {
    let source = app.state.sources.iter().find(|s| s.id == id.source_id)?;
    let host = match source.kind {
        ForgeKind::GitHub => source.host.strip_prefix("api.").unwrap_or(&source.host),
        ForgeKind::GitLab => &source.host,
    };
    let path = match id.kind {
        ForgeKind::GitHub => format!("pull/{}", id.number),
        ForgeKind::GitLab => format!("-/merge_requests/{}", id.number),
    };
    Some(format!("https://{host}/{}/{path}", id.repo))
}

/// The page that shows the diff of a change.
pub fn files_url(app: &App, id: &ChangeId) -> Option<String> {
    let base = change_url(app, id)?;
    Some(match id.kind {
        ForgeKind::GitHub => format!("{base}/files"),
        ForgeKind::GitLab => format!("{base}/diffs"),
    })
}

/// The address `o` and `y` act on: the diff page inside a diff, else the selected change.
pub fn current_url(app: &App) -> Option<String> {
    match (app.screen, app.diff.as_ref()) {
        (Screen::Diff, Some(state)) => files_url(app, &state.id),
        _ => change_url(app, &app.selected_change()?.id.clone()),
    }
}
