//! The first-run screen (frame 1i): a calm card that walks through connecting accounts.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use rb_core::ForgeKind;
use rb_theme::{Palette, Role, Theme, BUILTIN_IDS};
use unicode_width::UnicodeWidthStr;

use super::chrome::truncate;
use super::{style, HitMap};
use crate::app::{Action, App};
use crate::setup::flow::{required_scopes, Click, Flow, ScopeItem, Step};
use crate::setup::plain::scope_summary;

const CARD_WIDTH: u16 = 78;
const THEME_NAMES: [&str; 4] = ["Liminal HQ", "Dusk", "Afterglow Dark", "Afterglow Light"];

struct Body<'a> {
    palette: &'a Palette,
    width: usize,
    lines: Vec<Line<'static>>,
    rows: Vec<(usize, Click)>,
}

impl<'a> Body<'a> {
    fn new(palette: &'a Palette, width: usize) -> Self {
        Self {
            palette,
            width,
            lines: Vec::new(),
            rows: Vec::new(),
        }
    }

    fn blank(&mut self) {
        self.lines.push(Line::raw(""));
    }

    fn text(&mut self, role: Role, text: &str) {
        for part in wrap(text, self.width) {
            self.lines
                .push(Line::styled(part, style::fg(self.palette, role)));
        }
    }

    fn bright(&mut self, text: &str) {
        self.lines.push(Line::styled(
            text.to_string(),
            style::fg(self.palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ));
    }

    fn row(&mut self, spans: Vec<Span<'static>>, click: Click, cursor: bool) {
        let mut line = Line::from(spans);
        if cursor {
            line = line.style(style::bg(self.palette, Role::Selection));
        }
        self.rows.push((self.lines.len(), click));
        self.lines.push(line);
    }

    fn span(&self, role: Role, text: impl Into<String>) -> Span<'static> {
        Span::styled(text.into(), style::fg(self.palette, role))
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let joined = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if UnicodeWidthStr::width(joined.as_str()) > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = joined;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn title(step: Step) -> &'static str {
    match step {
        Step::Welcome | Step::Done => " Getting started ",
        Step::Connect => " 1 · Connect your sources ",
        Step::Scope => " 2 · What to include ",
        Step::Look => " 3 · Pick a look ",
        Step::Jax => " 4 · A little company ",
        Step::Summary => " 5 · Ready to save ",
    }
}

pub fn draw(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let Some(flow) = app.setup.as_ref() else {
        return;
    };
    let palette = &app.palette;
    let width = area.width.saturating_sub(4).min(CARD_WIDTH);
    let inner = usize::from(width.saturating_sub(4));
    let mut body = Body::new(palette, inner);
    match flow.step {
        Step::Welcome | Step::Done => welcome(&mut body, flow),
        Step::Connect => connect(&mut body, flow),
        Step::Scope => scope(&mut body, flow),
        Step::Look => look(&mut body, flow, app),
        Step::Jax => jax(&mut body, flow),
        Step::Summary => summary(&mut body, flow),
    }
    if let Some(message) = &flow.message {
        body.blank();
        body.text(Role::Warning, message);
    }

    let content = body.lines.len() as u16;
    let height = (content + 4).min(area.height);
    let card = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style::fg(palette, Role::Accent))
        .title(Span::styled(
            title(flow.step),
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ));
    let text_area = Rect::new(
        card.x + 2,
        card.y + 1,
        card.width.saturating_sub(4),
        card.height.saturating_sub(4),
    );
    frame.render_widget(block, card);
    frame.render_widget(Paragraph::new(body.lines.clone()), text_area);
    for (line, click) in &body.rows {
        let y = text_area.y + *line as u16;
        if y < text_area.bottom() {
            hits.push(
                Rect::new(card.x + 1, y, card.width.saturating_sub(2), 1),
                Action::Setup(*click),
            );
        }
    }
    buttons(frame, app, flow, card, hits);
}

