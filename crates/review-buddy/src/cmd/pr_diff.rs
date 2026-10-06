//! `pr diff`: the patch for one change. Raw and exact when piped, coloured on a terminal.

use rb_core::{ChangeId, FilePatch, FileStatus, Scope};
use rb_diff::{DiffBody, FileDiff, Highlighter, LineId, LineKind};
use rb_theme::Role;

use super::context::Context;
use super::error::CmdError;
use super::output::{print, print_paged, Painter};
use super::selector::{Target, Which};

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub name_only: bool,
    pub stat: bool,
    pub files: Vec<String>,
    /// Print the raw patch even when colour would be used.
    pub raw: bool,
}

pub fn run(ctx: &Context, selector: Option<&str>, options: &Options) -> Result<(), CmdError> {
    let target = ctx.resolve_selector(selector)?;
    let files = fetch(ctx, &target)?;
    let files = filter_files(files, &options.files)?;

    if options.name_only {
        return print(&name_only(&files));
    }
    if options.stat {
        return print(&stat(&files, ctx.out.width, &ctx.out.painter));
    }
    for file in files.iter().filter(|f| f.patch.is_none()) {
        eprintln!("{}", skipped_note(file));
    }
    let text = if options.raw || !ctx.out.colour() {
        raw_patch(&files)
    } else {
        coloured_patch(&files, &ctx.out.painter)
    };
    print_paged(&ctx.out, &text)
}

fn fetch(ctx: &Context, target: &Target) -> Result<Vec<FilePatch>, CmdError> {
    let source = ctx
        .sources()?
        .into_iter()
        .find(|s| s.id == target.source)
        .ok_or_else(|| CmdError::usage("That source isn't configured."))?;
    let provider = ctx.provider_for(&source)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let number = match &target.which {
            Which::Number(n) => *n,
            Which::Branch(branch) => {
                let page = provider.list_changes(&Scope::everything(), None).await?;
                page.items
                    .iter()
                    .find(|c| c.id.repo == target.repo && &c.branch == branch)
                    .map(|c| c.id.number)
                    .ok_or_else(|| {
                        CmdError::usage(format!(
                            "No change in {} comes from the branch {branch}.\nPass a number or a URL instead.",
                            target.repo
                        ))
                    })?
            }
        };
        let id = ChangeId {
            source_id: target.source.clone(),
            kind: target.kind,
            repo: target.repo.clone(),
            number,
        };
        Ok(provider.files(&id).await?)
    })
}

fn filter_files(files: Vec<FilePatch>, wanted: &[String]) -> Result<Vec<FilePatch>, CmdError> {
    if wanted.is_empty() {
        return Ok(files);
    }
    let matches = |file: &FilePatch, want: &str| {
        let want = want.trim_end_matches('/');
        [Some(&file.path), file.old_path.as_ref()]
            .into_iter()
            .flatten()
            .any(|p| p == want || p.strip_prefix(want).is_some_and(|r| r.starts_with('/')))
    };
    if let Some(missing) = wanted.iter().find(|w| !files.iter().any(|f| matches(f, w))) {
        return Err(CmdError::usage(format!(
            "No changed file matches {missing}.\nTry review-buddy pr diff --name-only to see the paths."
        )));
    }
    Ok(files
        .into_iter()
        .filter(|f| wanted.iter().any(|w| matches(f, w)))
        .collect())
}

fn name_only(files: &[FilePatch]) -> String {
    files.iter().map(|f| format!("{}\n", f.path)).collect()
}

fn skipped_note(file: &FilePatch) -> String {
    format!(
        "Skipped {}: binary or too large to show here.\nOpen the change in the browser to see it: review-buddy pr open.",
        file.path
    )
}

fn has_hunks(body: &str) -> bool {
    body.lines().any(|l| l.starts_with("@@"))
}

