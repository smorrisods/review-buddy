//! `review-buddy theme …`

use rb_theme::{Appearance, Theme, BUILTIN_IDS};
use serde_json::{json, Value};

use super::context::Context;
use super::error::CmdError;
use super::output::{self, json, Cell, Column, Table};

const FIELDS: &[&str] = &["id", "name", "appearance", "builtin", "current"];

struct Entry {
    id: String,
    name: String,
    appearance: &'static str,
    current: bool,
}

fn entries(current: &str) -> Vec<Entry> {
    BUILTIN_IDS
        .iter()
        .filter_map(|id| Theme::builtin(id).ok())
        .map(|theme| Entry {
            current: theme.id == current,
            appearance: match theme.appearance {
                Appearance::Dark => "dark",
                Appearance::Light => "light",
            },
            id: theme.id,
            name: theme.name,
        })
        .collect()
}

fn to_json(entries: &[Entry]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "name": e.name,
                    "appearance": e.appearance,
                    "builtin": true,
                    "current": e.current,
                })
            })
            .collect(),
    )
}

fn to_table(entries: &[Entry]) -> Table {
    let mut table = Table::new(vec![
        Column::fixed("Id"),
        Column::flex("Name"),
        Column::fixed("Appearance"),
        Column::fixed("Current"),
    ]);
    for e in entries {
        table.push(vec![
            Cell::styled(&e.id, rb_theme::Role::Accent),
            Cell::plain(&e.name),
            Cell::plain(e.appearance),
            Cell::plain(if e.current { "current" } else { "" }),
        ]);
    }
    table
}

/// The built-in themes, with the configured one marked.
pub fn list(ctx: &Context) -> Result<(), CmdError> {
    let entries = entries(&ctx.config.ui.theme);
    if let Some(text) = json::render(&ctx.args, FIELDS, &to_json(&entries), ctx.out.tty) {
        return output::print(&text?);
    }
    output::print(&ctx.out.table(&to_table(&entries)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_every_builtin_and_marks_the_current_one() {
        let list = entries("dusk");
        assert_eq!(list.len(), BUILTIN_IDS.len());
        assert_eq!(list.iter().filter(|e| e.current).count(), 1);
        assert!(list.iter().find(|e| e.id == "dusk").unwrap().current);
        assert_eq!(
            list.iter()
                .find(|e| e.id == "afterglow-light")
                .unwrap()
                .appearance,
            "light"
        );
    }

    #[test]
    fn json_matches_the_documented_fields() {
        let value = to_json(&entries("liminal-hq"));
        let first = value[0].as_object().unwrap();
        let keys: Vec<&str> = first.keys().map(String::as_str).collect();
        assert_eq!(keys, FIELDS);
    }

    #[test]
    fn piped_rows_are_tab_separated_with_no_header() {
        let text = super::super::output::render_tsv(&to_table(&entries("liminal-hq")));
        assert!(text.starts_with("liminal-hq\t"));
        assert!(text.lines().all(|l| l.split('\t').count() == 4));
    }
}