fn buttons(frame: &mut Frame, app: &App, flow: &Flow, card: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let y = card.bottom().saturating_sub(2);
    let items: Vec<(&str, Click, Role)> = if flow.asking_replace {
        vec![
            ("No, keep it ⏎", Click::No, Role::Accent),
            ("Yes, replace it (y)", Click::Yes, Role::Muted),
        ]
    } else if flow.step == Step::Summary {
        vec![
            ("Back", Click::Back, Role::Muted),
            ("Save ⏎", Click::Next, Role::Accent),
            ("Skip for now · esc", Click::Skip, Role::Muted),
        ]
    } else if flow.step == Step::Welcome {
        vec![
            ("Continue ⏎", Click::Next, Role::Accent),
            ("Skip for now · esc", Click::Skip, Role::Muted),
        ]
    } else {
        vec![
            ("Back", Click::Back, Role::Muted),
            ("Continue ⏎", Click::Next, Role::Accent),
            ("Skip for now · esc", Click::Skip, Role::Muted),
        ]
    };
    let mut x = card.x + 2;
    for (label, click, role) in items {
        let text = format!("[ {label} ]");
        let w = UnicodeWidthStr::width(text.as_str()) as u16;
        if x + w > card.right().saturating_sub(1) {
            break;
        }
        let rect = Rect::new(x, y, w, 1);
        frame.render_widget(
            Paragraph::new(Line::styled(text, style::fg(palette, role))),
            rect,
        );
        hits.push(rect, Action::Setup(click));
        x += w + 2;
    }
}

fn welcome(body: &mut Body, flow: &Flow) {
    body.bright("Welcome to Review Buddy.");
    body.text(
        Role::Muted,
        "Every pull and merge request, in one quiet queue.",
    );
    body.blank();
    body.text(
        Role::Text,
        "Let's connect the accounts you already use. It takes about a minute, and nothing is saved until you say so.",
    );
    body.blank();
    let line = if flow.detecting {
        "Looking for your accounts…".to_string()
    } else {
        match flow.hosts.len() {
            0 => "I didn't find an account yet. You can add a token on the next step.".to_string(),
            1 => "Found 1 place you might want to connect.".to_string(),
            n => format!("Found {n} places you might want to connect."),
        }
    };
    body.text(Role::Muted, &line);
    for note in &flow.notes {
        body.text(Role::Muted, note);
    }
}

fn connect(body: &mut Body, flow: &Flow) {
    if flow.hosts.is_empty() {
        body.text(
            Role::Text,
            "No GitHub or GitLab accounts turned up. Sign in with gh auth login or glab auth login and run review-buddy --setup again, or press esc and add a source to config.toml.",
        );
        return;
    }
    body.text(
        Role::Muted,
        "space picks a row · ⏎ continues · a row with no credentials asks for a token",
    );
    body.blank();
    for (i, row) in flow.hosts.iter().enumerate() {
        let cursor = i == flow.cursor;
        let (glyph, status) = row.status();
        let tag = match row.kind {
            ForgeKind::GitHub => "GH",
            ForgeKind::GitLab => "GL",
        };
        let glyph_role = if glyph == '✓' {
            Role::Success
        } else {
            Role::Muted
        };
        let used = 4 + 4 + 2 + 3 + row.host.len() + 2;
        let status = truncate(&status, body.width.saturating_sub(used));
        let spans = vec![
            body.span(Role::Accent, if cursor { "› " } else { "  " }),
            body.span(Role::Text, if row.selected { "[x] " } else { "[ ] " }),
            body.span(glyph_role, format!("{glyph} ")),
            body.span(Role::TextSecondary, format!("{tag} ")),
            Span::styled(row.host.clone(), style::fg(body.palette, Role::TextBright)),
            body.span(Role::Muted, format!("  {status}")),
        ];
        body.row(spans, Click::Row(i), cursor);
        if let Some(info) = row.connected() {
            for hint in &info.hints {
                for part in wrap(hint, body.width.saturating_sub(6)) {
                    body.lines.push(Line::styled(
                        format!("      {part}"),
                        style::fg(body.palette, Role::Muted),
                    ));
                }
            }
        }
        if cursor && flow.token_open {
            token_field(body, flow, row.kind);
        }
    }
}

fn token_field(body: &mut Body, flow: &Flow, kind: ForgeKind) {
    let dots = "•".repeat(flow.token.len().min(40));
    body.lines.push(Line::from(vec![
        body.span(Role::TextSecondary, "      Token  "),
        body.span(Role::TextBright, format!("{dots}▏")),
    ]));
    let scopes = required_scopes(kind).join(", ");
    body.lines.push(Line::styled(
        format!("      Needs {scopes} · ⏎ test and save to your keyring · esc cancel"),
        style::fg(body.palette, Role::Muted),
    ));
    if let Some(row) = flow.hosts.get(flow.cursor) {
        if let crate::setup::Conn::Failed(reason) = &row.conn {
            for part in wrap(reason, body.width.saturating_sub(6)) {
                body.lines.push(Line::styled(
                    format!("      {part}"),
                    style::fg(body.palette, Role::Warning),
                ));
            }
        }
    }
}

