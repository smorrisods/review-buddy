//! The canonical v0.2 frames: 1i first run (every step), 1j Settings → Sources (table, edit and
//! add forms, remove confirm, a row read-only from another config file) and the mixed
//! GitHub/GitLab dashboard (All view, a GitLab tab, the collapsed strip below 130 columns). They
//! render headlessly in the built-in themes at the two reference sizes, plus `NO_COLOR` variants
//! (text and modifier dump) for liminal-hq, and pin the colours that carry meaning. Shares its
//! helpers with `frames.rs` through `support/render.rs`. See `docs/testing.md`.
#![cfg(feature = "demo")]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use rb_core::{ForgeKind, Source};
use rb_platform::auth::CliTool;
use rb_theme::{ColourDepth, Role};
use review_buddy::app::{setup, update, App, AppConfig, Msg};
use review_buddy::demo::{parse_iso, DemoWorld, DEFAULT_FROZEN};
use review_buddy::settings::{self, Origin, SourceRow, TestInfo};
use review_buddy::setup::{
    self as first_run, AuthKind, Detection, Evidence, Flow, FoundHost, ProbeInfo, SourceSpec,
};
use review_buddy::ui::{chrome::WORDMARK, style};

#[path = "support/render.rs"]
mod render_support;
use render_support::{find, plain_dump, render, role, text};

const THEMES: [&str; 3] = ["liminal-hq", "dusk", "afterglow-dark"];
const SIZES: [(u16, u16); 2] = [(160, 40), (100, 30)];

fn new_app(theme: &str, (w, h): (u16, u16), no_color: bool) -> App {
    App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color,
        size: (w, h),
    })
}

fn press(app: &mut App, code: KeyCode) {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn found(host: &str, kind: ForgeKind, cli: Option<(CliTool, &str)>) -> FoundHost {
    FoundHost {
        host: host.into(),
        kind,
        evidence: vec![match cli {
            Some((tool, user)) => Evidence::Cli {
                tool,
                user: user.into(),
            },
            None => Evidence::GitConfig,
        }],
    }
}

/// Frame 1i. `step` counts the screens: 1 welcome, 2 connect accounts, 3 a masked token entry
/// on the GitLab host, 4 scope, 5 look, 6 Jax, 7 summary.
fn first_run(step: u8, theme: &str, size: (u16, u16), no_color: bool) -> App {
    let mut app = new_app(theme, size, no_color);
    let flow = Flow::new(
        "/home/u/.config/review-buddy/config.toml".into(),
        false,
        theme,
        true,
    );
    setup::start(&mut app, flow);
    update(
        &mut app,
        Msg::Setup(first_run::Input::Detected(Detection {
            hosts: vec![
                found(
                    "github.com",
                    ForgeKind::GitHub,
                    Some((CliTool::Gh, "smorris")),
                ),
                found("gitlab.work.ca", ForgeKind::GitLab, None),
            ],
            notes: Vec::new(),
        })),
    );
    update(
        &mut app,
        Msg::Setup(first_run::Input::Probed {
            host: "github.com".into(),
            result: Ok(ProbeInfo {
                login: "smorris".into(),
                orgs: vec!["liminal-hq".into(), "acme".into()],
                checked: true,
                ..ProbeInfo::default()
            }),
        }),
    );
    if step >= 2 {
        press(&mut app, KeyCode::Enter);
    }
    if step >= 3 {
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        type_text(&mut app, "glpat-secret");
        let screen = text(&render(&mut app));
        assert!(!screen.contains("glpat"), "the token never shows");
        if step > 3 {
            press(&mut app, KeyCode::Esc);
        }
    }
    if step >= 4 {
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char(' '));
    }
    if step >= 5 {
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Right);
    }
    if step >= 6 {
        press(&mut app, KeyCode::Enter);
    }
    if step >= 7 {
        press(&mut app, KeyCode::Enter);
    }
    app
}

