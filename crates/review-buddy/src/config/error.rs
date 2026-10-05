use std::fmt;
use std::path::{Path, PathBuf};

/// Where in a file a problem was found. Both numbers are 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

impl Location {
    /// Converts a byte offset in `text` into a line and column.
    pub fn from_offset(text: &str, offset: usize) -> Self {
        let offset = offset.min(text.len());
        let before = &text.as_bytes()[..offset];
        let line = before.iter().filter(|b| **b == b'\n').count() + 1;
        let line_start = before
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |i| i + 1);
        let column = text[line_start..]
            .char_indices()
            .take_while(|(i, _)| line_start + i < offset)
            .count()
            + 1;
        Self { line, column }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Couldn't read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}", format_problem(path, *location, message))]
    Invalid {
        path: PathBuf,
        location: Option<Location>,
        message: String,
    },
    #[error("Couldn't write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl ConfigError {
    pub(crate) fn invalid(
        path: &Path,
        text: &str,
        span: Option<std::ops::Range<usize>>,
        message: impl fmt::Display,
    ) -> Self {
        Self::Invalid {
            path: path.to_path_buf(),
            location: span.map(|s| Location::from_offset(text, s.start)),
            message: message.to_string(),
        }
    }

    /// The path of the file the problem is in.
    pub fn path(&self) -> &Path {
        match self {
            Self::Read { path, .. } | Self::Invalid { path, .. } | Self::Write { path, .. } => path,
        }
    }

    /// The line of the problem, when one is known.
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::Invalid { location, .. } => location.map(|l| l.line),
            _ => None,
        }
    }
}

fn format_problem(path: &Path, location: Option<Location>, message: &str) -> String {
    match location {
        Some(l) => format!("{}:{}:{}: {message}", path.display(), l.line, l.column),
        None => format!("{}: {message}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_become_lines_and_columns() {
        let text = "a = 1\nbb = 2\n";
        assert_eq!(
            Location::from_offset(text, 0),
            Location { line: 1, column: 1 }
        );
        assert_eq!(
            Location::from_offset(text, 9),
            Location { line: 2, column: 4 }
        );
        assert_eq!(Location::from_offset(text, 999).line, 3);
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let text = "é = 1";
        assert_eq!(Location::from_offset(text, 3).column, 3);
    }

    #[test]
    fn display_names_file_line_and_column() {
        let e = ConfigError::invalid(Path::new("/c/config.toml"), "x\ny = 1", Some(2..3), "bad");
        assert_eq!(e.to_string(), "/c/config.toml:2:1: bad");
        assert_eq!(e.line(), Some(2));
        let e = ConfigError::invalid(Path::new("/c/config.toml"), "", None, "bad");
        assert_eq!(e.to_string(), "/c/config.toml: bad");
    }
}
