use std::io::Write;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Value};

use super::error::ConfigError;
use super::schema::Config;

/// Edits a config file in place, keeping comments, ordering and formatting.
///
/// Open the write target, change keys, then `save`. The result is checked
/// against the schema before anything touches the disk.
#[derive(Debug)]
pub struct ConfigEditor {
    path: PathBuf,
    doc: DocumentMut,
}

impl ConfigEditor {
    /// Opens `path`; a missing file starts as an empty document.
    pub fn open(path: &Path) -> Result<Self, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: path.to_path_buf(),
                    source,
                })
            }
        };
        let doc = text
            .parse::<DocumentMut>()
            .map_err(|e| ConfigError::invalid(path, &text, e.span(), e.message()))?;
        Ok(Self {
            path: path.to_path_buf(),
            doc,
        })
    }

    /// Starts from `text` as if it had been read from `path`. Nothing is written until `save`.
    pub fn from_text(path: &Path, text: &str) -> Result<Self, ConfigError> {
        let doc = text
            .parse::<DocumentMut>()
            .map_err(|e| ConfigError::invalid(path, text, e.span(), e.message()))?;
        Ok(Self {
            path: path.to_path_buf(),
            doc,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The document as it would be written.
    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    /// Sets `table.key = value`, creating the table when needed. An existing
    /// value keeps its trailing comment.
    pub fn set(&mut self, table: &str, key: &str, value: impl Into<Value>) {
        let root = self.doc.as_table_mut();
        if !root.get(table).is_some_and(Item::is_table) {
            let mut t = toml_edit::Table::new();
            t.set_implicit(false);
            root.insert(table, Item::Table(t));
        }
        let table = root[table].as_table_mut().expect("table was just ensured");
        let mut new: Value = value.into();
        if let Some(old) = table.get(key).and_then(Item::as_value) {
            let decor = old.decor();
            *new.decor_mut() = decor.clone();
        }
        table.insert(key, Item::Value(new));
    }

    /// Removes `table.key`, returning whether it existed.
    pub fn remove(&mut self, table: &str, key: &str) -> bool {
        self.doc
            .get_mut(table)
            .and_then(Item::as_table_mut)
            .is_some_and(|t| t.remove(key).is_some())
    }

    /// Validates, then writes atomically with mode `0600`.
    pub fn save(&self) -> Result<(), ConfigError> {
        let text = self.text();
        toml::from_str::<Config>(&text)
            .map_err(|e| ConfigError::invalid(&self.path, &text, e.span(), e.message()))?;
        write_atomic(&self.path, text.as_bytes())
    }
}

/// Writes a temporary file beside `path`, syncs it, then renames it over `path`.
/// Parent directories are created `0700` and the file is `0600`.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), ConfigError> {
    let fail = |source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    };
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        rb_paths::ensure_private_dir(parent).map_err(fail)?;
    }
    let name = path.file_name().map_or_else(
        || "config.toml".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));

    let result = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# My config
[ui]
theme = \"liminal-hq\"   # the default
layout = \"panes\"

# work stuff
[[source]]
name = \"a\"
kind = \"github\"
host = \"github.com\"
";

    fn write(dir: &Path, text: &str) -> PathBuf {
        let p = dir.join("config.toml");
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn untouched_round_trip_is_byte_identical() {
        let t = tempfile::tempdir().unwrap();
        let p = write(t.path(), SAMPLE);
        let ed = ConfigEditor::open(&p).unwrap();
        ed.save().unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), SAMPLE);
    }

    #[test]
    fn set_keeps_comments_order_and_trailing_comment() {
        let t = tempfile::tempdir().unwrap();
        let p = write(t.path(), SAMPLE);
        let mut ed = ConfigEditor::open(&p).unwrap();
        ed.set("ui", "theme", "afterglow-dark");
        ed.set("ui", "jax", false);
        ed.save().unwrap();
        let out = std::fs::read_to_string(&p).unwrap();
        assert!(
            out.starts_with("# My config\n[ui]\ntheme = \"afterglow-dark\"   # the default\n"),
            "{out}"
        );
        assert!(out.contains("# work stuff\n[[source]]"));
        assert!(out.contains("jax = false"));
        let theme = out.find("theme").unwrap();
        let layout = out.find("layout").unwrap();
        assert!(theme < layout);
    }

    #[test]
    fn set_creates_file_and_table_with_private_modes() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("new/dir/config.toml");
        let mut ed = ConfigEditor::open(&p).unwrap();
        ed.set("review", "merge_method", "rebase");
        ed.save().unwrap();
        let loaded: Config = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(
            loaded.review.merge_method,
            super::super::MergeMethod::Rebase
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&p), 0o600);
            assert_eq!(mode(p.parent().unwrap()), 0o700);
        }
    }

    #[test]
    fn remove_drops_the_key_only() {
        let t = tempfile::tempdir().unwrap();
        let p = write(t.path(), SAMPLE);
        let mut ed = ConfigEditor::open(&p).unwrap();
        assert!(ed.remove("ui", "layout"));
        assert!(!ed.remove("ui", "layout"));
        assert!(!ed.remove("nope", "x"));
        assert!(!ed.text().contains("layout"));
        assert!(ed.text().contains("# the default"));
    }

    #[test]
    fn invalid_edits_are_refused_and_leave_the_file_alone() {
        let t = tempfile::tempdir().unwrap();
        let p = write(t.path(), SAMPLE);
        let mut ed = ConfigEditor::open(&p).unwrap();
        ed.set("ui", "layout", "wide");
        let e = ed.save().unwrap_err();
        assert!(matches!(e, ConfigError::Invalid { .. }));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), SAMPLE);
    }

    #[test]
    fn opening_a_broken_file_names_the_line() {
        let t = tempfile::tempdir().unwrap();
        let p = write(t.path(), "[ui]\ntheme = \n");
        let e = ConfigEditor::open(&p).unwrap_err();
        assert_eq!(e.line(), Some(2));
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("config.toml");
        write_atomic(&p, b"a = 1\n").unwrap();
        write_atomic(&p, b"a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a = 2\n");
        let names: Vec<_> = std::fs::read_dir(t.path()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn failed_write_reports_the_path() {
        let t = tempfile::tempdir().unwrap();
        let blocker = t.path().join("file");
        std::fs::write(&blocker, "x").unwrap();
        let e = write_atomic(&blocker.join("config.toml"), b"").unwrap_err();
        assert!(matches!(e, ConfigError::Write { .. }));
        assert!(e.to_string().contains("config.toml"));
    }
}