/// The lines git expects before a file's hunks.
fn header_lines(file: &FilePatch, body: &str) -> Vec<String> {
    let old = file.old_path.as_deref().unwrap_or(&file.path);
    let mut out = vec![format!("diff --git a/{old} b/{}", file.path)];
    let (from, to) = match file.status {
        FileStatus::Added => {
            out.push("new file mode 100644".into());
            ("/dev/null".to_string(), format!("b/{}", file.path))
        }
        FileStatus::Removed => {
            out.push("deleted file mode 100644".into());
            (format!("a/{old}"), "/dev/null".to_string())
        }
        FileStatus::Renamed => {
            if !has_hunks(body) {
                out.push("similarity index 100%".into());
            }
            out.push(format!("rename from {old}"));
            out.push(format!("rename to {}", file.path));
            (format!("a/{old}"), format!("b/{}", file.path))
        }
        FileStatus::Modified => (format!("a/{old}"), format!("b/{}", file.path)),
    };
    if has_hunks(body) {
        out.push(format!("--- {from}"));
        out.push(format!("+++ {to}"));
    }
    out
}

fn raw_patch(files: &[FilePatch]) -> String {
    let mut out = String::new();
    for file in files {
        let Some(body) = file.patch.as_deref() else {
            continue;
        };
        for line in header_lines(file, body) {
            out.push_str(&line);
            out.push('\n');
        }
        out.push_str(body);
        if !body.is_empty() && !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn coloured_patch(files: &[FilePatch], painter: &Painter) -> String {
    let mut highlighter = Highlighter::new();
    let mut out = String::new();
    for file in files {
        let Some(body) = file.patch.as_deref() else {
            continue;
        };
        for (i, line) in header_lines(file, body).iter().enumerate() {
            let role = if i == 0 { Role::Accent } else { Role::Muted };
            out.push_str(&painter.paint(role, line));
            out.push('\n');
        }
        let diff = FileDiff::from_file_patch(file);
        let DiffBody::Text(parsed) = &diff.body else {
            out.push_str(body);
            if !body.is_empty() && !body.ends_with('\n') {
                out.push('\n');
            }
            continue;
        };
        let highlighted = highlighter.highlight(&file.path, parsed, painter.palette().theme(), 4);
        for (h, hunk) in parsed.hunks.iter().enumerate() {
            out.push_str(&painter.paint(Role::Accent, &hunk.header.render()));
            out.push('\n');
            for (l, line) in hunk.lines.iter().enumerate() {
                let (sign, role) = match line.kind {
                    LineKind::Added => ('+', Role::Success),
                    LineKind::Removed => ('-', Role::Danger),
                    LineKind::Context => (' ', Role::Muted),
                };
                out.push_str(&painter.paint(role, &sign.to_string()));
                let id = LineId {
                    hunk: h as u32,
                    line: l as u32,
                };
                match highlighted.line(id) {
                    Some(spans) => {
                        for span in spans {
                            let colour = span.role.and_then(|r| painter.palette().syntax(r));
                            out.push_str(&painter.paint_colour(colour, &span.text));
                        }
                    }
                    None => out.push_str(&line.text),
                }
                out.push('\n');
                if line.no_newline {
                    out.push_str(&painter.paint(Role::Muted, "\\ No newline at end of file"));
                    out.push('\n');
                }
            }
        }
    }
    out
}

fn stat(files: &[FilePatch], width: usize, painter: &Painter) -> String {
    let label = |f: &FilePatch| match &f.old_path {
        Some(old) if old != &f.path => format!("{old} => {}", f.path),
        _ => f.path.clone(),
    };
    let name_w = files
        .iter()
        .map(|f| label(f).chars().count())
        .max()
        .unwrap_or(0);
    let biggest = files.iter().map(|f| f.adds + f.dels).max().unwrap_or(0);
    let num_w = biggest.to_string().len().max(3);
    let room = width.saturating_sub(name_w + num_w + 6).max(10);
    let scale = |n: u32| -> usize {
        if n == 0 {
            0
        } else if biggest as usize <= room {
            n as usize
        } else {
            ((n as usize * room) / biggest as usize).max(1)
        }
    };
    let mut out = String::new();
    for f in files {
        let name = label(f);
        let pad = " ".repeat(name_w - name.chars().count());
        let bar = if f.patch.is_none() {
            "Bin".to_string()
        } else {
            format!(
                "{:>num_w$} {}{}",
                f.adds + f.dels,
                painter.paint(Role::Success, &"+".repeat(scale(f.adds))),
                painter.paint(Role::Danger, &"-".repeat(scale(f.dels)))
            )
        };
        out.push_str(&format!(" {name}{pad} | {bar}\n"));
    }
    let adds: u32 = files.iter().map(|f| f.adds).sum();
    let dels: u32 = files.iter().map(|f| f.dels).sum();
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            one.to_string()
        } else {
            many.to_string()
        }
    };
    out.push_str(&format!(
        " {} {} changed, {adds} {}(+), {dels} {}(-)\n",
        files.len(),
        plural(files.len(), "file", "files"),
        plural(adds as usize, "insertion", "insertions"),
        plural(dels as usize, "deletion", "deletions"),
    ));
    out
}

