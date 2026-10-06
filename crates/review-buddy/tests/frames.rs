//! The canonical demo frames from the spec (1a three panes, 1d diff, 1f diff with the composer
//! and a pending suggestion) rendered headlessly in the built-in themes at the two reference
//! sizes, pinned with `insta`. See `docs/testing.md` for how to review and accept changes.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::TestBackend,
    buffer::Buffer,
    style::{Color, Modifier},
    Terminal,
};
use rb_theme::{ColourDepth, Role};
use review_buddy::app::{update, App, AppConfig, Cmd, Msg, Screen};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::ui::{self, chrome::WORDMARK, style, HitMap};

const THEMES: [&str; 3] = ["liminal-hq", "dusk", "afterglow-dark"];
const SIZES: [(u16, u16); 2] = [(160, 40), (100, 30)];

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

fn press(app: &mut App, code: KeyCode) -> Vec<Cmd> {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

/// The dashboard (frame 1a): demo data at the frozen time, truecolour forced.
fn dashboard(theme: &str, (w, h): (u16, u16), no_color: bool) -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color,
        size: (w, h),
    });
    let snapshot = block_on(world.snapshot()).unwrap();
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    app
}

/// The diff of the first demo change (frame 1d) with the cursor on a changed line.
fn diff(theme: &str, size: (u16, u16), no_color: bool, line: u32) -> App {
    let mut app = dashboard(theme, size, no_color);
    let id = app.selected_change().unwrap().id.clone();
    assert_eq!(id.number, 214);
    let cmds = press(&mut app, KeyCode::Enter);
    assert!(matches!(cmds.as_slice(), [Cmd::LoadDiff(_)]));
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let data = block_on(world.diff_data(&id)).unwrap();
    update(
        &mut app,
        Msg::DiffLoaded {
            id,
            result: Ok(Box::new(data)),
        },
    );
    assert_eq!(app.screen, Screen::Diff);
    press(&mut app, KeyCode::Char('n'));
    for _ in 0..500 {
        if cursor_new_no(&app) == Some(line) {
            return app;
        }
        press(&mut app, KeyCode::Char('j'));
    }
    panic!("no line {line} in the demo diff");
}

fn thread_frame(theme: &str, size: (u16, u16), no_color: bool) -> App {
    diff(theme, size, no_color, 44)
}

/// Frame 1f: the composer docked under a line that already has a pending suggestion.
fn composer_frame(theme: &str, size: (u16, u16), no_color: bool) -> App {
    let mut app = diff(theme, size, no_color, 51);
    press(&mut app, KeyCode::Char('c'));
    type_text(&mut app, "Worth a guard here?");
    app
}

fn cursor_new_no(app: &App) -> Option<u32> {
    let state = app.diff.as_ref()?;
    let id = state.view.rows.line_id(state.view.cursor)?;
    state.current()?.diff.parsed()?.line(id)?.new_no
}

fn render(app: &mut App) -> Buffer {
    let (w, h) = app.size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|frame| hits = ui::draw(frame, app)).unwrap();
    app.hits = hits;
    terminal.backend().buffer().clone()
}

fn rows(buffer: &Buffer) -> Vec<String> {
    let width = usize::from(buffer.area.width);
    buffer
        .content()
        .chunks(width)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect()
}

fn text(buffer: &Buffer) -> String {
    let rows: Vec<String> = rows(buffer)
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect();
    rows.join("\n")
}

/// Every run of cells that share a bold/dim/reverse set, as `row:col B|D|R "text"`, in reading
/// order, so a `NO_COLOR` snapshot shows exactly where the modifiers carry the meaning.
fn modifier_dump(buffer: &Buffer) -> String {
    let width = usize::from(buffer.area.width);
    let mut out = Vec::new();
    for (y, row) in buffer.content().chunks(width).enumerate() {
        let mut x = 0;
        while x < row.len() {
            let tag = modifier_tag(&row[x]);
            let start = x;
            while x < row.len() && modifier_tag(&row[x]) == tag {
                x += 1;
            }
            if !tag.is_empty() {
                let run: String = row[start..x].iter().map(|c| c.symbol()).collect();
                out.push(format!("{y}:{start} {tag} {:?}", run.trim_end()));
            }
        }
    }
    out.join("\n")
}

