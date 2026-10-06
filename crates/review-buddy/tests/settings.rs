//! Settings → Sources behaviour: confirm, clicks, check results. The screens themselves are pinned
//! in `frames_v02.rs`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_core::ForgeKind;
use rb_theme::ColourDepth;
use review_buddy::app::{update, Action, App, AppConfig, Msg};
use review_buddy::settings::{Click, Input, Origin, Snapshot, SourceRow, TestInfo};
use review_buddy::setup::{AuthKind, SourceSpec};
use review_buddy::ui::{self, HitMap};

fn row(name: &str, kind: ForgeKind, host: &str, auth: AuthKind, owners: &[&str]) -> SourceRow {
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

fn snapshot(origin: Option<Origin>) -> Snapshot {
    Snapshot {
        rows: vec![
            row(
                "github.com",
                ForgeKind::GitHub,
                "github.com",
                AuthKind::Cli,
                &["liminal-hq", "acme"],
            ),
            row(
                "work",
                ForgeKind::GitLab,
                "gitlab.work.ca",
                AuthKind::Token,
                &["platform"],
            ),
            row(
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

fn app(w: u16, h: u16, theme: &str, origin: Option<Origin>) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (w, h),
    });
    key(&mut app, KeyCode::Char(','));
    update(&mut app, Msg::Settings(Input::Loaded(Ok(snapshot(origin)))));
    update(
        &mut app,
        Msg::Settings(Input::Tested {
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
        Msg::Settings(Input::Tested {
            name: "work".into(),
            result: Err("Couldn't sign in to gitlab.work.ca: the token was rejected. To fix it, run review-buddy auth login --host gitlab.work.ca.".into()),
        }),
    );
    app.toasts.clear();
    app
}

fn text(buffer: &Buffer) -> String {
    let width = usize::from(buffer.area.width);
    buffer
        .content()
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render(app: &App, w: u16, h: u16) -> (String, HitMap) {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut hits = HitMap::default();
    terminal.draw(|f| hits = ui::draw(f, app)).unwrap();
    (text(terminal.backend().buffer()), hits)
}

fn key(app: &mut App, code: KeyCode) {
    update(app, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

#[test]
fn the_remove_confirm_opens_on_no_and_names_what_goes() {
    let mut a = app(160, 40, "liminal-hq", None);
    key(&mut a, KeyCode::Char('j'));
    key(&mut a, KeyCode::Char('x'));
    let (screen, hits) = render(&a, 160, 40);
    assert!(screen.contains("Remove work (gitlab.work.ca)?"), "{screen}");
    assert!(
        screen.contains("keyring (review-buddy/gitlab.work.ca) stays"),
        "{screen}"
    );
    assert!(
        screen.contains("› No, keep it  ⏎ ‹"),
        "No is where focus starts"
    );
    let buttons: Vec<_> = (0..40)
        .flat_map(|y| (0..160).map(move |x| (x, y)))
        .filter_map(|(x, y)| match hits.at(x, y) {
            Some(Action::Settings(Click::Choose(i))) => Some(*i),
            _ => None,
        })
        .collect();
    assert!(buttons.contains(&0) && buttons.contains(&1) && buttons.contains(&2));
}

#[test]
fn rows_are_clickable_and_the_footer_hints_are_actions() {
    let mut a = app(160, 40, "liminal-hq", None);
    let (_, hits) = render(&a, 160, 40);
    let rows: Vec<usize> = (0..40)
        .flat_map(|y| (0..160).map(move |x| (x, y)))
        .filter_map(|(x, y)| match hits.at(x, y) {
            Some(Action::Settings(Click::Row(i))) => Some(*i),
            _ => None,
        })
        .collect();
    assert!(rows.contains(&0) && rows.contains(&2));
    a.hits = hits;
    let pos = (0..160u16)
        .flat_map(|x| (0..40u16).map(move |y| (x, y)))
        .find(|(x, y)| a.hits.at(*x, *y) == Some(&Action::Settings(Click::Row(1))))
        .unwrap();
    update(
        &mut a,
        Msg::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: pos.0,
            row: pos.1,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(a.settings.as_ref().unwrap().cursor, 1);
}

#[test]
fn checks_show_in_the_table_without_colour() {
    let a = app(160, 40, "liminal-hq", None);
    let (screen, _) = render(&a, 160, 40);
    assert!(screen.contains("✓ smorris · expires 2027-03-01"));
    assert!(screen.contains("✗ Couldn't sign in to gitlab.work.ca"));
    assert!(screen.contains("· not tested"));
}

#[test]
fn demo_settings_are_labelled_and_read_only() {
    use rb_core::{AuthMode, Scope, Source, SourceId};
    let mut a = App::new(AppConfig {
        theme_id: "liminal-hq".into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (120, 30),
    });
    a.demo = true;
    a.state.sources = vec![Source {
        id: SourceId::new("demo-gh"),
        kind: ForgeKind::GitHub,
        host: "github.com".into(),
        label: "demo-gh".into(),
        scope: Scope::default(),
        auth: AuthMode::Cli,
        in_all: true,
        include_drafts: false,
        tag_colour: None,
    }];
    key(&mut a, KeyCode::Char(','));
    let (screen, _) = render(&a, 120, 30);
    assert!(screen.contains("demo sources (demo)"), "{screen}");
    assert!(screen.contains("demo-gh"));
    key(&mut a, KeyCode::Char('x'));
    assert!(a.settings.as_ref().unwrap().modal.is_none());
}