fn settings_row(
    name: &str,
    kind: ForgeKind,
    host: &str,
    auth: AuthKind,
    owners: &[&str],
) -> SourceRow {
    SourceRow {
        spec: SourceSpec {
            name: name.into(),
            kind,
            host: host.into(),
            api_url: None,
            auth,
            scope_user: kind == ForgeKind::GitHub,
            owners: owners.iter().map(ToString::to_string).collect(),
        },
        enabled: name != "old-lab",
        in_all: true,
        include_drafts: false,
        tag_colour: Some("cyan".into()),
        repos: Vec::new(),
    }
}

fn settings_snapshot(origin: Option<Origin>) -> settings::Snapshot {
    settings::Snapshot {
        rows: vec![
            settings_row(
                "github.com",
                ForgeKind::GitHub,
                "github.com",
                AuthKind::Cli,
                &["liminal-hq", "acme"],
            ),
            settings_row(
                "work",
                ForgeKind::GitLab,
                "gitlab.work.ca",
                AuthKind::Token,
                &["platform"],
            ),
            settings_row(
                "old-lab",
                ForgeKind::GitLab,
                "gitlab.old.example",
                AuthKind::Env("OLD_LAB_TOKEN".into()),
                &[],
            ),
        ],
        write_target: "/home/u/.config/review-buddy/config.toml".into(),
        editable: origin.is_none(),
        origin,
    }
}

fn settings_table(theme: &str, size: (u16, u16), no_color: bool, origin: Option<Origin>) -> App {
    let mut app = new_app(theme, size, no_color);
    press(&mut app, KeyCode::Char(','));
    update(
        &mut app,
        Msg::Settings(settings::Input::Loaded(Ok(settings_snapshot(origin)))),
    );
    update(
        &mut app,
        Msg::Settings(settings::Input::Tested {
            name: "github.com".into(),
            result: Ok(TestInfo {
                user: "smorris".into(),
                scopes: vec!["repo".into(), "read:org".into()],
                expires: Some("2027-03-01".into()),
                note: None,
            }),
        }),
    );
    update(
        &mut app,
        Msg::Settings(settings::Input::Tested {
            name: "work".into(),
            result: Err("Couldn't sign in to gitlab.work.ca: the token was rejected. To fix it, run review-buddy auth login --host gitlab.work.ca.".into()),
        }),
    );
    app.toasts.clear();
    app
}

/// Frame 1j, one screen per step: 1 table, 2 edit form with a typed token, 3 add picker, 4 add
/// form, 5 remove confirm, 6 a table read from another config file.
fn settings_step(step: u8, theme: &str, size: (u16, u16), no_color: bool) -> App {
    if step == 6 {
        let origin = Origin {
            path: "/etc/xdg/review-buddy/config.toml".into(),
            label: "from /etc/xdg/review-buddy/config.toml".into(),
        };
        let mut app = settings_table(theme, size, no_color, Some(origin));
        press(&mut app, KeyCode::Char('x'));
        assert!(
            app.toasts[0].notice.text.contains("/etc/xdg"),
            "explains why"
        );
        return app;
    }
    let mut app = settings_table(theme, size, no_color, None);
    match step {
        1 => {}
        2 => {
            press(&mut app, KeyCode::Char('j'));
            press(&mut app, KeyCode::Char('e'));
            for _ in 0..5 {
                press(&mut app, KeyCode::Tab);
            }
            type_text(&mut app, "glpat-secret");
            assert!(
                !text(&render(&mut app)).contains("glpat"),
                "the token never shows"
            );
        }
        3 | 4 => {
            press(&mut app, KeyCode::Char('a'));
            update(
                &mut app,
                Msg::Settings(settings::Input::Detected(Detection {
                    hosts: vec![
                        found(
                            "gitlab.example.org",
                            ForgeKind::GitLab,
                            Some((CliTool::Glab, "smorris")),
                        ),
                        found("git.sr.ht", ForgeKind::GitHub, None),
                    ],
                    notes: Vec::new(),
                })),
            );
            if step == 4 {
                press(&mut app, KeyCode::Enter);
            }
        }
        5 => {
            press(&mut app, KeyCode::Char('j'));
            press(&mut app, KeyCode::Char('x'));
        }
        _ => unreachable!(),
    }
    app
}

