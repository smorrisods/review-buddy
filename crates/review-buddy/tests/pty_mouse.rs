//! Mouse in the real binary, in a pseudo-terminal, under `--demo`: SGR mouse sequences for a
//! tab click, a double-click, a wheel turn and a drag over diff lines.
#![cfg(all(unix, feature = "demo"))]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_theme::ColourDepth;
use review_buddy::app::{update, App, AppConfig, Msg};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui;

const LIMIT: Duration = Duration::from_secs(10);
const SIZE: (u16, u16) = (160, 40);

struct Session {
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
}

impl Session {
    fn shows(&self, needle: &str) -> bool {
        String::from_utf8_lossy(&self.seen).contains(needle)
    }

    fn expect(&mut self, needle: &str, why: &str) {
        let deadline = Instant::now() + LIMIT;
        while Instant::now() < deadline {
            if self.shows(needle) {
                return;
            }
            if let Ok(chunk) = self.rx.recv_timeout(Duration::from_millis(100)) {
                self.seen.extend(chunk);
            }
        }
        let all = String::from_utf8_lossy(&self.seen);
        let tail: String = all
            .chars()
            .rev()
            .take(600)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        panic!("{why}: …{tail:?}");
    }

    fn send(&mut self, bytes: &[u8], needle: &str, why: &str) {
        // Input written straight after a repaint can be lost by the pty, so let it settle.
        std::thread::sleep(Duration::from_millis(300));
        self.seen.clear();
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
        self.expect(needle, why);
    }
}

fn sgr(button: u8, (x, y): (u16, u16), press: bool) -> Vec<u8> {
    format!(
        "\x1b[<{button};{};{}{}",
        x + 1,
        y + 1,
        if press { 'M' } else { 'm' }
    )
    .into_bytes()
}

fn app() -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let snapshot = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(world.snapshot())
        .unwrap();
    let mut app = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: SIZE,
    });
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    app
}

fn locate(app: &mut App, needle: &str) -> (u16, u16) {
    let mut terminal = Terminal::new(TestBackend::new(SIZE.0, SIZE.1)).unwrap();
    terminal
        .draw(|frame| app.hits = ui::draw(frame, app))
        .unwrap();
    let buffer: &Buffer = terminal.backend().buffer();
    (0..SIZE.1)
        .find_map(|y| {
            let line: String = (0..SIZE.0).map(|x| buffer[(x, y)].symbol()).collect();
            line.find(needle)
                .map(|byte| (line[..byte].chars().count() as u16, y))
        })
        .unwrap_or_else(|| panic!("{needle:?} on screen"))
}

#[test]
fn tabs_double_click_wheel_and_drag_work_through_sgr_mouse_reports() {
    let home = tempfile::tempdir().unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: SIZE.1,
            cols: SIZE.0,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_review-buddy"));
    cmd.args(["--demo", "--frozen-time", DEFAULT_FROZEN]);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("NO_COLOR");
    cmd.env("HOME", home.path());
    for var in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        cmd.env(var, home.path().join(var));
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
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
    let mut s = Session {
        writer,
        rx,
        seen: Vec::new(),
    };
    s.expect(
        "Add a menu bar and keyboard-driven menus",
        "the queue loads",
    );
    assert!(
        s.shows("\x1b[?1000h") && s.shows("\x1b[?1006h"),
        "mouse capture is switched on"
    );

    let mut probe = app();
    let (x, y) = locate(&mut probe, "Checks");
    let mut click = sgr(0, (x + 1, y), true);
    click.extend(sgr(0, (x + 1, y), false));
    s.send(
        &click,
        "passing · 1m 34s · required",
        "clicking Checks shows the checks",
    );

    let (x, y) = locate(&mut probe, "Waiting on you");
    let row = (x + 2, y + 2);
    let mut double = Vec::new();
    for _ in 0..2 {
        double.extend(sgr(0, row, true));
        double.extend(sgr(0, row, false));
    }
    s.send(
        &double,
        "@@ -10,7 +10,9 @@",
        "a double-click opens the diff",
    );

    let mut wheel = Vec::new();
    for _ in 0..6 {
        wheel.extend(sgr(65, (100, 20), true));
    }
    s.send(&wheel, "Option<", "the wheel scrolls the diff under it");

    let mut drag = sgr(0, (60, 3), true);
    drag.extend(sgr(32, (60, 6), true));
    drag.extend(sgr(0, (60, 6), false));
    s.send(&drag, "▌", "dragging marks the selected lines");

    s.send(
        b"\x1b",
        "       42 ",
        "esc clears the range first, still on the diff",
    );
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"\x1b", "Waiting on you", "a second esc leaves the diff");
    s.send(
        b"q",
        "aren't saved",
        "q warns that demo drafts aren't saved",
    );
    s.send(b"q", "\x1b[?1000l", "quitting releases the mouse");
    let deadline = Instant::now() + LIMIT;
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "q exits");
        std::thread::sleep(Duration::from_millis(50));
    }
}