#[cfg(all(test, feature = "demo"))]
mod tests {
    use super::*;
    use crate::cli::GlobalArgs;
    use crate::cmd::context::Terminal;
    use crate::cmd::output::colour::strip_ansi;
    use std::process::Command;

    fn pipe() -> Terminal {
        Terminal {
            stdout_tty: false,
            stdin_tty: false,
            width: None,
        }
    }

    fn demo_files() -> (Context, Vec<FilePatch>) {
        let args = GlobalArgs {
            demo: true,
            repo: Some("liminal-hq/review-buddy".into()),
            sources: vec!["liminal-hq".into()],
            frozen_time: Some("2026-10-05T10:00".into()),
            ..GlobalArgs::default()
        };
        let ctx = Context::build(args, pipe()).unwrap();
        let target = ctx.resolve_selector(Some("214")).unwrap();
        let files = fetch(&ctx, &target).unwrap();
        (ctx, files)
    }

    fn file(path: &str, adds: u32, dels: u32, patch: Option<&str>) -> FilePatch {
        FilePatch {
            path: path.into(),
            old_path: None,
            status: FileStatus::Modified,
            adds,
            dels,
            patch: patch.map(Into::into),
        }
    }

    #[test]
    fn raw_patch_has_git_headers_and_the_forge_body_verbatim() {
        let body = "@@ -1 +1 @@\n-a\n+b\n";
        let out = raw_patch(&[file("x.rs", 1, 1, Some(body))]);
        assert_eq!(
            out,
            format!("diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n{body}")
        );
    }

    #[test]
    fn added_removed_and_renamed_files_get_matching_headers() {
        let body = "@@ -0,0 +1 @@\n+a\n";
        let mut added = file("n.rs", 1, 0, Some(body));
        added.status = FileStatus::Added;
        let out = raw_patch(&[added]);
        assert!(out.contains("new file mode 100644\n--- /dev/null\n+++ b/n.rs\n"));

        let mut gone = file("g.rs", 0, 1, Some("@@ -1 +0,0 @@\n-a\n"));
        gone.status = FileStatus::Removed;
        assert!(
            raw_patch(&[gone]).contains("deleted file mode 100644\n--- a/g.rs\n+++ /dev/null\n")
        );

        let mut moved = file("new.rs", 0, 0, Some(""));
        moved.status = FileStatus::Renamed;
        moved.old_path = Some("old.rs".into());
        assert_eq!(
            raw_patch(&[moved]),
            "diff --git a/old.rs b/new.rs\nsimilarity index 100%\nrename from old.rs\nrename to new.rs\n"
        );
    }