/// The mixed demo dashboard (GitHub and GitLab sources). `tab` is the dashboard source index:
/// 0 is All.
fn mixed(theme: &str, size: (u16, u16), no_color: bool, tab: usize) -> App {
    let world = DemoWorld::new(parse_iso(DEFAULT_FROZEN).unwrap()).unwrap();
    let snapshot = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(world.snapshot())
        .unwrap();
    let mut app = new_app(theme, size, no_color);
    update(&mut app, Msg::Loaded(Box::new(snapshot)));
    if tab > 0 {
        press(
            &mut app,
            KeyCode::Char(char::from_digit(tab as u32 + 1, 10).unwrap()),
        );
    }
    app
}

fn gitlab_tab() -> usize {
    let app = mixed("liminal-hq", (160, 40), false, 0);
    let at = app
        .state
        .sources
        .iter()
        .position(|s| s.kind == ForgeKind::GitLab)
        .expect("the demo has a GitLab source");
    at + 1
}

fn dashboard_all(theme: &str, size: (u16, u16), no_color: bool) -> App {
    mixed(theme, size, no_color, 0)
}

fn dashboard_gitlab_tab(theme: &str, size: (u16, u16), no_color: bool) -> App {
    let tab = gitlab_tab();
    let app = mixed(theme, size, no_color, tab);
    assert_eq!(app.active_source().map(|s| s.kind), Some(ForgeKind::GitLab));
    app
}

fn dashboard_strip(theme: &str, size: (u16, u16), no_color: bool) -> App {
    assert!(size.0 < 130);
    mixed(theme, size, no_color, 0)
}

/// The scope step with the Back button focused (←) or the Skip button focused (tab, tab, tab).
fn first_run_4_scope_back_focused(theme: &str, size: (u16, u16), no_color: bool) -> App {
    let mut app = first_run(4, theme, size, no_color);
    press(&mut app, KeyCode::Left);
    app
}

fn first_run_5_look_skip_focused(theme: &str, size: (u16, u16), no_color: bool) -> App {
    let mut app = first_run(5, theme, size, no_color);
    for _ in 0..3 {
        press(&mut app, KeyCode::Tab);
    }
    app
}

macro_rules! steps {
    ($($name:ident: $f:ident($step:literal);)*) => {
        $(fn $name(theme: &str, size: (u16, u16), no_color: bool) -> App {
            $f($step, theme, size, no_color)
        })*
    };
}

