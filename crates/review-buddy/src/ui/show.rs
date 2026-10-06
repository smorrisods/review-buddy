//! The Show filters overlay: the five kind checkboxes, then the projects the queue draws from
//! (searchable and scrolling), what the filters hide, and the keys.

use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use rb_theme::Role;

use super::{chrome::truncate, layout, style, text::cells, text::justify, HitMap};
use crate::app::{projects, queue, show, Action, App};

const MAX_WIDTH: u16 = 64;

pub fn draw(frame: &mut Frame, app: &App, area: Rect, hits: &mut HitMap) {
    let palette = &app.palette;
    let width = MAX_WIDTH.min(area.width.saturating_sub(2));
    let metrics = show::metrics(app);
    let height = metrics.height.min(area.height);
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style::fg(palette, Role::Accent))
        .title(Line::styled(
            " Show ",
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD),
        ))
        .style(style::bg(palette, Role::Raised));
    let inner = layout::inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);

    let name_w = show::FILTERS
        .iter()
        .map(|f| cells(f.as_str()))
        .max()
        .unwrap_or(0);
    let mut lines = vec![Line::raw("")];
    for (n, filter) in show::FILTERS.into_iter().enumerate() {
        let on = show::is_on(app, filter);
        let at = app.show.cursor == n;
        let name = format!("{:<name_w$}", filter.as_str());
        let name_style = if at {
            style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD)
        } else {
            style::fg(palette, if on { Role::Text } else { Role::Muted })
        };
        let line = Line::from(vec![
            Span::styled(
                if at { "▌ " } else { "  " },
                style::fg(palette, Role::Accent),
            ),
            Span::styled(
                format!("[{}] ", if on { "x" } else { " " }),
                style::fg(palette, Role::Accent),
            ),
            Span::styled(name, name_style),
            Span::styled(
                format!("  {}", show::describe(filter)),
                style::fg(palette, Role::Muted),
            ),
        ]);
        let y = inner.y + 1 + n as u16;
        if y < inner.bottom() {
            hits.push(
                Rect::new(inner.x, y, inner.width, 1),
                Action::ToggleShow(filter),
            );
        }
        lines.push(if at {
            line.style(style::bg(palette, Role::Selection))
        } else {
            line
        });
    }

    lines.push(Line::raw(""));
    let groups = projects::catalog(&app.state);
    let (shown, total) = projects::tally(&app.state, &groups);
    let heading = if total == 0 {
        "Projects".to_string()
    } else {
        format!("Projects  {shown} of {total} projects shown")
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            heading,
            style::fg(palette, Role::TextSecondary).add_modifier(Modifier::BOLD),
        ),
    ]));
    let search_y = inner.y + lines.len() as u16;
    lines.push(search_line(app, inner.width as usize));
    if search_y < inner.bottom() {
        hits.push(
            Rect::new(inner.x, search_y, inner.width, 1),
            Action::FocusProjectSearch,
        );
    }
    let list_top = inner.y + lines.len() as u16;
    lines.extend(project_lines(app, metrics.list, inner, list_top, hits));

    let hidden = queue::hidden_split(&app.state, app.active_source().map(|s| &s.id));
    let note = hidden
        .note()
        .unwrap_or_else(|| "Nothing is hidden.".to_string());
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(note, style::fg(palette, Role::Muted)),
    ]));
    lines.push(hint_line(
        palette,
        &[
            ("space", "toggle"),
            ("/", "find"),
            ("a", "all"),
            ("n", "none"),
        ],
        show::can_save(app).then_some(("w", "save")),
        ("esc", "close"),
    ));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            if show::can_save(app) {
                "Changes last for this session only, unless you save."
            } else {
                "Changes last for this session only."
            },
            style::fg(palette, Role::Muted),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(lines).style(style::fg(palette, Role::Text)),
        inner,
    );
}

fn hint_line(
    palette: &rb_theme::Palette,
    keys: &[(&str, &str)],
    save: Option<(&str, &str)>,
    close: (&str, &str),
) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    let all = keys.iter().copied().chain(save).chain([close]);
    for (n, (key, what)) in all.enumerate() {
        if n > 0 {
            spans.push(Span::styled(" · ", style::fg(palette, Role::Muted)));
        }
        spans.push(Span::styled(
            key.to_string(),
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {what}"),
            style::fg(palette, Role::Muted),
        ));
    }
    Line::from(spans)
}

