//! The Settings screen (frame 1j): a nav on the left and the Sources table beside it, with the
//! selected source's details below and the add, edit and remove dialogs on top.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
    Frame,
};
use rb_core::ForgeKind;
use rb_theme::{Palette, Role};
use unicode_width::UnicodeWidthStr;

use super::chrome::{truncate, Hint};
use super::text::wrap;
use super::{style, HitMap};
use crate::app::{Action, App};
use crate::settings::state::{AuthChoice, Mode};
use crate::settings::{
    Check, Click, Field, Form, Modal, Pick, Remove, RemoveChoice, SourceRow, State,
};

const NAV_WIDTH: u16 = 16;
/// Lines the details block takes under the table.
const DETAILS: usize = 8;
/// The Scope column only appears when there is room for it beside the status.
const WIDE: usize = 112;

pub fn draw(frame: &mut Frame, app: &App, body: Rect, hits: &mut HitMap) {
    let Some(state) = app.settings.as_ref() else {
        return;
    };
    let palette = &app.palette;
    let nav = Rect::new(body.x, body.y, NAV_WIDTH.min(body.width), body.height);
    draw_nav(frame, palette, nav);
    let content = Rect::new(
        body.x + nav.width,
        body.y,
        body.width.saturating_sub(nav.width),
        body.height,
    );
    draw_sources(frame, app, state, content, hits);
    if let Some(modal) = &state.modal {
        draw_modal(frame, app, state, modal, body, hits);
    }
}

