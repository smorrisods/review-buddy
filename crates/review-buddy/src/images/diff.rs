//! Image files in a diff: which files count, where their two versions come from, and how the
//! pair is laid out. Pure; drawing is `ui::image_diff`.

use rb_core::{ChangeId, FileStatus, ForgeKind, Source};
use url::Url;

/// Formats the image decoder can read. SVG is text, so it keeps its text diff.
const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Whether `path` names a picture we can show.
pub fn is_image_path(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// How the original and the replacement sit in the diff pane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Layout {
    /// Original above, replacement below.
    #[default]
    Stacked,
    SideBySide,
    /// Only the original, as large as the pane allows.
    Original,
    /// Only the replacement, as large as the pane allows.
    Replacement,
}

impl Layout {
    pub const ALL: [Layout; 4] = [
        Layout::Stacked,
        Layout::SideBySide,
        Layout::Original,
        Layout::Replacement,
    ];

    pub fn next(self) -> Self {
        let at = Self::ALL.iter().position(|l| *l == self).unwrap_or(0);
        Self::ALL[(at + 1) % Self::ALL.len()]
    }

    /// Calm copy for the status line and the help text.
    pub fn label(self) -> &'static str {
        match self {
            Layout::Stacked => "original above, replacement below",
            Layout::SideBySide => "original left, replacement right",
            Layout::Original => "original only",
            Layout::Replacement => "replacement only",
        }
    }
}

/// The two versions of an image file, as addresses to fetch. Either side is `None` when the
/// file doesn't exist there (added or removed), or when no address can be built.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pair {
    pub original: Option<Url>,
    pub replacement: Option<Url>,
}

/// Where each version of a file lives on the forge's API. `base_sha` and `head_sha` are the
/// change's two commits; a renamed file's original is read from its old path.
pub fn pair(
    source: &Source,
    id: &ChangeId,
    base_sha: &str,
    head_sha: &str,
    path: &str,
    old_path: Option<&str>,
    status: FileStatus,
) -> Pair {
    let blob = |sha: &str, p: &str| blob_url(source, id, sha, p);
    let original = match status {
        FileStatus::Added => None,
        _ => blob(base_sha, old_path.unwrap_or(path)),
    };
    let replacement = match status {
        FileStatus::Removed => None,
        _ => blob(head_sha, path),
    };
    Pair {
        original,
        replacement,
    }
}

/// The API address of `path` at `sha`: GitHub's contents API (the fetcher asks for the raw
/// media type) or GitLab's raw file endpoint.
fn blob_url(source: &Source, id: &ChangeId, sha: &str, path: &str) -> Option<Url> {
    if sha.is_empty() || path.is_empty() {
        return None;
    }
    let host = source
        .host
        .trim()
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let mut url = match id.kind {
        ForgeKind::GitHub => {
            let root = if host == "github.com" || host == "api.github.com" {
                "https://api.github.com".to_string()
            } else {
                let web = host.strip_prefix("api.").unwrap_or(&host);
                format!("https://{web}/api/v3")
            };
            let mut url = Url::parse(&root).ok()?;
            let mut segments = url.path_segments_mut().ok()?;
            segments.pop_if_empty().push("repos");
            segments.extend(id.repo.split('/'));
            segments.push("contents");
            segments.extend(path.split('/'));
            drop(segments);
            url
        }
        ForgeKind::GitLab => {
            let mut url = Url::parse(&format!("https://{host}/api/v4")).ok()?;
            let mut segments = url.path_segments_mut().ok()?;
            segments.pop_if_empty().extend(["projects", &id.repo]);
            segments.extend(["repository", "files", path, "raw"]);
            drop(segments);
            url
        }
    };
    url.query_pairs_mut().append_pair("ref", sha);
    Some(url)
}

/// A rectangle in cells, relative to the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// Where the original and the replacement are drawn, each with one row above it for its caption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panels {
    pub original: Option<Cell>,
    pub replacement: Option<Cell>,
}

/// Splits a pane of `width` by `height` cells for `layout`. A side-by-side pane too narrow to
/// give each picture room (under 40 columns) stacks instead, and a stacked pane too short
/// (under 8 rows) shows the replacement alone.
pub fn panels(layout: Layout, width: u16, height: u16) -> Panels {
    let whole = Cell {
        x: 0,
        y: 0,
        width,
        height,
    };
    let layout = match layout {
        Layout::SideBySide if width < 40 => Layout::Stacked,
        Layout::Stacked if height < 8 => Layout::Replacement,
        other => other,
    };
    match layout {
        Layout::Original => Panels {
            original: Some(whole),
            replacement: None,
        },
        Layout::Replacement => Panels {
            original: None,
            replacement: Some(whole),
        },
        Layout::Stacked => {
            let top = height / 2;
            Panels {
                original: Some(Cell {
                    height: top,
                    ..whole
                }),
                replacement: Some(Cell {
                    y: top,
                    height: height - top,
                    ..whole
                }),
            }
        }
        Layout::SideBySide => {
            let left = width / 2;
            Panels {
                original: Some(Cell {
                    width: left,
                    ..whole
                }),
                replacement: Some(Cell {
                    x: left,
                    width: width - left,
                    ..whole
                }),
            }
        }
    }
}