steps! {
    first_run_1_welcome: first_run(1);
    first_run_2_connect: first_run(2);
    first_run_3_token: first_run(3);
    first_run_4_scope: first_run(4);
    first_run_5_look: first_run(5);
    first_run_6_jax: first_run(6);
    first_run_7_summary: first_run(7);
    settings_1_table: settings_step(1);
    settings_2_edit: settings_step(2);
    settings_3_add_picker: settings_step(3);
    settings_4_add_form: settings_step(4);
    settings_5_remove: settings_step(5);
    settings_6_read_only: settings_step(6);
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
    frame_first_run_4_scope_back_focused_liminal_hq_160x40: first_run_4_scope_back_focused("liminal-hq", 160, 40);
    frame_first_run_5_look_skip_focused_dusk_100x30: first_run_5_look_skip_focused("dusk", 100, 30);
    frame_first_run_1_welcome_liminal_hq_160x40: first_run_1_welcome("liminal-hq", 160, 40);
    frame_first_run_1_welcome_liminal_hq_100x30: first_run_1_welcome("liminal-hq", 100, 30);
    frame_first_run_1_welcome_dusk_160x40: first_run_1_welcome("dusk", 160, 40);
    frame_first_run_1_welcome_dusk_100x30: first_run_1_welcome("dusk", 100, 30);
    frame_first_run_1_welcome_afterglow_dark_160x40: first_run_1_welcome("afterglow-dark", 160, 40);
    frame_first_run_1_welcome_afterglow_dark_100x30: first_run_1_welcome("afterglow-dark", 100, 30);
    frame_first_run_2_connect_liminal_hq_160x40: first_run_2_connect("liminal-hq", 160, 40);
    frame_first_run_2_connect_liminal_hq_100x30: first_run_2_connect("liminal-hq", 100, 30);
    frame_first_run_2_connect_dusk_160x40: first_run_2_connect("dusk", 160, 40);
    frame_first_run_2_connect_dusk_100x30: first_run_2_connect("dusk", 100, 30);
    frame_first_run_2_connect_afterglow_dark_160x40: first_run_2_connect("afterglow-dark", 160, 40);
    frame_first_run_2_connect_afterglow_dark_100x30: first_run_2_connect("afterglow-dark", 100, 30);
    frame_first_run_3_token_liminal_hq_160x40: first_run_3_token("liminal-hq", 160, 40);
    frame_first_run_3_token_liminal_hq_100x30: first_run_3_token("liminal-hq", 100, 30);
    frame_first_run_3_token_dusk_160x40: first_run_3_token("dusk", 160, 40);
    frame_first_run_3_token_dusk_100x30: first_run_3_token("dusk", 100, 30);
    frame_first_run_3_token_afterglow_dark_160x40: first_run_3_token("afterglow-dark", 160, 40);
    frame_first_run_3_token_afterglow_dark_100x30: first_run_3_token("afterglow-dark", 100, 30);
    frame_first_run_4_scope_liminal_hq_160x40: first_run_4_scope("liminal-hq", 160, 40);
    frame_first_run_4_scope_liminal_hq_100x30: first_run_4_scope("liminal-hq", 100, 30);
    frame_first_run_4_scope_dusk_160x40: first_run_4_scope("dusk", 160, 40);
    frame_first_run_4_scope_dusk_100x30: first_run_4_scope("dusk", 100, 30);
    frame_first_run_4_scope_afterglow_dark_160x40: first_run_4_scope("afterglow-dark", 160, 40);
    frame_first_run_4_scope_afterglow_dark_100x30: first_run_4_scope("afterglow-dark", 100, 30);
    frame_first_run_5_look_liminal_hq_160x40: first_run_5_look("liminal-hq", 160, 40);
    frame_first_run_5_look_liminal_hq_100x30: first_run_5_look("liminal-hq", 100, 30);
    frame_first_run_5_look_dusk_160x40: first_run_5_look("dusk", 160, 40);
    frame_first_run_5_look_dusk_100x30: first_run_5_look("dusk", 100, 30);
    frame_first_run_5_look_afterglow_dark_160x40: first_run_5_look("afterglow-dark", 160, 40);
    frame_first_run_5_look_afterglow_dark_100x30: first_run_5_look("afterglow-dark", 100, 30);
    frame_first_run_6_jax_liminal_hq_160x40: first_run_6_jax("liminal-hq", 160, 40);
    frame_first_run_6_jax_liminal_hq_100x30: first_run_6_jax("liminal-hq", 100, 30);
    frame_first_run_6_jax_dusk_160x40: first_run_6_jax("dusk", 160, 40);
    frame_first_run_6_jax_dusk_100x30: first_run_6_jax("dusk", 100, 30);
    frame_first_run_6_jax_afterglow_dark_160x40: first_run_6_jax("afterglow-dark", 160, 40);
    frame_first_run_6_jax_afterglow_dark_100x30: first_run_6_jax("afterglow-dark", 100, 30);
    frame_first_run_7_summary_liminal_hq_160x40: first_run_7_summary("liminal-hq", 160, 40);
    frame_first_run_7_summary_liminal_hq_100x30: first_run_7_summary("liminal-hq", 100, 30);
    frame_first_run_7_summary_dusk_160x40: first_run_7_summary("dusk", 160, 40);
    frame_first_run_7_summary_dusk_100x30: first_run_7_summary("dusk", 100, 30);
    frame_first_run_7_summary_afterglow_dark_160x40: first_run_7_summary("afterglow-dark", 160, 40);
    frame_first_run_7_summary_afterglow_dark_100x30: first_run_7_summary("afterglow-dark", 100, 30);
    frame_settings_1_table_liminal_hq_160x40: settings_1_table("liminal-hq", 160, 40);
    frame_settings_1_table_liminal_hq_100x30: settings_1_table("liminal-hq", 100, 30);
    frame_settings_1_table_dusk_160x40: settings_1_table("dusk", 160, 40);
    frame_settings_1_table_dusk_100x30: settings_1_table("dusk", 100, 30);
    frame_settings_1_table_afterglow_dark_160x40: settings_1_table("afterglow-dark", 160, 40);
    frame_settings_1_table_afterglow_dark_100x30: settings_1_table("afterglow-dark", 100, 30);
    frame_settings_2_edit_liminal_hq_160x40: settings_2_edit("liminal-hq", 160, 40);
    frame_settings_2_edit_liminal_hq_100x30: settings_2_edit("liminal-hq", 100, 30);
    frame_settings_2_edit_dusk_160x40: settings_2_edit("dusk", 160, 40);
    frame_settings_2_edit_dusk_100x30: settings_2_edit("dusk", 100, 30);
    frame_settings_2_edit_afterglow_dark_160x40: settings_2_edit("afterglow-dark", 160, 40);
    frame_settings_2_edit_afterglow_dark_100x30: settings_2_edit("afterglow-dark", 100, 30);
    frame_settings_3_add_picker_liminal_hq_160x40: settings_3_add_picker("liminal-hq", 160, 40);
    frame_settings_3_add_picker_liminal_hq_100x30: settings_3_add_picker("liminal-hq", 100, 30);
    frame_settings_3_add_picker_dusk_160x40: settings_3_add_picker("dusk", 160, 40);
    frame_settings_3_add_picker_dusk_100x30: settings_3_add_picker("dusk", 100, 30);
    frame_settings_3_add_picker_afterglow_dark_160x40: settings_3_add_picker("afterglow-dark", 160, 40);
    frame_settings_3_add_picker_afterglow_dark_100x30: settings_3_add_picker("afterglow-dark", 100, 30);
    frame_settings_4_add_form_liminal_hq_160x40: settings_4_add_form("liminal-hq", 160, 40);
    frame_settings_4_add_form_liminal_hq_100x30: settings_4_add_form("liminal-hq", 100, 30);
    frame_settings_4_add_form_dusk_160x40: settings_4_add_form("dusk", 160, 40);
    frame_settings_4_add_form_dusk_100x30: settings_4_add_form("dusk", 100, 30);
    frame_settings_4_add_form_afterglow_dark_160x40: settings_4_add_form("afterglow-dark", 160, 40);
    frame_settings_4_add_form_afterglow_dark_100x30: settings_4_add_form("afterglow-dark", 100, 30);
    frame_settings_5_remove_liminal_hq_160x40: settings_5_remove("liminal-hq", 160, 40);
    frame_settings_5_remove_liminal_hq_100x30: settings_5_remove("liminal-hq", 100, 30);
    frame_settings_5_remove_dusk_160x40: settings_5_remove("dusk", 160, 40);
    frame_settings_5_remove_dusk_100x30: settings_5_remove("dusk", 100, 30);
    frame_settings_5_remove_afterglow_dark_160x40: settings_5_remove("afterglow-dark", 160, 40);
    frame_settings_5_remove_afterglow_dark_100x30: settings_5_remove("afterglow-dark", 100, 30);
    frame_settings_6_read_only_liminal_hq_160x40: settings_6_read_only("liminal-hq", 160, 40);
    frame_settings_6_read_only_liminal_hq_100x30: settings_6_read_only("liminal-hq", 100, 30);
    frame_settings_6_read_only_dusk_160x40: settings_6_read_only("dusk", 160, 40);
    frame_settings_6_read_only_dusk_100x30: settings_6_read_only("dusk", 100, 30);
    frame_settings_6_read_only_afterglow_dark_160x40: settings_6_read_only("afterglow-dark", 160, 40);
    frame_settings_6_read_only_afterglow_dark_100x30: settings_6_read_only("afterglow-dark", 100, 30);
    frame_dashboard_all_liminal_hq_160x40: dashboard_all("liminal-hq", 160, 40);
    frame_dashboard_all_liminal_hq_100x30: dashboard_all("liminal-hq", 100, 30);
    frame_dashboard_all_dusk_160x40: dashboard_all("dusk", 160, 40);
    frame_dashboard_all_dusk_100x30: dashboard_all("dusk", 100, 30);
    frame_dashboard_all_afterglow_dark_160x40: dashboard_all("afterglow-dark", 160, 40);
    frame_dashboard_all_afterglow_dark_100x30: dashboard_all("afterglow-dark", 100, 30);
    frame_dashboard_gitlab_tab_liminal_hq_160x40: dashboard_gitlab_tab("liminal-hq", 160, 40);
    frame_dashboard_gitlab_tab_liminal_hq_100x30: dashboard_gitlab_tab("liminal-hq", 100, 30);
    frame_dashboard_gitlab_tab_dusk_160x40: dashboard_gitlab_tab("dusk", 160, 40);
    frame_dashboard_gitlab_tab_dusk_100x30: dashboard_gitlab_tab("dusk", 100, 30);
    frame_dashboard_gitlab_tab_afterglow_dark_160x40: dashboard_gitlab_tab("afterglow-dark", 160, 40);
    frame_dashboard_gitlab_tab_afterglow_dark_100x30: dashboard_gitlab_tab("afterglow-dark", 100, 30);
    frame_dashboard_strip_liminal_hq_129x40: dashboard_strip("liminal-hq", 129, 40);
    frame_dashboard_strip_dusk_129x40: dashboard_strip("dusk", 129, 40);
    frame_dashboard_strip_afterglow_dark_129x40: dashboard_strip("afterglow-dark", 129, 40);
}

