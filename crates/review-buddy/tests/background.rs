//! The painted background: every theme at both sizes in each mode, across the screens and
//! overlays, at each colour depth, and under `NO_COLOR`. Cells are checked on the rendered buffer,
//! so a widget that clears to the terminal's own colour shows up as a hole.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use rb_core::ForgeKind;
use rb_theme::{nearest_ansi, quantise_256, BackgroundMode, Colour, ColourDepth, Rgb, BUILTIN_IDS};
use review_buddy::app::{setup, update, App, AppConfig, Msg, Screen};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::settings::{self, SourceRow};
use review_buddy::setup::{
    self as first_run, AuthKind, Detection, Evidence, Flow, FoundHost, ProbeInfo, SourceSpec,
};
use review_buddy::ui::style;

#[path = "support/render.rs"]
mod render_support;
use render_support::{
    assert_fully_painted, assert_unpainted, bg_histogram, render, surface_backgrounds, text,
};

const SIZES: [(u16, u16); 2] = [(160, 40), (100, 30)];
const MODES: [BackgroundMode; 3] = [
    BackgroundMode::Theme,
    BackgroundMode::Yes,
    BackgroundMode::No,
];

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

fn world() -> DemoWorld {
    DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap()
}

fn press(app: &mut App, code: KeyCode) {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn key(app: &mut App, c: char) {
    press(app, KeyCode::Char(c));
}

struct Setup {
    theme: &'static str,
    size: (u16, u16),
    mode: BackgroundMode,
    depth: ColourDepth,
    no_color: bool,
}

impl Setup {
    fn app(&self) -> App {
        let mut app = App::new(AppConfig {
            theme_id: self.theme.into(),
            depth: self.depth,
            no_color: self.no_color,
            size: self.size,
        });
        app.background.global = self.mode;
        app.rebuild_palette();
        app
    }

    /// What the rules say should be painted, written out by hand per theme.
    fn expected(&self) -> Option<Color> {
        if self.no_color {
            return None;
        }
        let opaque = match self.theme {
            "afterglow-dark" => Some(Rgb::new(0x0f, 0x0e, 0x1a)),
            "afterglow-light" => Some(Rgb::new(0xfb, 0xfa, 0xf6)),
            _ => None,
        };
        let fallback = Rgb::new(0x05, 0x05, 0x07);
        let rgb = match self.mode {
            BackgroundMode::No => return None,
            BackgroundMode::Theme if self.depth == ColourDepth::Ansi16 => return None,
            BackgroundMode::Theme => opaque?,
            BackgroundMode::Yes => opaque.unwrap_or(fallback),
        };
        Some(style::colour(match self.depth {
            ColourDepth::TrueColour => Colour::Rgb(rgb),
            ColourDepth::Ansi256 => Colour::Indexed(quantise_256(rgb)),
            ColourDepth::Ansi16 => Colour::Ansi(nearest_ansi(Colour::Rgb(rgb))),
        }))
    }

    fn check(&self, app: &mut App, scene: &str) -> ratatui::buffer::Buffer {
        let buffer = render(app);
        let what = format!(
            "{scene} {} {:?} {:?} {:?}",
            self.theme, self.size, self.mode, self.depth
        );
        let surfaces = surface_backgrounds(app);
        match self.expected() {
            Some(expected) => assert_fully_painted(&buffer, expected, &surfaces, &what),
            None => assert_unpainted(&buffer, &surfaces, &what),
        }
        buffer
    }
}

fn load_dashboard(app: &mut App) {
    let snapshot = block_on(world().snapshot()).unwrap();
    update(app, Msg::Loaded(Box::new(snapshot)));
}

fn open_diff(app: &mut App) {
    load_dashboard(app);
    let id = app.selected_change().unwrap().id.clone();
    let data = block_on(world().diff_data(&id)).unwrap();
    press(app, KeyCode::Enter);
    update(
        app,
        Msg::DiffLoaded {
            id,
            result: Ok(Box::new(data)),
        },
    );
    assert_eq!(app.screen, Screen::Diff);
}

fn dashboard(app: &mut App) {
    load_dashboard(app);
}

fn diff(app: &mut App) {
    open_diff(app);
}

fn composer(app: &mut App) {
    open_diff(app);
    key(app, 'c');
}

fn help(app: &mut App) {
    load_dashboard(app);
    key(app, '?');
    assert!(app.help);
}

fn show(app: &mut App) {
    load_dashboard(app);
    key(app, 's');
    assert!(app.show.open);
}

fn first_run_look(app: &mut App) {
    let found = |host: &str, kind, user: Option<&str>| FoundHost {
        host: host.into(),
        kind,
        evidence: vec![match user {
            Some(user) => Evidence::Cli {
                tool: rb_platform::auth::CliTool::Gh,
                user: user.into(),
            },
            None => Evidence::GitConfig,
        }],
    };
    let theme = app.palette.theme().id.clone();
    setup::start(
        app,
        Flow::new(
            "/home/u/.config/review-buddy/config.toml".into(),
            false,
            &theme,
            true,
        ),
    );
    update(
        app,
        Msg::Setup(first_run::Input::Detected(Detection {
            hosts: vec![
                found("github.com", ForgeKind::GitHub, Some("smorris")),
                found("gitlab.work.ca", ForgeKind::GitLab, None),
            ],
            notes: Vec::new(),
        })),
    );
    update(
        app,
        Msg::Setup(first_run::Input::Probed {
            host: "github.com".into(),
            result: Ok(ProbeInfo {
                login: "smorris".into(),
                orgs: vec!["liminal-hq".into()],
                checked: true,
                ..ProbeInfo::default()
            }),
        }),
    );
    press(app, KeyCode::Enter);
    press(app, KeyCode::Down);
    key(app, ' ');
    for c in "glpat-secret".chars() {
        key(app, c);
    }
    press(app, KeyCode::Esc);
    press(app, KeyCode::Enter);
    key(app, ' ');
    press(app, KeyCode::Enter);
    assert_eq!(app.screen, Screen::FirstRun);
    assert!(text(&render(app)).contains("Pick a look"), "the look step");
}

fn settings_screen(app: &mut App) {
    key(app, ',');
    let row = SourceRow {
        spec: SourceSpec {
            name: "github.com".into(),
            kind: ForgeKind::GitHub,
            host: "github.com".into(),
            api_url: None,
            auth: AuthKind::Cli,
            scope_user: true,
            owners: vec!["liminal-hq".into()],
        },
        enabled: true,
        in_all: true,
        include_drafts: false,
        tag_colour: Some("cyan".into()),
        repos: Vec::new(),
    };
    update(
        app,
        Msg::Settings(settings::Input::Loaded(Ok(settings::Snapshot {
            rows: vec![row],
            write_target: "/home/u/.config/review-buddy/config.toml".into(),
            editable: true,
            origin: None,
        }))),
    );
    assert_eq!(app.screen, Screen::Settings);
}

type Scene = fn(&mut App);

const SCENES: [(&str, Scene); 7] = [
    ("dashboard", dashboard),
    ("diff", diff),
    ("composer", composer),
    ("help", help),
    ("show", show),
    ("first-run-look", first_run_look),
    ("settings", settings_screen),
];

#[test]
fn every_scene_is_painted_or_left_alone_without_holes() {
    for theme in BUILTIN_IDS {
        for size in SIZES {
            for mode in MODES {
                for (name, scene) in SCENES {
                    let setup = Setup {
                        theme,
                        size,
                        mode,
                        depth: ColourDepth::TrueColour,
                        no_color: false,
                    };
                    let mut app = setup.app();
                    scene(&mut app);
                    setup.check(&mut app, name);
                }
            }
        }
    }
}

fn pinned(theme: &'static str, size: (u16, u16), mode: BackgroundMode) -> String {
    let setup = Setup {
        theme,
        size,
        mode,
        depth: ColourDepth::TrueColour,
        no_color: false,
    };
    let mut app = setup.app();
    dashboard(&mut app);
    bg_histogram(&setup.check(&mut app, "dashboard"))
}

macro_rules! histograms {
    ($($name:ident: $theme:literal, $w:literal x $h:literal, $mode:ident;)*) => {
        $(
            #[test]
            fn $name() {
                insta::assert_snapshot!(pinned($theme, ($w, $h), BackgroundMode::$mode));
            }
        )*
    };
}

histograms! {
    background_dashboard_liminal_hq_160x40_theme: "liminal-hq", 160 x 40, Theme;
    background_dashboard_liminal_hq_160x40_yes: "liminal-hq", 160 x 40, Yes;
    background_dashboard_liminal_hq_160x40_no: "liminal-hq", 160 x 40, No;
    background_dashboard_liminal_hq_100x30_theme: "liminal-hq", 100 x 30, Theme;
    background_dashboard_liminal_hq_100x30_yes: "liminal-hq", 100 x 30, Yes;
    background_dashboard_liminal_hq_100x30_no: "liminal-hq", 100 x 30, No;
    background_dashboard_dusk_160x40_theme: "dusk", 160 x 40, Theme;
    background_dashboard_dusk_160x40_yes: "dusk", 160 x 40, Yes;
    background_dashboard_dusk_160x40_no: "dusk", 160 x 40, No;
    background_dashboard_dusk_100x30_theme: "dusk", 100 x 30, Theme;
    background_dashboard_dusk_100x30_yes: "dusk", 100 x 30, Yes;
    background_dashboard_dusk_100x30_no: "dusk", 100 x 30, No;
    background_dashboard_afterglow_dark_160x40_theme: "afterglow-dark", 160 x 40, Theme;
    background_dashboard_afterglow_dark_160x40_yes: "afterglow-dark", 160 x 40, Yes;
    background_dashboard_afterglow_dark_160x40_no: "afterglow-dark", 160 x 40, No;
    background_dashboard_afterglow_dark_100x30_theme: "afterglow-dark", 100 x 30, Theme;
    background_dashboard_afterglow_dark_100x30_yes: "afterglow-dark", 100 x 30, Yes;
    background_dashboard_afterglow_dark_100x30_no: "afterglow-dark", 100 x 30, No;
    background_dashboard_afterglow_light_160x40_theme: "afterglow-light", 160 x 40, Theme;
    background_dashboard_afterglow_light_160x40_yes: "afterglow-light", 160 x 40, Yes;
    background_dashboard_afterglow_light_160x40_no: "afterglow-light", 160 x 40, No;
    background_dashboard_afterglow_light_100x30_theme: "afterglow-light", 100 x 30, Theme;
    background_dashboard_afterglow_light_100x30_yes: "afterglow-light", 100 x 30, Yes;
    background_dashboard_afterglow_light_100x30_no: "afterglow-light", 100 x 30, No;
}

#[test]
fn the_dashboard_text_is_the_same_painted_or_not() {
    for theme in BUILTIN_IDS {
        let render_text = |mode| {
            let setup = Setup {
                theme,
                size: (160, 40),
                mode,
                depth: ColourDepth::TrueColour,
                no_color: false,
            };
            let mut app = setup.app();
            dashboard(&mut app);
            text(&render(&mut app))
        };
        assert_eq!(
            render_text(BackgroundMode::Yes),
            render_text(BackgroundMode::No)
        );
    }
}

#[test]
fn colour_depths_quantise_the_background() {
    for theme in BUILTIN_IDS {
        for depth in [ColourDepth::Ansi256, ColourDepth::Ansi16] {
            for mode in MODES {
                for (name, scene) in SCENES {
                    let setup = Setup {
                        theme,
                        size: (160, 40),
                        mode,
                        depth,
                        no_color: false,
                    };
                    let mut app = setup.app();
                    scene(&mut app);
                    setup.check(&mut app, name);
                }
            }
        }
    }
}

#[test]
fn sixteen_colours_only_paint_when_forced() {
    for theme in BUILTIN_IDS {
        let setup = |mode| Setup {
            theme,
            size: (160, 40),
            mode,
            depth: ColourDepth::Ansi16,
            no_color: false,
        };
        assert_eq!(setup(BackgroundMode::Theme).expected(), None);
        assert!(setup(BackgroundMode::Yes).expected().is_some());
    }
}

#[test]
fn no_color_never_paints_in_any_mode() {
    for theme in BUILTIN_IDS {
        for mode in MODES {
            for (name, scene) in SCENES {
                let setup = Setup {
                    theme,
                    size: (160, 40),
                    mode,
                    depth: ColourDepth::TrueColour,
                    no_color: true,
                };
                let mut app = setup.app();
                scene(&mut app);
                let buffer = render(&mut app);
                assert!(
                    buffer.content().iter().all(|c| c.bg == Color::Reset),
                    "{name} {theme} {mode:?}: NO_COLOR paints no background"
                );
            }
        }
    }
}

#[test]
fn the_session_key_cycles_theme_yes_no_with_a_toast() {
    let setup = Setup {
        theme: "afterglow-dark",
        size: (160, 40),
        mode: BackgroundMode::Theme,
        depth: ColourDepth::TrueColour,
        no_color: false,
    };
    let mut app = setup.app();
    dashboard(&mut app);
    assert!(app.palette.background().is_some());
    key(&mut app, 'B');
    assert_eq!(app.background_mode(), BackgroundMode::Yes);
    let toast = app.status.as_ref().or(app.toasts.last()).unwrap();
    assert!(toast.notice.text.contains("painted"));
    key(&mut app, 'B');
    assert_eq!(app.background_mode(), BackgroundMode::No);
    assert!(app.palette.background().is_none());
    assert_unpainted(&render(&mut app), &surface_backgrounds(&app), "after B B");
    key(&mut app, 'B');
    assert_eq!(app.background_mode(), BackgroundMode::Theme);
    assert!(app.palette.background().is_some());
}

#[test]
fn the_session_key_beats_the_environment_the_theme_setting_and_the_global_one() {
    let mut app = Setup {
        theme: "dusk",
        size: (160, 40),
        mode: BackgroundMode::No,
        depth: ColourDepth::TrueColour,
        no_color: false,
    }
    .app();
    app.background
        .per_theme
        .insert("dusk".into(), BackgroundMode::Yes);
    app.rebuild_palette();
    assert!(app.palette.background().is_some(), "per-theme beats global");
    app.background.env = Some(BackgroundMode::No);
    app.rebuild_palette();
    assert!(app.palette.background().is_none(), "env beats per-theme");
    key(&mut app, 'B');
    assert_eq!(app.background_mode(), BackgroundMode::Theme);
    assert!(
        app.palette.background().is_none(),
        "dusk's theme default is transparent"
    );
    key(&mut app, 'B');
    assert_eq!(app.background_mode(), BackgroundMode::Yes);
    assert!(app.palette.background().is_some(), "session beats env");
}

#[test]
fn switching_theme_in_first_run_changes_the_painted_background() {
    let paint = |theme: &'static str| {
        let setup = Setup {
            theme,
            size: (160, 40),
            mode: BackgroundMode::Theme,
            depth: ColourDepth::TrueColour,
            no_color: false,
        };
        let mut app = setup.app();
        first_run_look(&mut app);
        setup.check(&mut app, "first-run-look");
        app.palette.background()
    };
    assert_eq!(paint("liminal-hq"), None);
    assert_ne!(paint("afterglow-dark"), paint("afterglow-light"));
    assert!(paint("afterglow-light").is_some());
}