fn scope(body: &mut Body, flow: &Flow) {
    let items = flow.scope_items();
    if items.is_empty() {
        body.text(
            Role::Text,
            "Everything you can see on each host will be included.",
        );
        return;
    }
    body.text(
        Role::Muted,
        "Pick what each account covers. Leave everything unticked to include everything you can see.",
    );
    body.blank();
    for (n, item) in items.iter().enumerate() {
        let cursor = n == flow.cursor;
        let (host, on, label) = match *item {
            ScopeItem::User(h) => (
                &flow.hosts[h],
                flow.hosts[h].user,
                "my own repositories".to_string(),
            ),
            ScopeItem::Org(h, o) => {
                let host = &flow.hosts[h];
                let noun = if host.kind == ForgeKind::GitHub {
                    "org"
                } else {
                    "group"
                };
                (host, host.orgs[o].1, format!("{noun} {}", host.orgs[o].0))
            }
        };
        let spans = vec![
            body.span(Role::Accent, if cursor { "› " } else { "  " }),
            body.span(Role::Text, if on { "[x] " } else { "[ ] " }),
            Span::styled(host.host.clone(), style::fg(body.palette, Role::TextBright)),
            body.span(Role::Muted, format!(" · {label}")),
        ];
        body.row(spans, Click::Row(n), cursor);
    }
}

fn look(body: &mut Body, flow: &Flow, app: &App) {
    body.text(
        Role::Muted,
        "← → changes the screen live. You can change it later with T.",
    );
    body.blank();
    for (i, id) in BUILTIN_IDS.iter().enumerate() {
        let cursor = i == flow.theme;
        let mut spans = vec![
            body.span(Role::Accent, if cursor { "› " } else { "  " }),
            body.span(
                if cursor { Role::TextBright } else { Role::Text },
                format!("{:<17}", THEME_NAMES[i]),
            ),
        ];
        if let Ok(theme) = Theme::builtin(id) {
            let preview = Palette::new(theme, app.palette.depth(), app.palette.no_color());
            for role in [
                Role::Accent,
                Role::Success,
                Role::Warning,
                Role::Interactive,
            ] {
                let st = preview
                    .colour(role)
                    .map_or_else(Style::default, |c| Style::default().fg(style::colour(c)));
                spans.push(Span::styled("██ ", st));
            }
        }
        body.row(spans, Click::Theme(i), cursor);
    }
}

fn jax(body: &mut Body, flow: &Flow) {
    body.text(
        Role::Muted,
        "Jax is a small otter who keeps you company in a corner of the screen. He stays out of the way.",
    );
    body.blank();
    let spans = vec![
        body.span(Role::Text, if flow.jax { "[x] " } else { "[ ] " }),
        body.span(Role::TextBright, "Keep Jax around"),
        body.span(Role::Muted, "  (J or space)"),
    ];
    body.row(spans, Click::Jax, true);
}

fn summary(body: &mut Body, flow: &Flow) {
    if flow.writing {
        body.text(Role::Text, "Saving…");
        return;
    }
    body.text(Role::Muted, "Here's what will be saved:");
    body.blank();
    for (_, row) in flow.chosen() {
        let who = row.account().map(|a| format!(" ({a})")).unwrap_or_default();
        body.lines.push(Line::from(vec![
            Span::styled(
                format!("  {}{who}", row.host),
                style::fg(body.palette, Role::TextBright),
            ),
            body.span(Role::Muted, format!(" · {}", scope_summary(row))),
        ]));
    }
    body.blank();
    let jax = if flow.jax { "Jax around" } else { "Jax away" };
    body.text(
        Role::Text,
        &format!("  Look: {} · {jax}", THEME_NAMES[flow.theme]),
    );
    body.text(Role::Muted, &format!("  Config: {}", flow.target.display()));
    if flow.asking_replace {
        body.blank();
        body.text(
            Role::Warning,
            "A config file is already there. Replace it? A copy is kept as config.toml.bak.",
        );
    }
}
