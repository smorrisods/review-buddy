//! A managed worktree of a change's head, for the pane to start in.
//!
//! Creating one changes local state, so this module is split in two. [`plan`] is pure: it works out
//! the path and the git commands and can describe them in a preview. [`execute`] runs them, and
//! only the host calls it, after the person confirmed. Review Buddy never removes a worktree on its
//! own; the preview says how to.
//!
//! The module knows nothing about forges. The host passes the ref to fetch (for example
//! `pull/214/head`) and the local clone to fetch it into.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Everything needed to plan a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The source's id, as shown to the person.
    pub source: String,
    /// `owner/name` or the GitLab project path.
    pub repo: String,
    pub number: u64,
    pub url: String,
    /// The ref that holds the change's head on the remote.
    pub refspec: String,
    pub remote: String,
    /// The local clone to fetch into and add the worktree from.
    pub clone_dir: PathBuf,
    /// `$XDG_STATE_HOME/review-buddy/worktrees`.
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub request: Request,
    pub path: PathBuf,
    /// The worktree is already there, so nothing is created and it is used as it is.
    pub reuse: bool,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeError(pub String);

impl std::fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WorktreeError {}

fn slug(text: &str) -> String {
    let mut out: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    while out.starts_with('.') {
        out.remove(0);
    }
    if out.is_empty() {
        out.push('x');
    }
    out
}

/// Where the worktree for a change lives: `<root>/<source>/<repo>/<number>`, each part reduced to
/// letters, digits, dots, dashes and underscores.
pub fn path_for(root: &Path, source: &str, repo: &str, number: u64) -> PathBuf {
    let repo = repo.split('/').map(slug).collect::<Vec<_>>().join("__");
    root.join(slug(source)).join(repo).join(number.to_string())
}

/// The variables every pane child gets, in a worktree or not.
pub fn env_for(source: &str, repo: &str, number: u64, url: &str) -> Vec<(String, String)> {
    vec![
        ("RB_SOURCE".into(), source.into()),
        ("RB_REPO".into(), repo.into()),
        ("RB_NUMBER".into(), number.to_string()),
        ("RB_URL".into(), url.into()),
    ]
}

/// Plans the worktree. `exists` says whether the path is already a directory.
pub fn plan(request: &Request, exists: bool) -> Plan {
    let path = path_for(
        &request.root,
        &request.source,
        &request.repo,
        request.number,
    );
    let clone = request.clone_dir.to_string_lossy().into_owned();
    let steps = if exists {
        Vec::new()
    } else {
        vec![
            Step {
                program: "git".into(),
                args: vec![
                    "-C".into(),
                    clone.clone(),
                    "fetch".into(),
                    request.remote.clone(),
                    request.refspec.clone(),
                ],
            },
            Step {
                program: "git".into(),
                args: vec![
                    "-C".into(),
                    clone,
                    "worktree".into(),
                    "add".into(),
                    "--detach".into(),
                    path.to_string_lossy().into_owned(),
                    "FETCH_HEAD".into(),
                ],
            },
        ]
    };
    Plan {
        request: request.clone(),
        path,
        reuse: exists,
        steps,
    }
}

fn quote(text: &str) -> String {
    if text.is_empty()
        || text
            .chars()
            .any(|c| c.is_whitespace() || "'\"$`\\".contains(c))
    {
        format!("'{}'", text.replace('\'', "'\\''"))
    } else {
        text.to_string()
    }
}

