//! First-run behaviour: clicks reach the flow. The screens themselves are pinned in `frames_v02.rs`.

use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};
use rb_core::ForgeKind;
use rb_platform::auth::CliTool;
use rb_theme::ColourDepth;
use review_buddy::app::{setup, update, App, AppConfig, Msg};
use review_buddy::setup::{Detection, Evidence, Flow, FoundHost, Input, ProbeInfo};
use review_buddy::ui::{self, HitMap};

fn app(width: u16, height: u16, theme: &str) -> App {
    let mut app = App::new(AppConfig {
        theme_id: theme.into(),
        depth: ColourDepth::TrueColour,
        no_color: false,
        size: (width, height),
    });
    let flow = Flow::new(
        "/home/u/.config/review-buddy/config.toml".into(),
        false,
        theme,
        true,
    );
    setup::start(&mut app, flow);
    app
}

fn send(app: &mut App, input: Input) {
    update(app, Msg::Setup(input));
}

fn detect(app: &mut App) {
    let host = |host: &str, kind, cli: Option<&str>| FoundHost {
        host: host.into(),
        kind,
        evidence: vec![match cli {
            Some(user) => Evidence::Cli {
                tool: CliTool::Gh,
                user: user.into(),
            },
            None => Evidence::GitConfig,
        }],
    };
    send(
        app,
        Input::Detected(Detection {
            hosts: vec![
                host("github.com", ForgeKind::GitHub, Some("smorris")),
                host("gitlab.work.ca", ForgeKind::GitLab, None),
            ],
            notes: Vec::new(),
        }),
    );
    send(
        app,
        Input::Probed {
            host: "github.com".into(),
            result: Ok(ProbeInfo {
                login: "smorris".into(),
                orgs: vec!["liminal-hq".into(), "acme".into()],
                checked: true,
                ..ProbeInfo::default()
            }),
        },
    );
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

#[test]
fn clicks_on_buttons_and_rows_reach_the_flow() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut a = app(160, 40, "liminal-hq");
    detect(&mut a);
    let (screen, hits) = render(&a, 160, 40);
    let find = |needle: &str| {
        screen.lines().enumerate().find_map(|(y, l)| {
            l.find(needle)
                .map(|b| (l[..b].chars().count() as u16, y as u16))
        })
    };
    let (x, y) = find("Continue").unwrap();
    assert!(hits.at(x, y).is_some());
    a.hits = hits;
    update(
        &mut a,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(
        a.setup.as_ref().unwrap().step,
        review_buddy::setup::Step::Connect
    );
}
