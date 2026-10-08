//! The remembered panel layout: `session.toml` in the state directory.
//!
//! Only what the person changed is stored, so a setting they never touched keeps following
//! `config.toml`. Reading never fails: a missing, empty or corrupt file is an empty session,
//! and a key that doesn't parse is skipped while the rest are kept. Nothing here is used in
//! demo mode or when `ui.remember_layout` is off; callers decide that by not passing a path.

use std::io;
use std::path::{Path, PathBuf};

use rb_theme::BackgroundMode;
use toml::{Table, Value};

use crate::config::{DetailMode, DetailPosition, SourcesLayout};
use crate::ui::layout::{Options, Size};

/// The path of the remembered layout under a state directory.
pub fn file_in(state_dir: &Path) -> PathBuf {
    state_dir.join(rb_paths::SESSION_FILE)
}

/// The layout keys, each `None` until the person changes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionLayout {
    pub detail_position: Option<DetailPosition>,
    pub sources: Option<SourcesLayout>,
    pub detail: Option<DetailMode>,
    pub queue_width: Option<Size>,
    pub queue_height: Option<Size>,
    pub sources_width: Option<Size>,
    pub background: Option<BackgroundMode>,
    /// Where the terminal pane sits and how big it is.
    pub terminal_position: Option<DetailPosition>,
    pub terminal_size: Option<Size>,
    /// Soft wrap in the diff (`z`).
    pub diff_wrap: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Session {
    pub layout: SessionLayout,
}

fn pick<T: Copy>(all: &[T], text: &str, name: fn(T) -> &'static str) -> Option<T> {
    let text = text.trim().to_ascii_lowercase();
    all.iter().copied().find(|v| name(*v) == text)
}

fn text<'a>(table: &'a Table, key: &str) -> Option<&'a str> {
    table.get(key)?.as_str()
}

fn size(table: &Table, key: &str) -> Option<Size> {
    match table.get(key)? {
        Value::Integer(n) => Size::parse(&n.to_string()).ok(),
        Value::String(s) => Size::parse(s).ok(),
        _ => None,
    }
}

fn size_value(size: Size) -> Value {
    match size {
        Size::Cells(n) => Value::Integer(i64::from(n)),
        Size::Percent(_) => Value::String(size.to_string()),
    }
}

impl Session {
    /// Reads the text of a session file. Never fails.
    pub fn parse(source: &str) -> Self {
        let Ok(root) = source.parse::<Table>() else {
            return Self::default();
        };
        let Some(table) = root.get("layout").and_then(Value::as_table) else {
            return Self::default();
        };
        Self {
            layout: SessionLayout {
                detail_position: text(table, "detail_position").and_then(|v| {
                    pick(
                        &[
                            DetailPosition::Auto,
                            DetailPosition::Right,
                            DetailPosition::Left,
                            DetailPosition::Top,
                            DetailPosition::Bottom,
                        ],
                        v,
                        DetailPosition::as_str,
                    )
                }),
                sources: text(table, "sources").and_then(|v| {
                    pick(
                        &[SourcesLayout::Auto, SourcesLayout::Left, SourcesLayout::Top],
                        v,
                        SourcesLayout::as_str,
                    )
                }),
                detail: text(table, "detail").and_then(|v| {
                    pick(
                        &[DetailMode::Auto, DetailMode::Open, DetailMode::Closed],
                        v,
                        DetailMode::as_str,
                    )
                }),
                queue_width: size(table, "queue_width"),
                queue_height: size(table, "queue_height"),
                sources_width: size(table, "sources_width"),
                background: text(table, "background").and_then(|v| v.parse().ok()),
                terminal_position: text(table, "terminal_position").and_then(|v| {
                    pick(
                        &[
                            DetailPosition::Auto,
                            DetailPosition::Right,
                            DetailPosition::Left,
                            DetailPosition::Top,
                            DetailPosition::Bottom,
                        ],
                        v,
                        DetailPosition::as_str,
                    )
                }),
                terminal_size: size(table, "terminal_size"),
                diff_wrap: table.get("diff_wrap").and_then(Value::as_bool),
            },
        }
    }