impl Step {
    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Plan {
    /// The command that removes the worktree again, for the preview and the docs.
    pub fn cleanup_command(&self) -> String {
        Step {
            program: "git".into(),
            args: vec![
                "-C".into(),
                self.request.clone_dir.to_string_lossy().into_owned(),
                "worktree".into(),
                "remove".into(),
                self.path.to_string_lossy().into_owned(),
            ],
        }
        .display()
    }

    /// Plain sentences for the confirm: what will happen, what is left alone, how to clean up.
    pub fn preview(&self) -> Vec<String> {
        let r = &self.request;
        let mut lines = Vec::new();
        if self.reuse {
            lines.push(format!(
                "A checkout for {}#{} already exists at {}.",
                r.repo,
                r.number,
                self.path.display()
            ));
            lines.push(
                "The terminal will start there as it is. Nothing is fetched or changed.".into(),
            );
        } else {
            lines.push(format!(
                "Create a separate checkout of {}#{} at {}.",
                r.repo,
                r.number,
                self.path.display()
            ));
            for step in &self.steps {
                lines.push(format!("  {}", step.display()));
            }
            lines.push(format!(
                "Your own working copy at {} is left alone.",
                r.clone_dir.display()
            ));
        }
        lines.push(format!(
            "Review Buddy never removes it for you. When you are done: {}",
            self.cleanup_command()
        ));
        lines
    }

    pub fn env(&self) -> Vec<(String, String)> {
        env_for(
            &self.request.source,
            &self.request.repo,
            self.request.number,
            &self.request.url,
        )
    }
}

/// Runs the plan and returns the directory the terminal should start in.
pub fn execute(plan: &Plan) -> Result<PathBuf, WorktreeError> {
    if !plan.path.starts_with(&plan.request.root) {
        return Err(WorktreeError(
            "The worktree path isn't inside Review Buddy's worktrees folder, so nothing was created.".into(),
        ));
    }
    if plan.reuse {
        return Ok(plan.path.clone());
    }
    if let Some(parent) = plan.path.parent() {
        make_private_dir(parent).map_err(|e| {
            WorktreeError(format!(
                "Couldn't create {}: {e}. Check the folder's permissions and try again.",
                parent.display()
            ))
        })?;
    }
    for step in &plan.steps {
        let out = Command::new(&step.program)
            .args(&step.args)
            .output()
            .map_err(|e| {
                WorktreeError(format!(
                    "Couldn't run {}: {e}. Install it, or start in the current directory instead.",
                    step.program
                ))
            })?;
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr);
            let why = why
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("no details");
            return Err(WorktreeError(format!(
                "`{}` failed: {why}. Check that {} is reachable and that you can read {}, then try again.",
                step.display(),
                plan.request.remote,
                plan.request.repo
            )));
        }
    }
    Ok(plan.path.clone())
}