fn search_line(app: &App, width: usize) -> Line<'static> {
    let palette = &app.palette;
    let control = &app.show;
    let mut spans = vec![
        Span::raw("  "),
        Span::styled(
            "/ ",
            style::fg(palette, Role::Accent).add_modifier(Modifier::BOLD),
        ),
    ];
    if control.searching || !control.search.is_empty() {
        let mut text = control.search.clone();
        if control.searching {
            text.push('▏');
        }
        spans.push(Span::styled(
            truncate(&text, width.saturating_sub(24)),
            style::fg(palette, Role::TextBright),
        ));
        let n = show::matches(app).len();
        let found = format!("{n} {}", if n == 1 { "match" } else { "matches" });
        return justify(
            spans,
            vec![
                Span::styled(found, style::fg(palette, Role::Muted)),
                Span::raw("  "),
            ],
            width,
        );
    }
    spans.push(Span::styled(
        "type to find a project",
        style::fg(palette, Role::Muted),
    ));
    Line::from(spans)
}

fn project_lines(
    app: &App,
    list: usize,
    inner: Rect,
    top: u16,
    hits: &mut HitMap,
) -> Vec<Line<'static>> {
    let palette = &app.palette;
    let rows = show::list_rows(app, &app.show.search);
    let mut out: Vec<Line<'static>> = Vec::new();
    if rows.is_empty() {
        let text = if app.show.search.trim().is_empty() {
            "No projects yet. They appear once changes load."
        } else {
            "No project matches. Esc clears the search."
        };
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(text, style::fg(palette, Role::Muted)),
        ]));
    }
    let cursor_project = app.show.cursor.checked_sub(show::FILTERS.len());
    let before = rows
        .iter()
        .take(app.show.scroll)
        .filter(|r| matches!(r, show::ListRow::Project { .. }))
        .count();
    let inner_w = usize::from(inner.width);
    for (offset, row) in rows.iter().skip(app.show.scroll).take(list).enumerate() {
        let y = top + offset as u16;
        match row {
            show::ListRow::Heading {
                label,
                projects,
                changes,
            } => {
                let counts = format!(
                    "{projects} {} · {changes} {}",
                    if *projects == 1 {
                        "project"
                    } else {
                        "projects"
                    },
                    if *changes == 1 { "change" } else { "changes" },
                );
                out.push(justify(
                    vec![
                        Span::raw("  "),
                        Span::styled(
                            truncate(label, inner_w.saturating_sub(counts.len() + 6)),
                            style::fg(palette, Role::TextSecondary).add_modifier(Modifier::BOLD),
                        ),
                    ],
                    vec![
                        Span::styled(counts, style::fg(palette, Role::Muted)),
                        Span::raw("  "),
                    ],
                    inner_w,
                ));
            }
            show::ListRow::Project {
                source,
                repo,
                count,
            } => {
                let at = before + out_project_index(&rows, app.show.scroll, offset);
                let selected = cursor_project == Some(at);
                let on = !app.state.queue_settings.projects.is_hidden(source, repo);
                let name_style = if selected {
                    style::fg(palette, Role::TextBright).add_modifier(Modifier::BOLD)
                } else {
                    style::fg(palette, if on { Role::Text } else { Role::Muted })
                };
                let count = count.to_string();
                let room = inner_w.saturating_sub(count.len() + 10);
                let line = justify(
                    vec![
                        Span::styled(
                            if selected { "▌ " } else { "  " },
                            style::fg(palette, Role::Accent),
                        ),
                        Span::styled(
                            format!("[{}] ", if on { "x" } else { " " }),
                            style::fg(palette, Role::Accent),
                        ),
                        Span::styled(truncate(repo, room), name_style),
                    ],
                    vec![
                        Span::styled(count, style::fg(palette, Role::Muted)),
                        Span::raw("  "),
                    ],
                    inner_w,
                );
                if y < inner.bottom() {
                    hits.push(
                        Rect::new(inner.x, y, inner.width, 1),
                        Action::ToggleProject(source.clone(), repo.clone()),
                    );
                }
                out.push(if selected {
                    line.style(style::bg(palette, Role::Selection))
                } else {
                    line
                });
            }
        }
    }
    while out.len() < list {
        out.push(Line::raw(""));
    }
    out
}

/// How many project rows sit between the first line on screen and `offset` lines below it.
fn out_project_index(rows: &[show::ListRow], scroll: usize, offset: usize) -> usize {
    rows.iter()
        .skip(scroll)
        .take(offset)
        .filter(|r| matches!(r, show::ListRow::Project { .. }))
        .count()
}