fn modifier_tag(cell: &ratatui::buffer::Cell) -> String {
    [
        (Modifier::BOLD, "B"),
        (Modifier::DIM, "D"),
        (Modifier::REVERSED, "R"),
    ]
    .iter()
    .filter(|(m, _)| cell.modifier.contains(*m))
    .map(|(_, t)| *t)
    .collect()
}

fn plain_dump(app: &mut App) -> String {
    let buffer = render(app);
    assert!(
        buffer
            .content()
            .iter()
            .all(|c| c.fg == Color::Reset && c.bg == Color::Reset),
        "NO_COLOR paints no colour"
    );
    format!(
        "{}\n\n-- modifiers --\n{}",
        text(&buffer),
        modifier_dump(&buffer)
    )
}

fn find(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    rows(buffer).iter().enumerate().find_map(|(y, row)| {
        row.find(needle)
            .map(|byte| (row[..byte].chars().count() as u16, y as u16))
    })
}

fn role(app: &App, role: Role) -> Option<Color> {
    app.palette.colour(role).map(style::colour)
}

macro_rules! frames {
    ($($name:ident: $scene:ident($theme:literal, $w:literal, $h:literal);)*) => {
        $(
            #[test]
            fn $name() {
                let mut app = $scene($theme, ($w, $h), false);
                insta::assert_snapshot!(text(&render(&mut app)));
            }
        )*
    };
}

macro_rules! plain_frames {
    ($($name:ident: $scene:ident($w:literal, $h:literal);)*) => {
        $(
            #[test]
            fn $name() {
                let mut app = $scene("liminal-hq", ($w, $h), true);
                insta::assert_snapshot!(plain_dump(&mut app));
            }
        )*
    };
}

frames! {
    frame_1a_three_panes_liminal_hq_160x40: dashboard("liminal-hq", 160, 40);
    frame_1a_three_panes_dusk_160x40: dashboard("dusk", 160, 40);
    frame_1a_three_panes_afterglow_dark_160x40: dashboard("afterglow-dark", 160, 40);
    frame_1a_three_panes_liminal_hq_100x30: dashboard("liminal-hq", 100, 30);
    frame_1a_three_panes_dusk_100x30: dashboard("dusk", 100, 30);
    frame_1a_three_panes_afterglow_dark_100x30: dashboard("afterglow-dark", 100, 30);
    frame_1d_diff_liminal_hq_160x40: thread_frame("liminal-hq", 160, 40);
    frame_1d_diff_dusk_160x40: thread_frame("dusk", 160, 40);
    frame_1d_diff_afterglow_dark_160x40: thread_frame("afterglow-dark", 160, 40);
    frame_1d_diff_liminal_hq_100x30: thread_frame("liminal-hq", 100, 30);
    frame_1d_diff_dusk_100x30: thread_frame("dusk", 100, 30);
    frame_1d_diff_afterglow_dark_100x30: thread_frame("afterglow-dark", 100, 30);
    frame_1f_composer_liminal_hq_160x40: composer_frame("liminal-hq", 160, 40);
    frame_1f_composer_dusk_160x40: composer_frame("dusk", 160, 40);
    frame_1f_composer_afterglow_dark_160x40: composer_frame("afterglow-dark", 160, 40);
    frame_1f_composer_liminal_hq_100x30: composer_frame("liminal-hq", 100, 30);
    frame_1f_composer_dusk_100x30: composer_frame("dusk", 100, 30);
    frame_1f_composer_afterglow_dark_100x30: composer_frame("afterglow-dark", 100, 30);
}

plain_frames! {
    frame_1a_three_panes_no_color_160x40: dashboard(160, 40);
    frame_1a_three_panes_no_color_100x30: dashboard(100, 30);
    frame_1d_diff_no_color_160x40: thread_frame(160, 40);
    frame_1d_diff_no_color_100x30: thread_frame(100, 30);
    frame_1f_composer_no_color_160x40: composer_frame(160, 40);
    frame_1f_composer_no_color_100x30: composer_frame(100, 30);
}