plain_frames! {
    frame_first_run_4_scope_back_focused_no_color_100x30: first_run_4_scope_back_focused(100, 30);
    frame_first_run_5_look_skip_focused_no_color_100x30: first_run_5_look_skip_focused(100, 30);
    frame_first_run_1_welcome_no_color_160x40: first_run_1_welcome(160, 40);
    frame_first_run_1_welcome_no_color_100x30: first_run_1_welcome(100, 30);
    frame_first_run_2_connect_no_color_160x40: first_run_2_connect(160, 40);
    frame_first_run_2_connect_no_color_100x30: first_run_2_connect(100, 30);
    frame_first_run_3_token_no_color_160x40: first_run_3_token(160, 40);
    frame_first_run_3_token_no_color_100x30: first_run_3_token(100, 30);
    frame_first_run_4_scope_no_color_160x40: first_run_4_scope(160, 40);
    frame_first_run_4_scope_no_color_100x30: first_run_4_scope(100, 30);
    frame_first_run_5_look_no_color_160x40: first_run_5_look(160, 40);
    frame_first_run_5_look_no_color_100x30: first_run_5_look(100, 30);
    frame_first_run_6_jax_no_color_160x40: first_run_6_jax(160, 40);
    frame_first_run_6_jax_no_color_100x30: first_run_6_jax(100, 30);
    frame_first_run_7_summary_no_color_160x40: first_run_7_summary(160, 40);
    frame_first_run_7_summary_no_color_100x30: first_run_7_summary(100, 30);
    frame_settings_1_table_no_color_160x40: settings_1_table(160, 40);
    frame_settings_1_table_no_color_100x30: settings_1_table(100, 30);
    frame_settings_2_edit_no_color_160x40: settings_2_edit(160, 40);
    frame_settings_2_edit_no_color_100x30: settings_2_edit(100, 30);
    frame_settings_3_add_picker_no_color_160x40: settings_3_add_picker(160, 40);
    frame_settings_3_add_picker_no_color_100x30: settings_3_add_picker(100, 30);
    frame_settings_4_add_form_no_color_160x40: settings_4_add_form(160, 40);
    frame_settings_4_add_form_no_color_100x30: settings_4_add_form(100, 30);
    frame_settings_5_remove_no_color_160x40: settings_5_remove(160, 40);
    frame_settings_5_remove_no_color_100x30: settings_5_remove(100, 30);
    frame_settings_6_read_only_no_color_160x40: settings_6_read_only(160, 40);
    frame_settings_6_read_only_no_color_100x30: settings_6_read_only(100, 30);
    frame_dashboard_all_no_color_160x40: dashboard_all(160, 40);
    frame_dashboard_all_no_color_100x30: dashboard_all(100, 30);
    frame_dashboard_gitlab_tab_no_color_160x40: dashboard_gitlab_tab(160, 40);
    frame_dashboard_gitlab_tab_no_color_100x30: dashboard_gitlab_tab(100, 30);
}