fn draw_nav(frame: &mut Frame, palette: &Palette, area: Rect) {
    let lines = vec![
        Line::styled(
            " Settings",
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::styled(
            "› Sources",
            style::fg(palette, Role::Accent)
                .add_modifier(Modifier::BOLD)
                .patch(style::bg(palette, Role::Selection)),
        ),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn cells(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn pad(text: &str, width: usize) -> String {
    let text = truncate(text, width);
    let fill = width.saturating_sub(cells(&text));
    format!("{text}{}", " ".repeat(fill))
}

struct Columns {
    name: usize,
    host: usize,
    auth: usize,
    scope: Option<usize>,
    status: usize,
}

fn columns(width: usize) -> Columns {
    // cursor 2, on 3, kind 2, then name, host, auth, in All 3, and a gap between each.
    let wide = width >= WIDE;
    let (name, host, auth) = if wide { (18, 22, 14) } else { (14, 18, 10) };
    let scope = wide.then_some(26);
    let fixed = 2 + 3 + 2 + name + host + auth + 3 + scope.unwrap_or(0);
    let gaps = if wide { 8 } else { 7 };
    Columns {
        name,
        host,
        auth,
        scope,
        status: width.saturating_sub(fixed + gaps).max(8),
    }
}

fn check_text(state: &State, row: &SourceRow) -> (Role, String) {
    let name = &row.spec.name;
    if state.testing.contains(name) {
        return (Role::Muted, "· checking…".to_string());
    }
    match state.checks.get(name) {
        Some(Check::Ok(info)) => {
            let mut text = format!(
                "✓ {}",
                if info.user.is_empty() {
                    "signed in"
                } else {
                    &info.user
                }
            );
            if let Some(expires) = &info.expires {
                text.push_str(&format!(" · expires {expires}"));
            }
            (Role::Success, text)
        }
        Some(Check::Failed(reason)) => (Role::Warning, format!("✗ {reason}")),
        None => (Role::Muted, "· not tested".to_string()),
    }
}

fn draw_sources(frame: &mut Frame, app: &App, state: &State, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style::fg(palette, Role::Line))
        .title(Span::styled(
            " Sources ",
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = usize::from(inner.width);
    let muted = |text: &str| Line::styled(truncate(text, width), style::fg(palette, Role::Muted));

    let mut lines: Vec<Line> = Vec::new();
    lines.push(muted(if state.demo {
        "These are the demo sources (demo). Nothing here is read from or written to your config."
    } else {
        "Where your pull and merge requests come from. Changes save to your config right away."
    }));
    if let Some(why) = state.locked().filter(|_| !state.demo) {
        for line in wrap(&why, width) {
            lines.push(Line::styled(line, style::fg(palette, Role::TextSecondary)));
        }
    }
    lines.push(Line::raw(""));

    if !state.loaded {
        lines.push(muted("Reading your config…"));
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }
    if state.rows.is_empty() {
        lines.push(muted("No sources yet. Press a to connect one."));
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let cols = columns(width);
    lines.push(header(palette, &cols));
    let top = lines.len();
    let room = usize::from(inner.height)
        .saturating_sub(top + DETAILS + 1)
        .max(1);
    let first = (state.cursor + 1).saturating_sub(room);
    for (i, row) in state.rows.iter().enumerate().skip(first).take(room) {
        let y = inner.y + lines.len() as u16;
        lines.push(table_row(palette, state, row, &cols, i == state.cursor));
        hits.push(
            Rect::new(inner.x, y, inner.width, 1),
            Action::Settings(Click::Row(i)),
        );
    }
    if state.rows.len() > first + room {
        let more = state.rows.len() - first - room;
        lines.push(muted(&format!("  … {more} more, j to scroll")));
    }
    lines.push(Line::raw(""));
    if let Some(row) = state.selected() {
        lines.extend(details(palette, state, row, width));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn header(palette: &Palette, cols: &Columns) -> Line<'static> {
    let mut text = format!(
        "  {} {} {} {} {} {}",
        pad("on", 3),
        pad("", 2),
        pad("name", cols.name),
        pad("host", cols.host),
        pad("sign-in", cols.auth),
        pad("all", 3),
    );
    if let Some(scope) = cols.scope {
        text.push_str(&format!(" {}", pad("scope", scope)));
    }
    text.push_str(&format!(" {}", pad("token", cols.status)));
    Line::styled(
        text,
        style::fg(palette, Role::Muted).add_modifier(Modifier::BOLD),
    )
}

fn table_row(
    palette: &Palette,
    state: &State,
    row: &SourceRow,
    cols: &Columns,
    selected: bool,
) -> Line<'static> {
    let spec = &row.spec;
    let tag = match spec.kind {
        ForgeKind::GitHub => "GH",
        ForgeKind::GitLab => "GL",
    };
    let text_role = if row.enabled { Role::Text } else { Role::Muted };
    let (status_role, status) = check_text(state, row);
    let mut spans = vec![
        Span::styled(
            if selected { "› " } else { "  " },
            style::fg(palette, Role::Accent),
        ),
        Span::styled(
            pad(if row.enabled { "on" } else { "off" }, 3),
            style::fg(
                palette,
                if row.enabled {
                    Role::Success
                } else {
                    Role::Muted
                },
            ),
        ),
        Span::raw(" "),
        Span::styled(tag, style::fg(palette, Role::Accent)),
        Span::raw(" "),
        Span::styled(pad(&spec.name, cols.name), style::fg(palette, text_role)),
        Span::raw(" "),
        Span::styled(pad(&spec.host, cols.host), style::fg(palette, text_role)),
        Span::raw(" "),
        Span::styled(
            pad(&row.auth_label(), cols.auth),
            style::fg(palette, text_role),
        ),
        Span::raw(" "),
        Span::styled(
            pad(if row.in_all { "yes" } else { "no" }, 3),
            style::fg(palette, Role::Muted),
        ),
    ];
    if let Some(scope) = cols.scope {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            pad(&row.scope_summary(), scope),
            style::fg(palette, Role::Muted),
        ));
    }
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        pad(&status, cols.status),
        style::fg(palette, status_role),
    ));
    let mut line = Line::from(spans);
    if selected {
        line = line.style(style::bg(palette, Role::Selection));
    }
    line
}

fn details(palette: &Palette, state: &State, row: &SourceRow, width: usize) -> Vec<Line<'static>> {
    let spec = &row.spec;
    let label = |text: &str| Span::styled(pad(text, 14), style::fg(palette, Role::Muted));
    let value = |role: Role, text: String| {
        Span::styled(
            truncate(&text, width.saturating_sub(14)),
            style::fg(palette, role),
        )
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            spec.name.clone(),
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ·  {}", spec.host),
            style::fg(palette, Role::Muted),
        ),
    ])];
    let signin = match &spec.auth {
        crate::setup::AuthKind::Token => "a token in your OS keyring (never shown)".to_string(),
        crate::setup::AuthKind::Cli => format!("your {} sign-in", row.auth_label()),
        crate::setup::AuthKind::Env(var) => format!("the {var} environment variable"),
        crate::setup::AuthKind::Command(command) => format!("the output of `{command}`"),
    };
    lines.push(Line::from(vec![
        label("Signs in with"),
        value(Role::Text, signin),
    ]));
    lines.push(Line::from(vec![
        label("Covers"),
        value(Role::Text, row.scope_summary()),
    ]));
    let (role, text) = match (
        state.testing.contains(&spec.name),
        state.checks.get(&spec.name),
    ) {
        (true, _) => (Role::Muted, "checking…".to_string()),
        (_, Some(Check::Ok(info))) => {
            let mut text = format!("✓ signed in as {}", info.user);
            if !info.scopes.is_empty() {
                text.push_str(&format!(" · scopes {}", info.scopes.join(", ")));
            }
            text.push_str(&match &info.expires {
                Some(when) => format!(" · expires {when}"),
                None => " · no expiry reported".to_string(),
            });
            if let Some(note) = &info.note {
                text.push_str(&format!(" · {note}"));
            }
            (Role::Success, text)
        }
        (_, Some(Check::Failed(reason))) => (Role::Warning, format!("✗ {reason}")),
        _ => (
            Role::Muted,
            "not tested yet. Press t to check it.".to_string(),
        ),
    };
    lines.push(Line::from(vec![label("Last check"), value(role, text)]));
    let mut shown = format!(
        "{} · drafts {}",
        if row.in_all { "in All" } else { "not in All" },
        if row.include_drafts {
            "included"
        } else {
            "left out"
        }
    );
    if let Some(colour) = &row.tag_colour {
        shown.push_str(&format!(" · tag {colour}"));
    }
    lines.push(Line::from(vec![label("Queue"), value(Role::Text, shown)]));
    if let Some(url) = &spec.api_url {
        lines.push(Line::from(vec![
            label("API"),
            value(Role::Text, url.clone()),
        ]));
    }
    if let Some(origin) = &state.origin {
        lines.push(Line::from(vec![
            label("Defined"),
            value(
                Role::TextSecondary,
                format!("{} (read-only here)", origin.label),
            ),
        ]));
    }
    lines
}