#[test]
fn frames_contain_what_the_spec_shows() {
    for theme in THEMES {
        for size in SIZES {
            let a = text(&render(&mut dashboard(theme, size, false)));
            assert!(a.contains("Waiting on you"), "{theme} {size:?}\n{a}");
            let d = text(&render(&mut thread_frame(theme, size, false)));
            assert!(d.contains("thread · line 44"), "{theme} {size:?}\n{d}");
            let f = text(&render(&mut composer_frame(theme, size, false)));
            assert!(f.contains("Worth a guard here?"), "{theme} {size:?}\n{f}");
            assert!(
                f.contains("pending suggestion · line 51"),
                "{theme} {size:?}\n{f}"
            );
        }
    }
}

#[test]
fn selected_row_rule_uses_the_accent_in_every_theme() {
    for theme in THEMES {
        let mut app = dashboard(theme, (160, 40), false);
        let buffer = render(&mut app);
        let (x, y) = find(&buffer, "◐ Add a menu bar").unwrap();
        let rule = &buffer[(x - 2, y)];
        assert_eq!(rule.symbol(), "▌");
        assert_eq!(Some(rule.fg), role(&app, Role::Accent), "{theme}");
    }
}

#[test]
fn the_wordmark_runs_the_theme_gradient() {
    for theme in THEMES {
        let mut app = dashboard(theme, (160, 40), false);
        let buffer = render(&mut app);
        let (x, y) = find(&buffer, WORDMARK).unwrap();
        let stops = app.palette.wordmark();
        let first = style::colour(*stops.first().unwrap());
        let last = style::colour(*stops.last().unwrap());
        let end = x + WORDMARK.chars().count() as u16 - 1;
        assert_eq!(buffer[(x, y)].fg, first, "{theme}");
        assert_eq!(buffer[(end, y)].fg, last, "{theme}");
        assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
    }
}

#[test]
fn diff_tints_use_added_bg_and_removed_bg() {
    for theme in THEMES {
        let mut app = thread_frame(theme, (160, 40), false);
        let buffer = render(&mut app);
        let added = style::bg(&app.palette, Role::AddedBg).bg.unwrap();
        let removed = style::bg(&app.palette, Role::RemovedBg).bg.unwrap();
        assert_ne!(added, removed, "{theme}");
        let has = |c: Color| buffer.content().iter().any(|cell| cell.bg == c);
        assert!(has(added), "{theme}: an added line carries added_bg");
        assert!(has(removed), "{theme}: a removed line carries removed_bg");
    }
}

#[test]
fn the_composer_border_uses_the_accent_and_shows_one_caret() {
    for theme in THEMES {
        let mut app = composer_frame(theme, (160, 40), false);
        let buffer = render(&mut app);
        let (x, y) = find(&buffer, "Comment · menus").unwrap();
        let corner = &buffer[(x - 2, y)];
        assert_eq!(corner.symbol(), "╭");
        assert_eq!(Some(corner.fg), role(&app, Role::Accent), "{theme}");
        let carets = buffer
            .content()
            .iter()
            .filter(|c| c.modifier.contains(Modifier::REVERSED))
            .count();
        assert_eq!(carets, 1, "{theme}");
    }
}

#[test]
fn a_transparent_background_stays_unset_in_liminal_hq() {
    let mut app = dashboard("liminal-hq", (160, 40), false);
    assert_eq!(role(&app, Role::Background), None);
    let buffer = render(&mut app);
    assert_eq!(buffer[(159, 39)].bg, Color::Reset);
    assert_eq!(buffer[(80, 0)].bg, Color::Reset);
}

#[test]
fn no_colour_frames_keep_their_signals_as_modifiers() {
    let mut app = diff_plain();
    let dump = plain_dump(&mut app);
    assert!(
        dump.contains(" B "),
        "bold marks the cursor and headings\n{dump}"
    );
}

fn diff_plain() -> App {
    thread_frame("liminal-hq", (160, 40), true)
}