#[test]
fn frames_contain_what_the_spec_shows() {
    for theme in THEMES {
        for size in SIZES {
            let welcome = text(&render(&mut first_run(1, theme, size, false)));
            assert!(
                welcome.contains("Getting started"),
                "{theme} {size:?}\n{welcome}"
            );
            let summary = text(&render(&mut first_run(7, theme, size, false)));
            assert!(
                summary.contains("Jax around"),
                "{theme} {size:?}\n{summary}"
            );
            let table = text(&render(&mut settings_step(1, theme, size, false)));
            assert!(
                table.contains("gitlab.work.ca"),
                "{theme} {size:?}\n{table}"
            );
            let remove = text(&render(&mut settings_step(5, theme, size, false)));
            assert!(
                remove.contains("Remove work (gitlab.work.ca)?"),
                "{theme} {size:?}\n{remove}"
            );
            let all = text(&render(&mut dashboard_all(theme, size, false)));
            assert!(all.contains("Waiting on you"), "{theme} {size:?}\n{all}");
        }
    }
}

#[test]
fn the_first_run_card_border_uses_the_accent() {
    for theme in THEMES {
        let mut app = first_run(1, theme, (160, 40), false);
        let buffer = render(&mut app);
        let corner = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .map(|(x, y)| &buffer[(x, y)])
            .find(|c| c.symbol() == "╭" && Some(c.fg) == role(&app, Role::Accent));
        assert!(corner.is_some(), "{theme}: a card with an accent border");
    }
}