/// The footer while a dialog is open.
pub fn modal_hints(state: &State) -> Option<Vec<Hint>> {
    let hint = |key, label| Hint {
        key,
        label,
        action: None,
    };
    Some(match state.modal.as_ref()? {
        Modal::Pick(_) => vec![
            hint("⏎", "choose"),
            hint("1-9", "pick"),
            hint("esc", "cancel"),
        ],
        Modal::Form(_) => vec![
            hint("tab", "next field"),
            hint("⏎", "save"),
            hint("←/→", "change choice"),
            hint("esc", "cancel"),
        ],
        Modal::Remove(_) => vec![
            hint("⏎", "choose"),
            hint("y", "remove"),
            hint("esc", "no, keep it"),
        ],
    })
}

fn draw_modal(
    frame: &mut Frame,
    app: &App,
    state: &State,
    modal: &Modal,
    body: Rect,
    hits: &mut HitMap,
) {
    let palette = &app.palette;
    let width = body.width.saturating_sub(4).min(76);
    let inner_w = usize::from(width.saturating_sub(4));
    let mut dialog = Dialog::new(palette, inner_w);
    let title = match modal {
        Modal::Pick(pick) => {
            pick_lines(&mut dialog, pick);
            " Add a source "
        }
        Modal::Form(form) => {
            form_lines(&mut dialog, form);
            if form.is_edit() {
                " Edit source "
            } else {
                " Add a source "
            }
        }
        Modal::Remove(remove) => {
            remove_lines(&mut dialog, state, remove);
            " Remove source? "
        }
    };
    let height = (dialog.lines.len() as u16 + 2).min(body.height);
    let rect = Rect::new(
        body.x + body.width.saturating_sub(width) / 2,
        body.y + body.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style::fg(palette, Role::Accent))
        .title(Line::styled(
            title,
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1))
        .style(style::bg(palette, Role::Raised));
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(dialog.lines).block(block), rect);
    for (line, click) in dialog.rows {
        let y = rect.y + 1 + line as u16;
        if y < rect.bottom().saturating_sub(1) {
            hits.push(
                Rect::new(rect.x + 1, y, rect.width.saturating_sub(2), 1),
                Action::Settings(click),
            );
        }
    }
}

