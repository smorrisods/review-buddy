//! Runs the terminal pane's effects: finding a clone and planning a worktree, creating it, and
//! the PTY itself. `update` only ever sees the results as messages.
//!
//! Demo mode never gets here with anything that touches the system: the scripted pane lives in
//! memory, and `execute` ignores every effect when the backend is demo, as a second line of defence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use rb_term::worktree::{self, Plan, Request};
use rb_term::{Pty, PtyEvent};
use tokio::sync::mpsc::UnboundedSender;

use crate::app::terminal::{Launch, TermCmd, TermMsg};
use crate::app::{Msg, Notice};
use crate::cmd::git;

/// Owns the running child, if any.
#[derive(Default)]
pub struct PaneHost {
    active: Mutex<Option<Pty>>,
}

impl std::fmt::Debug for PaneHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PaneHost")
    }
}

impl PaneHost {
    pub fn execute(
        &self,
        cmd: TermCmd,
        tx: &UnboundedSender<Msg>,
        demo: bool,
        copy: impl FnOnce(&str) -> Notice,
    ) {
        if demo {
            return;
        }
        match cmd {
            TermCmd::Plan {
                launch,
                checkouts,
                root,
            } => {
                let tx = tx.clone();
                tokio::task::spawn_blocking(move || {
                    let result = plan_worktree(&launch, &checkouts, &root);
                    let _ = tx.send(Msg::Term(TermMsg::Planned(result)));
                });
            }
            TermCmd::CreateWorktree(plan) => {
                let tx = tx.clone();
                tokio::task::spawn_blocking(move || {
                    let result = worktree::execute(&plan).map_err(|e| e.to_string());
                    let _ = tx.send(Msg::Term(TermMsg::WorktreeReady(result)));
                });
            }
            TermCmd::Spawn { gen, spec } => {
                let events = tx.clone();
                let sink: Arc<dyn Fn(PtyEvent) + Send + Sync> = Arc::new(move |event| {
                    let msg = match event {
                        PtyEvent::Output(bytes) => TermMsg::Output { gen, bytes },
                        PtyEvent::Exited(code) => TermMsg::Exited { gen, code },
                    };
                    let _ = events.send(Msg::Term(msg));
                });
                match Pty::spawn(&spec, sink) {
                    Ok(pty) => self.replace(Some(pty)),
                    Err(err) => {
                        let _ = tx.send(Msg::Term(TermMsg::SpawnFailed {
                            gen,
                            reason: err.to_string(),
                        }));
                    }
                }
            }
            TermCmd::Write(bytes) => {
                if let Ok(mut active) = self.active.lock() {
                    if let Some(pty) = active.as_mut() {
                        let _ = pty.write(&bytes);
                    }
                }
            }
            TermCmd::Resize { cols, rows } => {
                if let Ok(active) = self.active.lock() {
                    if let Some(pty) = active.as_ref() {
                        pty.resize(cols, rows);
                    }
                }
            }
            TermCmd::Close => {
                self.replace(None);
                restore_host_input();
            }
            TermCmd::Copy(text) => {
                let _ = tx.send(Msg::Status(copy(&text)));
            }
        }
    }

    fn replace(&self, next: Option<Pty>) {
        if let Ok(mut active) = self.active.lock() {
            *active = next;
        }
    }
}

/// On Windows a ConPTY child can leave the host in win32-input-mode; turn it off. Elsewhere this
/// writes nothing.
fn restore_host_input() {
    use std::io::Write;
    let bytes = rb_term::host_cleanup();
    if !bytes.is_empty() {
        let mut out = std::io::stdout();
        let _ = out.write_all(bytes);
        let _ = out.flush();
    }
}

