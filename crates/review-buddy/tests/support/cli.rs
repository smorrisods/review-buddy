//! A sandboxed `review-buddy` command for the CLI tests. Include it with
//! `#[path = "support/cli.rs"] mod sandbox;` (it is separate from `support/mod.rs`, the
//! wiremock stub).
//!
//! The command runs with a cleared environment so nothing from the developer's machine leaks in.
//! Only what a process needs to start is passed through: `PATH`, plus `SystemRoot` and friends
//! on Windows, where an empty environment breaks sockets and home resolution.
#![allow(dead_code)]

use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

pub const FROZEN: &str = "2026-10-05T10:00";

/// Variables kept from the real environment. Everything else is dropped.
const PASSTHROUGH: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SystemRoot",
    "SYSTEMROOT",
    "SystemDrive",
    "windir",
    "COMSPEC",
    "TEMP",
    "TMP",
    "TMPDIR",
];

pub struct Sandbox {
    home: TempDir,
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Sandbox {
    pub fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
        }
    }

    pub fn path(&self) -> &Path {
        self.home.path()
    }

    /// Writes `config.toml` in the sandbox and returns its path, for `--config`.
    pub fn write_config(&self, text: &str) -> std::path::PathBuf {
        let file = self.path().join("config.toml");
        std::fs::write(&file, text).unwrap();
        file
    }

    /// The binary with a throwaway home and XDG tree and colour off.
    pub fn cmd(&self) -> Command {
        let root = self.path();
        let mut cmd = Command::cargo_bin("review-buddy").unwrap();
        cmd.env_clear();
        for key in PASSTHROUGH {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        cmd.env("HOME", root)
            .env("USERPROFILE", root)
            .env("APPDATA", root.join("appdata"))
            .env("LOCALAPPDATA", root.join("localappdata"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_CONFIG_DIRS", root.join("etc"))
            .env("XDG_DATA_DIRS", root.join("share"))
            .env("NO_COLOR", "1");
        cmd
    }

    /// `cmd()` with `--demo --frozen-time 2026-10-05T10:00` already applied.
    pub fn demo(&self) -> Command {
        let mut cmd = self.cmd();
        cmd.args(["--demo", "--frozen-time", FROZEN]);
        cmd
    }
}

/// Lets a command ask for colour even though stdout is a pipe.
pub fn with_colour(cmd: &mut Command) -> &mut Command {
    cmd.env_remove("NO_COLOR").env("CLICOLOR_FORCE", "1")
}

/// A sandboxed command plus the directory that must outlive it.
pub fn sandboxed() -> (Command, TempDir) {
    let sandbox = Sandbox::new();
    let cmd = sandbox.cmd();
    (cmd, sandbox.home)
}

/// A sandboxed demo command plus the directory that must outlive it.
pub fn demo_sandboxed() -> (Command, TempDir) {
    let sandbox = Sandbox::new();
    let cmd = sandbox.demo();
    (cmd, sandbox.home)
}

pub fn stdout_of(cmd: &mut Command) -> String {
    let out = cmd.assert().get_output().stdout.clone();
    normalise(&String::from_utf8(out).unwrap())
}

/// Makes output comparable across machines: LF line endings, the demo temp directory replaced
/// with `<demo>` and its backslashes flipped, and the crate version replaced with `<version>`.
pub fn normalise(text: &str) -> String {
    let text = text
        .replace("\r\n", "\n")
        .replace(env!("CARGO_PKG_VERSION"), "<version>");
    const MARK: &str = "review-buddy-demo-";
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut rest = line;
        while let Some(at) = rest.find(MARK) {
            let start = rest[..at]
                .rfind(|c: char| c.is_whitespace())
                .map_or(0, |p| p + 1);
            let after = at + MARK.len();
            let dir_end = rest[after..]
                .find(['/', '\\'])
                .map_or(rest.len(), |p| after + p);
            let token_end = rest[dir_end..]
                .find(|c: char| c.is_whitespace())
                .map_or(rest.len(), |p| dir_end + p);
            out.push_str(&rest[..start]);
            out.push_str("<demo>");
            out.push_str(&rest[dir_end..token_end].replace('\\', "/"));
            rest = &rest[token_end..];
        }
        out.push_str(rest);
    }
    out
}

/// Runs the binary in a pseudo-terminal, so stdout is a TTY, and returns the raw output with
/// its escape sequences and the exit code.
#[cfg(unix)]
pub fn run_in_pty(sandbox: &Sandbox, args: &[&str], colour: bool) -> (String, u32) {
    use std::io::Read;

    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(["--demo", "--frozen-time", FROZEN]);
    cmd.args(args);
    cmd.env_clear();
    for key in PASSTHROUGH {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    let root = sandbox.path();
    cmd.env("HOME", root);
    cmd.env("XDG_CONFIG_HOME", root.join("config"));
    cmd.env("XDG_DATA_HOME", root.join("data"));
    cmd.env("XDG_CACHE_HOME", root.join("cache"));
    cmd.env("XDG_STATE_HOME", root.join("state"));
    cmd.env("TERM", "xterm-256color");
    cmd.env("REVIEW_BUDDY_PAGER", "cat");
    if !colour {
        cmd.env("NO_COLOR", "1");
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut out = String::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    let code = child.wait().unwrap().exit_code();
    (out.replace('\r', ""), code)
}

/// Removes CSI escape sequences.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
