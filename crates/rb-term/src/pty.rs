//! Spawns a command in a pseudo-terminal (ConPTY on Windows) through `portable-pty`.
//!
//! Output and exit arrive through a callback from helper threads, so the host can turn them into
//! messages. Dropping a [`Pty`] stops the child.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

/// `CSI ? 9001 l`: turns win32-input-mode off. ConPTY can leave a host terminal in that mode when a
/// child exits abnormally, after which keys arrive as unreadable sequences.
pub const WIN32_INPUT_MODE_OFF: &[u8] = b"\x1b[?9001l";

/// What to write to the host terminal when the pane closes and on exit. Empty off Windows.
pub fn host_cleanup() -> &'static [u8] {
    if cfg!(windows) {
        WIN32_INPUT_MODE_OFF
    } else {
        b""
    }
}

/// Variables that describe the host terminal's own extras (graphics, tabs). The pane doesn't
/// provide them, so the child shouldn't be told they exist.
const HOST_ONLY_VARS: &[&str] = &[
    "KITTY_WINDOW_ID",
    "KITTY_PID",
    "KITTY_INSTALLATION_DIR",
    "ITERM_SESSION_ID",
    "WEZTERM_PANE",
    "WEZTERM_UNIX_SOCKET",
    "WEZTERM_EXECUTABLE",
    "KONSOLE_VERSION",
    "GHOSTTY_RESOURCES_DIR",
];

/// What to run and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Extra variables on top of the inherited environment.
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnError(pub String);

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SpawnError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyEvent {
    Output(Vec<u8>),
    /// The child ended. `None` when its status couldn't be read.
    Exited(Option<u32>),
}

/// The shell to run when none is configured: `$SHELL` or `/bin/sh`, `%COMSPEC%` or `cmd.exe` on
/// Windows.
pub fn default_shell(var: &dyn Fn(&str) -> Option<String>) -> String {
    let get = |k: &str| var(k).filter(|v| !v.trim().is_empty());
    if cfg!(windows) {
        get("COMSPEC").unwrap_or_else(|| "cmd.exe".to_string())
    } else {
        get("SHELL").unwrap_or_else(|| "/bin/sh".to_string())
    }
}

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    finished: Arc<AtomicBool>,
}

impl std::fmt::Debug for Pty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pty").finish_non_exhaustive()
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(2),
        pixel_width: 0,
        pixel_height: 0,
    }
}

impl Pty {
    /// Starts the command. `on_event` is called from helper threads with its output and, last, its
    /// exit.
    pub fn spawn(
        spec: &SpawnSpec,
        on_event: Arc<dyn Fn(PtyEvent) + Send + Sync>,
    ) -> Result<Self, SpawnError> {
        let fail = |what: &str, err: &dyn std::fmt::Display| {
            SpawnError(format!(
                "Couldn't start {}: {what} ({err}). Check ui.terminal.command in your config.",
                spec.program
            ))
        };
        let pair = native_pty_system()
            .openpty(size(spec.cols, spec.rows))
            .map_err(|e| fail("no pseudo-terminal", &e))?;
        let mut cmd = CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        // portable-pty starts in the home directory unless told otherwise; "no directory" here
        // means where Review Buddy was started.
        match &spec.cwd {
            Some(dir) => cmd.cwd(dir),
            None => {
                if let Ok(dir) = std::env::current_dir() {
                    cmd.cwd(dir);
                }
            }
        }
        for var in HOST_ONLY_VARS {
            cmd.env_remove(var);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "review-buddy");
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| fail("it wouldn't run", &e))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| fail("no output stream", &e))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| fail("no input stream", &e))?;
        let killer = child.clone_killer();
        let finished = Arc::new(AtomicBool::new(false));

        let out = Arc::clone(&on_event);
        let done = Arc::clone(&finished);
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 16 * 1024];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                out(PtyEvent::Output(buf[..n].to_vec()));
            }
            done.store(true, Ordering::SeqCst);
        });

        let drained = Arc::clone(&finished);
        std::thread::spawn(move || {
            let code = child.wait().ok().map(|s| s.exit_code());
            // Let the reader finish what the child wrote last. ConPTY's reader may never see EOF,
            // so don't wait for it for long.
            for _ in 0..40 {
                if drained.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            on_event(PtyEvent::Exited(code));
        });

        Ok(Self {
            master: pair.master,
            writer,
            killer,
            finished,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(size(cols, rows));
    }

    /// Stops the child, if it is still running.
    pub fn kill(&mut self) {
        let _ = self.killer.kill();
    }

    /// Whether the child's output has ended.
    pub fn output_ended(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_prefers_the_environment() {
        let var = |k: &str| match k {
            "SHELL" => Some("/bin/zsh".to_string()),
            "COMSPEC" => Some("C:\\x\\pwsh.exe".to_string()),
            _ => None,
        };
        let want = if cfg!(windows) {
            "C:\\x\\pwsh.exe"
        } else {
            "/bin/zsh"
        };
        assert_eq!(default_shell(&var), want);
        let fallback = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
        assert_eq!(default_shell(&|_| None), fallback);
        assert_eq!(default_shell(&|_| Some("  ".into())), fallback);
    }

    #[test]
    fn win32_input_mode_is_turned_off_on_cleanup() {
        assert_eq!(WIN32_INPUT_MODE_OFF, b"\x1b[?9001l");
        assert_eq!(host_cleanup().is_empty(), !cfg!(windows));
    }

    #[test]
    fn a_missing_program_says_what_to_check() {
        let spec = SpawnSpec {
            program: "/definitely/not/here".into(),
            args: vec![],
            cwd: None,
            env: vec![],
            cols: 80,
            rows: 24,
        };
        let err = Pty::spawn(&spec, Arc::new(|_| {})).unwrap_err();
        assert!(err.0.contains("ui.terminal.command"), "{err}");
    }
}