#[cfg(unix)]
fn make_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn make_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(root: &Path, clone: &Path) -> Request {
        Request {
            source: "work gh".into(),
            repo: "acme/widgets".into(),
            number: 214,
            url: "https://example.test/acme/widgets/pull/214".into(),
            refspec: "pull/214/head".into(),
            remote: "origin".into(),
            clone_dir: clone.to_path_buf(),
            root: root.to_path_buf(),
        }
    }

    #[test]
    fn the_path_is_stable_and_safe() {
        let root = Path::new("/state/worktrees");
        assert_eq!(
            path_for(root, "work gh", "acme/widgets", 214),
            root.join("work-gh").join("acme__widgets").join("214")
        );
        let nasty = path_for(root, "../x", "../../etc/passwd", 1);
        assert!(nasty.starts_with(root));
        assert!(!nasty.to_string_lossy().contains("/../"));
    }

    #[test]
    fn nested_gitlab_groups_flatten() {
        let p = path_for(Path::new("/r"), "gl", "group/sub/project", 7);
        assert_eq!(
            p,
            Path::new("/r")
                .join("gl")
                .join("group__sub__project")
                .join("7")
        );
    }

    #[test]
    fn the_plan_fetches_then_adds_a_detached_worktree() {
        let p = plan(
            &request(Path::new("/s/w"), Path::new("/src/widgets")),
            false,
        );
        assert!(!p.reuse);
        assert_eq!(p.steps.len(), 2);
        assert_eq!(
            p.steps[0].display(),
            "git -C /src/widgets fetch origin pull/214/head"
        );
        assert!(p.steps[1].display().contains("worktree add --detach"));
        assert!(p.steps[1].display().ends_with("FETCH_HEAD"));
    }

    #[test]
    fn the_preview_explains_the_work_and_the_cleanup() {
        let p = plan(
            &request(Path::new("/s/w"), Path::new("/src/my widgets")),
            false,
        );
        let text = p.preview().join("\n");
        assert!(text.contains("Create a separate checkout of acme/widgets#214"));
        assert!(text.contains("git -C '/src/my widgets' fetch origin pull/214/head"));
        assert!(text.contains("left alone"));
        assert!(text.contains("never removes it for you"));
        assert!(text.contains("worktree remove"));
    }

    #[test]
    fn an_existing_worktree_is_reused_without_commands() {
        let p = plan(&request(Path::new("/s/w"), Path::new("/src/widgets")), true);
        assert!(p.reuse && p.steps.is_empty());
        let text = p.preview().join("\n");
        assert!(text.contains("already exists"));
        assert!(text.contains("Nothing is fetched"));
    }

    #[test]
    fn the_environment_names_the_change() {
        let env = env_for("s", "a/b", 9, "https://x.test/9");
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("RB_SOURCE"), Some("s"));
        assert_eq!(get("RB_REPO"), Some("a/b"));
        assert_eq!(get("RB_NUMBER"), Some("9"));
        assert_eq!(get("RB_URL"), Some("https://x.test/9"));
    }

    #[test]
    fn a_path_outside_the_root_is_refused() {
        let mut p = plan(
            &request(Path::new("/s/w"), Path::new("/src/widgets")),
            false,
        );
        p.path = PathBuf::from("/elsewhere/214");
        assert!(execute(&p).unwrap_err().0.contains("isn't inside"));
    }

    fn git_ok() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.test",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn execute_creates_a_detached_worktree_of_the_change_head() {
        if !git_ok() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        let clone = tmp.path().join("clone");
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "--bare", "-q"]);
        std::fs::create_dir_all(&clone).unwrap();
        git(&clone, &["init", "-q"]);
        std::fs::write(clone.join("a.txt"), "one").unwrap();
        git(&clone, &["add", "."]);
        git(&clone, &["commit", "-q", "-m", "one"]);
        git(
            &clone,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        std::fs::write(clone.join("a.txt"), "two").unwrap();
        git(&clone, &["commit", "-q", "-am", "two"]);
        git(&clone, &["push", "-q", "origin", "HEAD:refs/pull/214/head"]);
        git(&clone, &["reset", "-q", "--hard", "HEAD~1"]);

        let root = tmp.path().join("state").join("worktrees");
        let p = plan(&request(&root, &clone), false);
        let dir = execute(&p).unwrap();
        assert_eq!(dir, p.path);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "two");
        assert_eq!(
            std::fs::read_to_string(clone.join("a.txt")).unwrap(),
            "one",
            "the clone is untouched"
        );

        let again = plan(&request(&root, &clone), dir.is_dir());
        assert!(again.reuse);
        assert_eq!(execute(&again).unwrap(), dir);
    }

    #[test]
    fn a_failed_fetch_says_what_to_check() {
        if !git_ok() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        std::fs::create_dir_all(&clone).unwrap();
        git(&clone, &["init", "-q"]);
        git(
            &clone,
            &[
                "remote",
                "add",
                "origin",
                tmp.path().join("nowhere").to_str().unwrap(),
            ],
        );
        let root = tmp.path().join("w");
        let err = execute(&plan(&request(&root, &clone), false)).unwrap_err();
        assert!(err.0.contains("failed"), "{err}");
        assert!(err.0.contains("then try again"), "{err}");
        assert!(!root
            .join("work-gh")
            .join("acme__widgets")
            .join("214")
            .exists());
    }
}