struct Dialog<'a> {
    palette: &'a Palette,
    width: usize,
    lines: Vec<Line<'static>>,
    rows: Vec<(usize, Click)>,
}

impl<'a> Dialog<'a> {
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

    fn row(&mut self, line: Line<'static>, click: Click) {
        self.rows.push((self.lines.len(), click));
        self.lines.push(line);
    }

    fn choice(&mut self, label: &str, focused: bool, click: Click) {
        let span = if focused {
            Span::styled(
                format!("› {label} ‹"),
                style::fg(self.palette, Role::Accent)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
        } else {
            Span::styled(format!("  {label}  "), style::fg(self.palette, Role::Muted))
        };
        self.row(Line::from(span), click);
    }
}

fn pick_lines(d: &mut Dialog, pick: &Pick) {
    d.text(
        Role::Muted,
        if pick.looking {
            "Looking for accounts you already use…"
        } else if pick.hosts.is_empty() {
            "Nothing new turned up on this machine. You can still add a host by hand."
        } else {
            "Found on this machine. Pick one, or add another host."
        },
    );
    d.blank();
    for (i, found) in pick.hosts.iter().enumerate() {
        let tag = match found.kind {
            ForgeKind::GitHub => "GH",
            ForgeKind::GitLab => "GL",
        };
        let label = format!("{}  {tag}  {}  ·  {}", i + 1, found.host, found.found_in());
        d.choice(
            &truncate(&label, d.width.saturating_sub(4)),
            i == pick.cursor,
            Click::Choose(i),
        );
    }
    let n = pick.hosts.len();
    d.choice(
        &format!("{}  Another host…", n + 1),
        pick.cursor == n,
        Click::Choose(n),
    );
}

fn auth_label(choice: AuthChoice, kind: ForgeKind) -> &'static str {
    match (choice, kind) {
        (AuthChoice::Cli, ForgeKind::GitHub) => "gh sign-in",
        (AuthChoice::Cli, ForgeKind::GitLab) => "glab sign-in",
        (AuthChoice::Token, _) => "token in the keyring",
        (AuthChoice::Env, _) => "environment variable",
        (AuthChoice::Command, _) => "command that prints a token",
    }
}

fn form_lines(d: &mut Dialog, form: &Form) {
    let palette = d.palette;
    let label_w = 14;
    let value_w = d.width.saturating_sub(label_w + 3);
    let owners = match form.kind {
        ForgeKind::GitHub => "Organisations",
        ForgeKind::GitLab => "Groups",
    };
    for (i, field) in form.visible().into_iter().enumerate() {
        let focused = field == form.focus;
        let (label, mut spans): (&str, Vec<Span<'static>>) = match field {
            Field::Name => (
                "Name",
                editor_spans(palette, &form.name, value_w, focused, false),
            ),
            Field::Host => (
                "Host",
                editor_spans(palette, &form.host, value_w, focused, false),
            ),
            Field::ApiUrl => ("API address", {
                if form.api_url.is_blank() && !focused {
                    vec![Span::styled(
                        "(default for the host)",
                        style::fg(palette, Role::Muted),
                    )]
                } else {
                    editor_spans(palette, &form.api_url, value_w, focused, false)
                }
            }),
            Field::Detail => (
                if form.auth == AuthChoice::Env {
                    "Variable"
                } else {
                    "Command"
                },
                editor_spans(palette, &form.detail, value_w, focused, false),
            ),
            Field::Owners => (owners, {
                if form.owners.is_blank() && !focused {
                    vec![Span::styled(
                        "(everything you can see)",
                        style::fg(palette, Role::Muted),
                    )]
                } else {
                    editor_spans(palette, &form.owners, value_w, focused, false)
                }
            }),
            Field::Kind => (
                "Forge",
                vec![chooser(
                    palette,
                    match form.kind {
                        ForgeKind::GitHub => "GitHub",
                        ForgeKind::GitLab => "GitLab",
                    },
                    focused,
                )],
            ),
            Field::Auth => (
                "Sign in with",
                vec![chooser(palette, auth_label(form.auth, form.kind), focused)],
            ),
            Field::User => (
                "Your own repos",
                vec![chooser(
                    palette,
                    if form.user {
                        "included"
                    } else {
                        "not added on their own"
                    },
                    focused,
                )],
            ),
            Field::Token => ("Token", {
                if form.token.0.is_blank() && !focused {
                    let hint = if form.is_edit() {
                        "(leave blank to keep the saved token)"
                    } else {
                        "(paste one; it goes in your OS keyring)"
                    };
                    vec![Span::styled(hint, style::fg(palette, Role::Muted))]
                } else {
                    editor_spans(palette, &form.token.0, value_w, focused, true)
                }
            }),
        };
        let mut line = vec![
            Span::styled(
                if focused { "› " } else { "  " },
                style::fg(palette, Role::Accent),
            ),
            Span::styled(pad(label, label_w), style::fg(palette, Role::Muted)),
        ];
        line.append(&mut spans);
        d.row(Line::from(line), Click::Field(i));
    }
    d.blank();
    if let Some(error) = &form.error {
        d.text(Role::Warning, error);
    } else if form.saving {
        d.text(Role::Muted, "Checking and saving…");
    } else if let Mode::Edit(_) = form.mode {
        d.text(
            Role::Muted,
            "Only what you change is written; the rest of the file stays as it is.",
        );
    } else {
        d.text(
            Role::Muted,
            "Saved to your config. The token itself never goes in the file.",
        );
    }
    d.blank();
    let save = if form.saving { "Saving…" } else { "Save ⏎" };
    d.row(
        Line::from(vec![
            Span::styled(format!("[ {save} ]"), style::fg(palette, Role::Accent)),
            Span::raw("  "),
            Span::styled("[ Cancel · esc ]", style::fg(palette, Role::Muted)),
        ]),
        Click::Save,
    );
}