/// Cells `(columns, rows)` for a picture of `pixels` inside `cols` by `rows` cells, keeping its
/// aspect ratio and never drawing it larger than its natural size, so two pictures of the same
/// size always look the same size.
pub fn fit_in(pixels: (u32, u32), font: (u16, u16), cols: u16, rows: u16) -> (u16, u16) {
    let (iw, ih) = (f64::from(pixels.0.max(1)), f64::from(pixels.1.max(1)));
    let (fw, fh) = (f64::from(font.0.max(1)), f64::from(font.1.max(1)));
    let scale = (f64::from(cols.max(1)) * fw / iw)
        .min(f64::from(rows.max(1)) * fh / ih)
        .min(1.0);
    let c = ((iw * scale / fw).ceil() as u16).clamp(1, cols.max(1));
    let r = ((ih * scale / fh).ceil() as u16).clamp(1, rows.max(1));
    (c, r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::{AuthMode, Scope, SourceId};

    fn source(kind: ForgeKind, host: &str) -> Source {
        Source {
            id: SourceId("s".into()),
            kind,
            host: host.into(),
            label: "s".into(),
            scope: Scope::default(),
            auth: AuthMode::Cli,
            in_all: true,
            include_drafts: false,
            tag_colour: None,
        }
    }

    fn id(kind: ForgeKind, repo: &str) -> ChangeId {
        ChangeId {
            source_id: SourceId("s".into()),
            kind,
            repo: repo.into(),
            number: 1,
        }
    }

    #[test]
    fn image_paths_are_found_by_extension_ignoring_case() {
        assert!(is_image_path("a/b/shot.PNG"));
        assert!(is_image_path("x.webp"));
        assert!(!is_image_path("logo.svg"));
        assert!(!is_image_path("png"));
        assert!(!is_image_path("src/lib.rs"));
    }

    #[test]
    fn layouts_cycle_through_all_four_and_back() {
        let mut l = Layout::default();
        let mut seen = vec![l];
        for _ in 0..3 {
            l = l.next();
            seen.push(l);
        }
        assert_eq!(seen, Layout::ALL);
        assert_eq!(l.next(), Layout::Stacked);
    }

    #[test]
    fn github_dot_com_uses_the_contents_api_with_encoded_paths() {
        let p = pair(
            &source(ForgeKind::GitHub, "github.com"),
            &id(ForgeKind::GitHub, "o/r"),
            "b1",
            "h1",
            "dir/my shot.png",
            None,
            FileStatus::Modified,
        );
        assert_eq!(
            p.replacement.unwrap().as_str(),
            "https://api.github.com/repos/o/r/contents/dir/my%20shot.png?ref=h1"
        );
        assert_eq!(
            p.original.unwrap().as_str(),
            "https://api.github.com/repos/o/r/contents/dir/my%20shot.png?ref=b1"
        );
    }

    #[test]
    fn enterprise_github_uses_the_api_v3_prefix() {
        let p = pair(
            &source(ForgeKind::GitHub, "ghe.example.com"),
            &id(ForgeKind::GitHub, "o/r"),
            "b",
            "h",
            "a.png",
            None,
            FileStatus::Added,
        );
        assert!(p.original.is_none());
        assert_eq!(
            p.replacement.unwrap().as_str(),
            "https://ghe.example.com/api/v3/repos/o/r/contents/a.png?ref=h"
        );
    }

    #[test]
    fn gitlab_encodes_the_project_and_path_and_reads_a_renamed_original_from_its_old_path() {
        let p = pair(
            &source(ForgeKind::GitLab, "gitlab.com"),
            &id(ForgeKind::GitLab, "grp/sub/proj"),
            "b",
            "h",
            "new/a.png",
            Some("old/a.png"),
            FileStatus::Renamed,
        );
        assert_eq!(
            p.original.unwrap().as_str(),
            "https://gitlab.com/api/v4/projects/grp%2Fsub%2Fproj/repository/files/old%2Fa.png/raw?ref=b"
        );
        assert!(p
            .replacement
            .unwrap()
            .as_str()
            .contains("files/new%2Fa.png/raw"));
    }

    #[test]
    fn a_removed_file_has_no_replacement() {
        let p = pair(
            &source(ForgeKind::GitLab, "gitlab.com"),
            &id(ForgeKind::GitLab, "g/p"),
            "b",
            "h",
            "a.png",
            None,
            FileStatus::Removed,
        );
        assert!(p.original.is_some() && p.replacement.is_none());
    }

    #[test]
    fn panels_split_the_pane_for_each_layout() {
        let s = panels(Layout::Stacked, 80, 20);
        assert_eq!(
            (s.original.unwrap().height, s.replacement.unwrap().y),
            (10, 10)
        );
        let b = panels(Layout::SideBySide, 80, 20);
        assert_eq!(
            (b.original.unwrap().width, b.replacement.unwrap().x),
            (40, 40)
        );
        assert!(panels(Layout::Original, 80, 20).replacement.is_none());
        assert!(panels(Layout::Replacement, 80, 20).original.is_none());
    }

    #[test]
    fn cramped_panes_fall_back_to_something_that_fits() {
        assert!(panels(Layout::SideBySide, 30, 20).original.unwrap().width == 30);
        let short = panels(Layout::Stacked, 80, 6);
        assert!(short.original.is_none() && short.replacement.is_some());
    }

    #[test]
    fn pictures_fit_the_cell_without_growing_past_their_size() {
        assert_eq!(fit_in((100, 40), (10, 20), 40, 10), (10, 2));
        let (c, r) = fit_in((2000, 1000), (10, 20), 40, 10);
        assert!(c <= 40 && r <= 10);
        assert_eq!(fit_in((1, 1), (10, 20), 40, 10), (1, 1));
    }
}