    #[test]
    fn files_without_a_patch_are_skipped_but_still_named() {
        let files = [
            file("a.png", 0, 0, None),
            file("b.rs", 1, 0, Some("@@ -1 +1,2 @@\n a\n+b\n")),
        ];
        let out = raw_patch(&files);
        assert!(!out.contains("a.png") && out.contains("b.rs"));
        assert_eq!(name_only(&files), "a.png\nb.rs\n");
        assert!(skipped_note(&files[0]).starts_with("Skipped a.png"));
        assert!(stat(&files, 80, &Painter::plain()).contains("a.png | Bin"));
    }

    #[test]
    fn file_filter_matches_paths_and_directories_and_reports_misses() {
        let files = vec![file("src/a.rs", 1, 0, None), file("docs/b.md", 1, 0, None)];
        let kept = filter_files(files.clone(), &["src".into()]).unwrap();
        assert_eq!(kept.len(), 1);
        let both = filter_files(files.clone(), &["src/a.rs".into(), "docs/b.md".into()]).unwrap();
        assert_eq!(both.len(), 2);
        let err = filter_files(files, &["nope".into()]).unwrap_err();
        assert_eq!(err.exit().code(), 2);
    }

    #[test]
    fn stat_scales_bars_and_totals() {
        let files = [file("a.rs", 30, 10, None), file("b.rs", 1, 0, None)];
        let out = stat(&files, 40, &Painter::plain());
        let first = out.lines().next().unwrap();
        assert!(first.chars().count() <= 40, "{first}");
        assert!(out.contains("2 files changed, 31 insertions(+), 10 deletions(-)"));
        let one = stat(&[file("c", 1, 1, Some(""))], 80, &Painter::plain());
        assert!(one.contains("1 file changed, 1 insertion(+), 1 deletion(-)"));
    }

    #[test]
    fn coloured_output_strips_back_to_the_raw_patch() {
        let (_, files) = demo_files();
        let painter = Painter::new(
            rb_theme::Palette::new(
                rb_theme::Theme::default(),
                rb_theme::ColourDepth::TrueColour,
                false,
            ),
            true,
        );
        let coloured = coloured_patch(&files, &painter);
        assert!(coloured.contains('\u{1b}'));
        let raw = raw_patch(&files);
        let strip = |s: &str| {
            strip_ansi(s)
                .lines()
                .map(|l| l.trim_end().to_string())
                .collect::<Vec<_>>()
        };
        let plain = strip(&coloured);
        let expected = strip(&raw);
        assert_eq!(plain.len(), expected.len());
        for (a, b) in plain.iter().zip(&expected) {
            if !b.starts_with("@@") {
                assert_eq!(a, b);
            }
        }
    }

    /// Rebuilds a pre-image that a patch applies to, placing known lines at their old line
    /// numbers and filling the gaps.
    fn pre_image(file: &FilePatch) -> String {
        let parsed = rb_diff::parse_patch(file.patch.as_deref().unwrap());
        let mut lines: Vec<String> = Vec::new();
        for (_, line) in parsed.iter() {
            if let (Some(n), true) = (line.old_no, line.kind != LineKind::Added) {
                let n = n as usize;
                while lines.len() < n {
                    lines.push(format!("filler {}", lines.len() + 1));
                }
                lines[n - 1] = line.text.clone();
            }
        }
        lines.into_iter().map(|l| l + "\n").collect()
    }

    #[test]
    fn the_demo_patch_applies_with_git_apply() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let (_, files) = demo_files();
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false"])
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap()
        };
        assert!(git(&["init", "-q"]).status.success());
        for f in files.iter().filter(|f| f.status == FileStatus::Modified) {
            let path = dir.path().join(&f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, pre_image(f)).unwrap();
        }
        std::fs::write(dir.path().join("change.patch"), raw_patch(&files)).unwrap();
        let out = git(&["apply", "--check", "change.patch"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