#[test]
fn the_wordmark_gradient_survives_in_the_dashboard_of_a_mixed_queue() {
    for theme in THEMES {
        let mut app = dashboard_all(theme, (160, 40), false);
        let buffer = render(&mut app);
        let (x, y) = find(&buffer, WORDMARK).unwrap();
        let stops = app.palette.wordmark();
        assert_eq!(
            buffer[(x, y)].fg,
            style::colour(*stops.first().unwrap()),
            "{theme}"
        );
    }
}

#[test]
fn source_tags_take_their_forge_colour_from_tag_fg() {
    for theme in THEMES {
        let mut app = dashboard_all(theme, (160, 40), false);
        let buffer = render(&mut app);
        let github = role(&app, Role::Github).unwrap();
        let gitlab = role(&app, Role::Gitlab).unwrap();
        assert_ne!(github, gitlab, "{theme}");
        let sources: Vec<Source> = app.state.sources.clone();
        for (tag, kind, want) in [
            ("GH", ForgeKind::GitHub, github),
            ("GL", ForgeKind::GitLab, gitlab),
        ] {
            let source = sources
                .iter()
                .find(|s| s.kind == kind && s.tag_colour.is_none());
            let expected = style::tag_fg(&app.palette, source, kind).fg;
            if source.is_some() {
                assert_eq!(expected, Some(want), "{theme} {tag}");
            }
            let (x, y) = find(&buffer, tag).unwrap_or_else(|| panic!("{theme}: a {tag} tag"));
            let fg = buffer[(x, y)].fg;
            assert_ne!(fg, Color::Reset, "{theme} {tag} is coloured");
        }
    }
}

#[test]
fn the_selected_settings_row_is_marked_by_a_background_the_others_lack() {
    for theme in THEMES {
        let mut app = settings_step(1, theme, (160, 40), false);
        let buffer = render(&mut app);
        let (x, y) = find(&buffer, "github.com").unwrap();
        let (ox, oy) = find(&buffer, "old-lab").unwrap();
        assert_ne!(buffer[(x, y)].bg, buffer[(ox, oy)].bg, "{theme}");
    }
}

#[test]
fn no_colour_settings_keep_the_checks_as_text() {
    let mut app = settings_step(1, "liminal-hq", (160, 40), true);
    let dump = plain_dump(&mut app);
    assert!(dump.contains("✓ smorris"), "{dump}");
    assert!(dump.contains("✗ Couldn't sign in"), "{dump}");
}