    /// The file's text, with keys in a stable order.
    pub fn to_toml(&self) -> String {
        let l = &self.layout;
        let mut table = Table::new();
        let mut put = |key: &str, value: Option<Value>| {
            if let Some(value) = value {
                table.insert(key.to_string(), value);
            }
        };
        let name = |s: &str| Some(Value::String(s.to_string()));
        put(
            "detail_position",
            l.detail_position.and_then(|v| name(v.as_str())),
        );
        put("sources", l.sources.and_then(|v| name(v.as_str())));
        put("detail", l.detail.and_then(|v| name(v.as_str())));
        put("queue_width", l.queue_width.map(size_value));
        put("queue_height", l.queue_height.map(size_value));
        put("sources_width", l.sources_width.map(size_value));
        put("background", l.background.and_then(|v| name(v.key())));
        put(
            "terminal_position",
            l.terminal_position.and_then(|v| name(v.as_str())),
        );
        put("terminal_size", l.terminal_size.map(size_value));
        put("diff_wrap", l.diff_wrap.map(Value::Boolean));
        let mut root = Table::new();
        root.insert("layout".to_string(), Value::Table(table));
        format!(
            "# Remembered by review-buddy. Delete it with `review-buddy config reset-layout`.\n{}",
            toml::to_string(&root).unwrap_or_default()
        )
    }

    pub fn is_empty(&self) -> bool {
        self.layout == SessionLayout::default()
    }
}

/// Reads the remembered layout, or `None` when there is no file.
pub fn load(path: &Path) -> Option<Session> {
    let text = std::fs::read(path).ok()?;
    Some(Session::parse(&String::from_utf8_lossy(&text)))
}

/// Writes the layout atomically with the file mode `0600` in a private directory. Does nothing
/// when the file already holds the same text.
pub fn save(path: &Path, session: &Session) -> io::Result<()> {
    let text = session.to_toml();
    if std::fs::read(path).is_ok_and(|old| old == text.as_bytes()) {
        return Ok(());
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    rb_paths::write_private_file(&temp, text.as_bytes())?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// What the layout looked like at one moment: the options and the session's background choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub options: Options,
    pub background: Option<BackgroundMode>,
    /// The terminal pane's placement and size.
    pub terminal: (DetailPosition, Option<Size>),
    /// Soft wrap in the diff.
    pub wrap: bool,
}

/// Remembers what changed and decides when to write it. Pure: the runtime does the writing.
#[derive(Debug, Clone)]
pub struct Tracker {
    path: PathBuf,
    seen: Snapshot,
    session: Session,
    written: Session,
    pending: bool,
}

/// What [`Tracker::observe`] wants done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// Ask for [`Tracker::due`] after the debounce delay.
    Schedule,
    /// Write this now (the app is quitting).
    SaveNow(Session),
}