fn expand_home(path: &str) -> PathBuf {
    let home = || {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    };
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => home().map_or_else(|| PathBuf::from(path), |h| h.join(rest)),
        None if path == "~" => home().unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The name of the remote in the clone at `dir` whose URL is `repo`, and the clone's top folder.
fn clone_of(dir: &Path, repo: &str) -> Option<(PathBuf, String)> {
    let top = git_out(dir, &["rev-parse", "--show-toplevel"]).filter(|t| !t.is_empty())?;
    let remotes = git::parse_remote_list(&git_out(dir, &["remote", "-v"])?);
    let rules = git_out(dir, &["config", "--get-regexp", r"^url\..*\.insteadof$"])
        .map(|t| git::parse_instead_of(&t))
        .unwrap_or_default();
    let same =
        |url: &str| git::parse_remote_url(url).is_some_and(|r| r.path.eq_ignore_ascii_case(repo));
    // `git remote -v` shows a URL after `insteadOf` rewrote it; the config holds it as written.
    // Either spelling can name the repository.
    let matches = |remote: &git::Remote| {
        let raw = git_out(
            dir,
            &["config", "--get", &format!("remote.{}.url", remote.name)],
        );
        [Some(remote.url.clone()), raw]
            .into_iter()
            .flatten()
            .any(|url| same(&url) || same(&git::apply_instead_of(&url, &rules)))
    };
    let preferred = git::preferred_remote(&remotes).filter(|r| matches(r));
    let found = preferred.or_else(|| remotes.iter().find(|r| matches(r)))?;
    Some((PathBuf::from(top), found.name.clone()))
}

/// Looks for a local clone of the change's repository (the directory Review Buddy was started in,
/// then `ui.terminal.checkouts`) and plans the worktree there.
pub fn plan_worktree(
    launch: &Launch,
    checkouts: &BTreeMap<String, String>,
    root: &Path,
) -> Result<Plan, String> {
    let repo = &launch.id.repo;
    let mut candidates: Vec<PathBuf> = std::env::current_dir().into_iter().collect();
    if let Some(dir) = checkouts.get(repo) {
        candidates.push(expand_home(dir));
    }
    for dir in &candidates {
        if let Some((clone_dir, remote)) = clone_of(dir, repo) {
            let request = Request {
                source: launch.id.source_id.as_str().to_string(),
                repo: repo.clone(),
                number: launch.id.number,
                url: launch.url.clone(),
                refspec: launch.id.checkout_refspec(),
                remote,
                clone_dir,
                root: root.to_path_buf(),
            };
            let path = worktree::path_for(root, &request.source, repo, request.number);
            return Ok(worktree::plan(&request, path.is_dir()));
        }
    }
    Err(format!(
        "Review Buddy didn't find a local clone of {repo}. Start it from inside a clone, or list one under [ui.terminal.checkouts] in your config. Starting in the current directory still works."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::{ChangeId, ForgeKind, SourceId};

    fn launch(repo: &str) -> Launch {
        Launch {
            id: ChangeId {
                source_id: SourceId::new("work"),
                kind: ForgeKind::GitHub,
                repo: repo.into(),
                number: 214,
            },
            url: "https://github.com/acme/widgets/pull/214".into(),
        }
    }

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn init_clone(dir: &Path, url: &str) {
        for args in [vec!["init", "-q"], vec!["remote", "add", "origin", url]] {
            let ok = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .status()
                .unwrap()
                .success();
            assert!(ok);
        }
    }

    #[test]
    fn a_checkout_entry_is_found_and_planned() {
        if !git_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("widgets");
        std::fs::create_dir_all(&clone).unwrap();
        init_clone(&clone, "git@github.com:acme/widgets.git");
        let mut checkouts = BTreeMap::new();
        checkouts.insert(
            "acme/widgets".to_string(),
            clone.to_string_lossy().into_owned(),
        );
        let root = tmp.path().join("worktrees");
        let plan = plan_worktree(&launch("acme/widgets"), &checkouts, &root).unwrap();
        assert_eq!(plan.request.remote, "origin");
        assert_eq!(plan.request.refspec, "pull/214/head");
        assert!(plan.path.starts_with(&root));
        assert!(!plan.reuse);
        assert_eq!(plan.steps.len(), 2);
        assert!(!root.exists(), "planning creates nothing");
    }

    #[test]
    fn a_clone_of_another_repository_is_not_used() {
        if !git_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("other");
        std::fs::create_dir_all(&clone).unwrap();
        init_clone(&clone, "https://github.com/acme/other.git");
        let mut checkouts = BTreeMap::new();
        checkouts.insert(
            "acme/widgets".to_string(),
            clone.to_string_lossy().into_owned(),
        );
        let err = plan_worktree(&launch("acme/widgets"), &checkouts, tmp.path()).unwrap_err();
        assert!(
            err.contains("didn't find a local clone of acme/widgets"),
            "{err}"
        );
        assert!(err.contains("ui.terminal.checkouts"));
    }

    #[test]
    fn home_is_expanded_only_for_a_leading_tilde() {
        assert_eq!(expand_home("/abs/x"), PathBuf::from("/abs/x"));
        assert_eq!(expand_home("rel/x"), PathBuf::from("rel/x"));
        assert_ne!(expand_home("~/x"), PathBuf::from("~/x").join("never"));
    }
}
