//! Drives Settings → Sources in a pseudo-terminal: open with `,`, add a source through the form
//! against a stub GitHub, then remove it through the confirm, checking the config on disk and
//! that no token is written anywhere. Tokens come from an environment variable, so the real
//! keyring is never touched.
#![cfg(all(unix, feature = "live"))]

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "ghp_pty_secret_value";

/// The screen as the terminal would show it. Only changed cells are written to a terminal, so
/// matching on the raw output would miss any text that shares a cell with what was there before.
struct Grid {
    cells: Vec<Vec<char>>,
    row: usize,
    col: usize,
    pending: Vec<u8>,
}

const ROWS: usize = 40;
const COLS: usize = 160;

impl Grid {
    fn new() -> Self {
        Self {
            cells: vec![vec![' '; COLS]; ROWS],
            row: 0,
            col: 0,
            pending: Vec::new(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(e) => e.valid_up_to(),
        };
        let rest = self.pending.split_off(valid);
        let mut text = String::from_utf8(std::mem::replace(&mut self.pending, rest)).unwrap();
        if let Some(at) = text.rfind('\x1b') {
            let tail = &text[at + 1..];
            let done = match tail.chars().next() {
                None => false,
                Some('[') => tail[1..]
                    .chars()
                    .any(|c| ('\u{40}'..='\u{7e}').contains(&c)),
                Some(']') => tail.contains('\x07') || tail.contains("\x1b\\"),
                Some(_) => true,
            };
            if !done {
                let held = text.split_off(at);
                self.pending.splice(0..0, held.into_bytes());
            }
        }
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => match chars.next() {
                    Some('[') => {
                        let mut params = String::new();
                        while let Some(&n) = chars.peek() {
                            chars.next();
                            if ('\u{40}'..='\u{7e}').contains(&n) {
                                self.csi(n, &params);
                                break;
                            }
                            params.push(n);
                        }
                    }
                    Some(']') => {
                        while let Some(n) = chars.next() {
                            if n == '\x07' || (n == '\x1b' && chars.next() == Some('\\')) {
                                break;
                            }
                        }
                    }
                    _ => {}
                },
                '\r' => self.col = 0,
                '\n' => self.row = (self.row + 1).min(ROWS - 1),
                c if c.is_control() => {}
                c => self.put(c),
            }
        }
    }

    fn csi(&mut self, last: char, params: &str) {
        let nums: Vec<usize> = params
            .split(';')
            .map(|p| p.trim_start_matches('?').parse().unwrap_or(0))
            .collect();
        match last {
            'H' | 'f' => {
                self.row = nums.first().copied().unwrap_or(1).clamp(1, ROWS) - 1;
                self.col = nums.get(1).copied().unwrap_or(1).clamp(1, COLS) - 1;
            }
            'J' if nums.first() == Some(&2) => {
                self.cells = vec![vec![' '; COLS]; ROWS];
            }
            'K' => {
                for c in self.col..COLS {
                    self.cells[self.row][c] = ' ';
                }
            }
            _ => {}
        }
    }

    fn put(&mut self, c: char) {
        let width = unicode_width::UnicodeWidthChar::width(c)
            .unwrap_or(1)
            .max(1);
        if self.col < COLS {
            self.cells[self.row][self.col] = c;
            for extra in 1..width {
                if self.col + extra < COLS {
                    self.cells[self.row][self.col + extra] = '\0';
                }
            }
        }
        self.col += width;
    }

    fn text(&self) -> String {
        self.cells
            .iter()
            .map(|row| row.iter().filter(|c| **c != '\0').collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

struct Session {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    grid: Grid,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Session {
    fn sees(&mut self, needle: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            while let Ok(chunk) = self.rx.try_recv() {
                self.grid.feed(&chunk);
            }
            if self.grid.text().contains(needle) {
                return true;
            }
            if let Ok(chunk) = self.rx.recv_timeout(Duration::from_millis(100)) {
                self.grid.feed(&chunk);
            }
        }
        self.grid.text().contains(needle)
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    fn tail(&self) -> String {
        self.grid.text()
    }
}

fn stub_bin(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let script = |name: &str, body: &str| {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    script(
        "gh",
        "case \"$1 $2\" in\n\"auth status\") echo ghe.test; echo '  ✓ Logged in to ghe.test account octo (keyring)';;\n\"auth token\") echo gho_fake;;\n*) exit 1;;\nesac",
    );
    script("glab", "exit 1");
}

fn spawn(home: &Path, bin: &Path, args: &[&str]) -> Session {
    spawn_with(home, bin, args, &[])
}

fn spawn_with(home: &Path, bin: &Path, args: &[&str], extra: &[(&str, &Path)]) -> Session {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(args);
    cmd.env_clear();
    cmd.env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("HOME", home);
    cmd.env("STUB_TOKEN", TOKEN);
    for var in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        cmd.env(var, home.join(var));
    }
    for (key, value) in extra {
        cmd.env(key, value);
    }
    let child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    Session {
        child,
        writer,
        rx,
        grid: Grid::new(),
        _master: pair.master,
    }
}

async fn stub_github() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-oauth-scopes", "repo, read:org")
                .set_body_json(json!({"login": "octo"})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/rate_limit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": {"core": {"limit": 5000, "remaining": 1, "reset": 1}}
        })))
        .mount(&server)
        .await;
    Mock::given(path("/user/orgs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"login": "liminal-hq"}])))
        .mount(&server)
        .await;
    server
}

fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn wait_exit(s: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "did not exit");
        let _ = s.rx.recv_timeout(Duration::from_millis(50));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn adding_and_removing_a_source_writes_the_config_and_never_a_token() {
    let server = stub_github().await;
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    stub_bin(&bin);
    let dir = home.path().join("XDG_CONFIG_HOME/review-buddy");
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.toml");
    let before = format!(
        "# mine\n[[source]]\nname = \"ghe\"   # first\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{}\"\nauth = \"env:STUB_TOKEN\"\n",
        server.uri()
    );
    std::fs::write(&config, &before).unwrap();

    let mut s = spawn(home.path(), &bin, &[]);
    assert!(s.sees("review buddy"), "{}", s.tail());
    std::thread::sleep(Duration::from_millis(500));
    s.send(b",");
    assert!(
        s.sees("Where your pull and merge requests come from"),
        "{}",
        s.tail()
    );
    assert!(s.sees("not tested"), "the table has loaded: {}", s.tail());

    s.send(b"t");
    assert!(
        s.sees("signed in as octo"),
        "the token check runs against the stub: {}",
        s.tail()
    );

    s.send(b"a");
    assert!(s.sees("Another host"), "{}", s.tail());
    s.send(b"\r");
    assert!(s.sees("API address"), "{}", s.tail());
    s.send(b"lab.test");
    s.send(b"\x1b[A\x1b[A");
    s.send(b"lab");
    s.send(b"\t\t\t");
    s.send(server.uri().as_bytes());
    s.send(b"\t");
    s.send(b"\x1b[C");
    assert!(s.sees("environment variable"), "{}", s.tail());
    s.send(b"\t");
    s.send(b"STUB_TOKEN");
    s.send(b"\r");
    assert!(s.sees("Added lab."), "{}", s.tail());

    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.starts_with(&before), "{written}");
    assert!(written.contains("name = \"lab\""), "{written}");
    assert!(written.contains("host = \"lab.test\""));
    assert!(written.contains("auth = \"env:STUB_TOKEN\""));
    assert_eq!(
        std::fs::metadata(&config).unwrap().permissions().mode() & 0o777,
        0o600
    );

    s.send(b"x");
    assert!(s.sees("Remove lab (lab.test)?"), "{}", s.tail());
    s.send(b"\r");
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("name = \"lab\""),
        "enter on the confirm keeps the source"
    );

    s.send(b"x");
    assert!(s.sees("Remove lab (lab.test)?"), "{}", s.tail());
    s.send(b"y");
    assert!(s.sees("Removed lab."), "{}", s.tail());
    assert_eq!(std::fs::read_to_string(&config).unwrap(), before);

    s.send(b"\x1b");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"q");
    wait_exit(&mut s);

    let mut files = Vec::new();
    files_under(home.path(), &mut files);
    for file in files {
        let bytes = std::fs::read(&file).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(TOKEN),
            "a token was written to {}",
            file.display()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sources_from_a_system_config_are_labelled_and_left_alone() {
    let server = stub_github().await;
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    stub_bin(&bin);
    let system = home.path().join("etc/xdg/review-buddy");
    std::fs::create_dir_all(&system).unwrap();
    let body = format!(
        "[[source]]\nname = \"ghe\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{}\"\nauth = \"env:STUB_TOKEN\"\n",
        server.uri()
    );
    std::fs::write(system.join("config.toml"), &body).unwrap();
    let user = home.path().join("XDG_CONFIG_HOME/review-buddy");
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(user.join("config.toml"), "# mine\n").unwrap();

    let mut s = spawn_with(
        home.path(),
        &bin,
        &[],
        &[("XDG_CONFIG_DIRS", &home.path().join("etc/xdg"))],
    );
    assert!(s.sees("review buddy"), "{}", s.tail());
    std::thread::sleep(Duration::from_millis(500));
    s.send(b",");
    assert!(s.sees("Defined"), "{}", s.tail());
    assert!(s.sees("read-only here"), "{}", s.tail());
    s.send(b"x");
    assert!(s.sees("Review Buddy doesn"), "{}", s.tail());
    assert_eq!(
        std::fs::read_to_string(user.join("config.toml")).unwrap(),
        "# mine\n"
    );
    assert_eq!(
        std::fs::read_to_string(system.join("config.toml")).unwrap(),
        body
    );
    s.send(b"\x1b");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"q");
    wait_exit(&mut s);
}