impl Tracker {
    /// `loaded` is what the file held at launch, so launching alone writes nothing.
    pub fn new(path: PathBuf, loaded: Session, now: Snapshot) -> Self {
        Self {
            path,
            seen: now,
            session: loaded,
            written: loaded,
            pending: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Folds in any change since the last look and says what to do about it.
    pub fn observe(&mut self, now: Snapshot, quitting: bool) -> Step {
        let (a, b) = (self.seen, now);
        let l = &mut self.session.layout;
        if a.options.position != b.options.position {
            l.detail_position = Some(b.options.position);
        }
        if a.options.sources != b.options.sources {
            l.sources = Some(b.options.sources);
        }
        if a.options.detail != b.options.detail {
            l.detail = Some(b.options.detail);
        }
        if a.options.split.queue_width != b.options.split.queue_width {
            l.queue_width = b.options.split.queue_width;
        }
        if a.options.split.queue_height != b.options.split.queue_height {
            l.queue_height = b.options.split.queue_height;
        }
        if a.options.split.sources_width != b.options.split.sources_width {
            l.sources_width = b.options.split.sources_width;
        }
        if a.terminal.0 != b.terminal.0 {
            l.terminal_position = Some(b.terminal.0);
        }
        if a.terminal.1 != b.terminal.1 {
            l.terminal_size = b.terminal.1;
        }
        if a.wrap != b.wrap {
            l.diff_wrap = Some(b.wrap);
        }
        if a.background != b.background && b.background.is_some() {
            l.background = b.background;
        }
        self.seen = now;
        let dirty = self.session != self.written;
        if quitting {
            self.pending = false;
            if dirty {
                self.written = self.session;
                return Step::SaveNow(self.session);
            }
        } else if dirty && !self.pending {
            self.pending = true;
            return Step::Schedule;
        }
        Step::Nothing
    }

    /// The debounce delay passed: what to write, if anything changed since the last write.
    pub fn due(&mut self) -> Option<Session> {
        self.pending = false;
        (self.session != self.written).then(|| {
            self.written = self.session;
            self.session
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot {
            options: Options::default(),
            background: None,
            terminal: (DetailPosition::Auto, None),
            wrap: false,
        }
    }

    fn tracker() -> Tracker {
        Tracker::new(PathBuf::from("session.toml"), Session::default(), snap())
    }

    fn full() -> Session {
        Session {
            layout: SessionLayout {
                detail_position: Some(DetailPosition::Left),
                sources: Some(SourcesLayout::Top),
                detail: Some(DetailMode::Closed),
                queue_width: Some(Size::Cells(52)),
                queue_height: Some(Size::Percent(60)),
                sources_width: Some(Size::Cells(30)),
                background: Some(BackgroundMode::Yes),
                terminal_position: Some(DetailPosition::Bottom),
                terminal_size: Some(Size::Percent(35)),
                diff_wrap: Some(true),
            },
        }
    }

    #[test]
    fn the_text_round_trips() {
        assert_eq!(Session::parse(&full().to_toml()), full());
        assert_eq!(
            Session::parse(&Session::default().to_toml()),
            Session::default()
        );
    }

    #[test]
    fn empty_corrupt_and_unknown_files_are_quietly_empty_or_partial() {
        assert!(Session::parse("").is_empty());
        assert!(Session::parse("\u{0}\u{1}not toml [[[").is_empty());
        assert!(Session::parse("layout = 3").is_empty());
        assert!(Session::parse("[other]\nx = 1\n").is_empty());
        let partial = Session::parse(
            "[layout]\ndetail_position = \"sideways\"\nsources = \"top\"\nqueue_width = \"5%\"\nbackground = 7\nfuture = true\n[extra]\n",
        );
        assert_eq!(partial.layout.sources, Some(SourcesLayout::Top));
        assert_eq!(partial.layout.detail_position, None);
        assert_eq!(partial.layout.queue_width, None);
        assert_eq!(partial.layout.background, None);
    }

    #[test]
    fn values_are_case_and_space_tolerant() {
        let s =
            Session::parse("[layout]\ndetail_position = \" Bottom \"\nqueue_height = \"40%\"\n");
        assert_eq!(s.layout.detail_position, Some(DetailPosition::Bottom));
        assert_eq!(s.layout.queue_height, Some(Size::Percent(40)));
    }

    #[test]
    fn saving_and_loading_round_trips_with_private_modes() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_in(&dir.path().join("state/review-buddy"));
        assert_eq!(load(&path), None);
        save(&path, &full()).unwrap();
        assert_eq!(load(&path), Some(full()));
        assert!(!path.with_extension("toml.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
    }

    #[test]
    fn saving_the_same_content_leaves_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.toml");
        save(&path, &full()).unwrap();
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        save(&path, &full()).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), first);
    }

    #[test]
    fn toggling_wrap_is_remembered_and_launching_alone_is_not() {
        let mut t = tracker();
        assert_eq!(t.observe(snap(), false), Step::Nothing);
        let mut now = snap();
        now.wrap = true;
        assert_eq!(t.observe(now, false), Step::Schedule);
        assert_eq!(t.due().unwrap().layout.diff_wrap, Some(true));
        now.wrap = false;
        t.observe(now, false);
        assert_eq!(
            t.due().unwrap().layout.diff_wrap,
            Some(false),
            "turning it off is remembered too"
        );
        assert_eq!(
            Session::parse("[layout]\ndiff_wrap = \"yes\"\n")
                .layout
                .diff_wrap,
            None,
            "a value that isn't a boolean is skipped"
        );
    }

    #[test]
    fn nothing_changed_means_nothing_to_write() {
        let mut t = tracker();
        assert_eq!(t.observe(snap(), false), Step::Nothing);
        assert_eq!(t.observe(snap(), true), Step::Nothing);
        assert_eq!(t.due(), None);
    }

    #[test]
    fn a_burst_of_changes_schedules_one_write_with_the_latest_values() {
        let mut t = tracker();
        let mut now = snap();
        now.options.position = DetailPosition::Right;
        assert_eq!(t.observe(now, false), Step::Schedule);
        now.options.position = DetailPosition::Top;
        assert_eq!(t.observe(now, false), Step::Nothing, "already scheduled");
        now.options.sources = SourcesLayout::Top;
        assert_eq!(t.observe(now, false), Step::Nothing);
        let saved = t.due().unwrap();
        assert_eq!(saved.layout.detail_position, Some(DetailPosition::Top));
        assert_eq!(saved.layout.sources, Some(SourcesLayout::Top));
        assert_eq!(t.due(), None, "nothing new since");
        now.options.detail = DetailMode::Closed;
        assert_eq!(
            t.observe(now, false),
            Step::Schedule,
            "a later change again"
        );
    }

    #[test]
    fn changing_back_before_the_write_still_writes_only_if_different() {
        let mut t = Tracker::new(PathBuf::from("s"), Session::default(), snap());
        let mut now = snap();
        now.options.detail = DetailMode::Closed;
        assert_eq!(t.observe(now, false), Step::Schedule);
        now.options.detail = DetailMode::Open;
        t.observe(now, false);
        let saved = t.due().unwrap();
        assert_eq!(saved.layout.detail, Some(DetailMode::Open));
    }

    #[test]
    fn quitting_writes_at_once_and_only_when_dirty() {
        let mut t = tracker();
        let mut now = snap();
        now.options.split.queue_width = Some(Size::Cells(60));
        match t.observe(now, true) {
            Step::SaveNow(s) => assert_eq!(s.layout.queue_width, Some(Size::Cells(60))),
            other => panic!("{other:?}"),
        }
        assert_eq!(t.due(), None);
        assert_eq!(t.observe(now, true), Step::Nothing);
    }

    #[test]
    fn the_terminal_placement_is_remembered_and_a_reset_forgets_the_size() {
        let mut t = tracker();
        let mut now = snap();
        now.terminal = (DetailPosition::Left, Some(Size::Cells(50)));
        assert_eq!(t.observe(now, false), Step::Schedule);
        let saved = t.due().unwrap();
        assert_eq!(saved.layout.terminal_position, Some(DetailPosition::Left));
        assert_eq!(saved.layout.terminal_size, Some(Size::Cells(50)));
        let mut later = now;
        later.terminal.1 = None;
        t.observe(later, false);
        let saved = t.due().unwrap();
        assert_eq!(saved.layout.terminal_size, None);
        assert_eq!(saved.layout.terminal_position, Some(DetailPosition::Left));
    }

    #[test]
    fn resetting_a_size_forgets_it_and_the_background_follows_the_session_key() {
        let loaded = Session {
            layout: SessionLayout {
                queue_width: Some(Size::Cells(60)),
                ..SessionLayout::default()
            },
        };
        let mut start = snap();
        start.options.split.queue_width = Some(Size::Cells(60));
        let mut t = Tracker::new(PathBuf::from("s"), loaded, start);
        let mut now = snap();
        now.background = Some(BackgroundMode::No);
        assert_eq!(t.observe(now, false), Step::Schedule);
        let saved = t.due().unwrap();
        assert_eq!(saved.layout.queue_width, None);
        assert_eq!(saved.layout.background, Some(BackgroundMode::No));
    }
}