fn chooser(palette: &Palette, text: &str, focused: bool) -> Span<'static> {
    let shown = if focused {
        format!("◂ {text} ▸")
    } else {
        text.to_string()
    };
    Span::styled(shown, style::fg(palette, Role::Text))
}

/// A single-line editor's text, with a block caret when it has focus. A hidden one shows a dot
/// per character and never the text.
fn editor_spans(
    palette: &Palette,
    editor: &crate::app::editor::Editor,
    width: usize,
    focused: bool,
    hidden: bool,
) -> Vec<Span<'static>> {
    let text = style::fg(palette, Role::Text);
    let chars: Vec<char> = editor
        .text()
        .chars()
        .map(|c| if hidden { '•' } else { c })
        .collect();
    let (_, col) = editor.cursor();
    let room = width.saturating_sub(1).max(1);
    let across = if focused { col.saturating_sub(room) } else { 0 };
    let shown: Vec<char> = chars.iter().skip(across).take(room + 1).copied().collect();
    if !focused {
        let line: String = shown.iter().take(room).collect();
        return vec![Span::styled(line, text)];
    }
    let at = col - across;
    let before: String = shown.iter().take(at).collect();
    let under = shown.get(at).copied();
    let after: String = shown
        .iter()
        .skip(at + 1)
        .take(room.saturating_sub(at))
        .collect();
    vec![
        Span::styled(before, text),
        Span::styled(
            under.map_or(" ".to_string(), String::from),
            Style::default().add_modifier(Modifier::REVERSED),
        ),
        Span::styled(after, text),
    ]
}

fn remove_lines(d: &mut Dialog, state: &State, remove: &Remove) {
    let spec = &remove.spec;
    d.text(
        Role::TextBright,
        &format!("Remove {} ({})?", spec.name, spec.host),
    );
    d.blank();
    d.text(
        Role::Text,
        &format!(
            "This takes the {} source out of {}. Its pull and merge requests leave your queue. Nothing changes on {}.",
            spec.name,
            state.write_target.display(),
            spec.host
        ),
    );
    if spec.auth == crate::setup::AuthKind::Token {
        d.blank();
        let text = if remove.shares_token {
            format!(
                "The token in your keyring stays: another source uses review-buddy/{} too.",
                spec.host
            )
        } else {
            format!(
                "The token in your keyring (review-buddy/{}) stays unless you remove it too.",
                spec.host
            )
        };
        d.text(Role::Muted, &text);
    }
    d.blank();
    for (i, choice) in remove.choices().into_iter().enumerate() {
        let label = match choice {
            RemoveChoice::Keep => "No, keep it  ⏎",
            RemoveChoice::Remove => "Remove the source  y",
            RemoveChoice::RemoveAndToken => "Remove the source and its keyring token  d",
        };
        d.choice(label, i == remove.choice, Click::Choose(i));
    }
}
