//! Output for commands: tables on a terminal, TSV on a pipe, colour, JSON and the pager.

pub mod colour;
pub mod json;
pub mod pager;
pub mod table;

use std::io::Write;

pub use colour::{colour_enabled, Painter};
pub use table::{render_table, render_tsv, Cell, Column, Table};

use super::error::CmdError;

/// How stdout should be formatted for this run.
#[derive(Debug, Clone)]
pub struct OutputMode {
    /// stdout is a terminal.
    pub tty: bool,
    pub width: usize,
    pub painter: Painter,
    pub pager: Option<Vec<String>>,
}

impl OutputMode {
    pub fn colour(&self) -> bool {
        self.painter.enabled()
    }

    /// Renders a table for this mode: aligned and truncated on a terminal, TSV otherwise.
    pub fn table(&self, table: &Table) -> String {
        if self.tty {
            render_table(table, self.width, &self.painter)
        } else {
            render_tsv(table)
        }
    }
}

/// Writes to stdout. A closed pipe (`| head`) ends quietly instead of failing.
pub fn print(text: &str) -> Result<(), CmdError> {
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other.map_err(CmdError::from),
    }
}

/// Prints through the pager on a terminal, and plainly otherwise.
pub fn print_paged(mode: &OutputMode, text: &str) -> Result<(), CmdError> {
    if mode.tty {
        pager::page(text, mode.pager.as_deref())?;
        Ok(())
    } else {
        print(text)
    }
}
