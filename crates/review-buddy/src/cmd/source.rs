//! `review-buddy source …`

use serde_json::{json, Value};

use super::auth::kind_name;
use super::context::Context;
use super::error::CmdError;
use super::output::{self, json, Cell, Column, Table};
use super::DEMO_LABEL;
use crate::config::{AuthSetting, Kind, SourceConfig};

const FIELDS: &[&str] = &[
    "name", "kind", "host", "enabled", "inAll", "auth", "scope", "apiUrl",
];

struct Row {
    name: String,
    kind: &'static str,
    host: String,
    enabled: bool,
    in_all: bool,
    auth: String,
    scope: String,
    api_url: Option<String>,
}

fn list_or_all(label: &str, items: &[String]) -> Option<String> {
    (!items.is_empty()).then(|| format!("{label} {}", items.join(", ")))
}

fn scope_text(cfg: &SourceConfig) -> String {
    let s = &cfg.scope;
    let (owners, repos) = match cfg.kind {
        Kind::Github => (list_or_all("orgs", &s.orgs), list_or_all("repos", &s.repos)),
        Kind::Gitlab => (
            list_or_all("groups", &s.groups),
            list_or_all("projects", &s.projects),
        ),
    };
    let parts: Vec<String> = [owners, repos, s.user.then(|| "your account".to_string())]
        .into_iter()
        .flatten()
        .collect();
    if parts.is_empty() {
        "everything you can see".into()
    } else {
        parts.join("; ")
    }
}

fn row(cfg: &SourceConfig) -> Row {
    Row {
        name: cfg.name.clone(),
        kind: match cfg.kind {
            Kind::Github => "github",
            Kind::Gitlab => "gitlab",
        },
        host: cfg.host.clone(),
        enabled: cfg.enabled,
        in_all: cfg.in_all,
        auth: cfg
            .auth
            .as_ref()
            .map_or_else(|| "cli".to_string(), AuthSetting::to_string),
        scope: scope_text(cfg),
        api_url: cfg.api_url.clone(),
    }
}

fn demo_rows(ctx: &Context) -> Result<Vec<Row>, CmdError> {
    Ok(ctx
        .sources()?
        .into_iter()
        .map(|s| Row {
            name: s.label,
            kind: kind_name(s.kind),
            host: s.host,
            enabled: true,
            in_all: s.in_all,
            auth: "demo".into(),
            scope: "demo fixtures".into(),
            api_url: None,
        })
        .collect())
}

fn rows(ctx: &Context) -> Result<Vec<Row>, CmdError> {
    if ctx.is_demo() {
        return demo_rows(ctx);
    }
    let all = ctx.configured()?;
    if ctx.args.sources.is_empty() {
        return Ok(all.iter().map(row).collect());
    }
    let named = ctx.sources()?;
    Ok(all
        .iter()
        .filter(|s| named.iter().any(|n| n.id.as_str() == s.name))
        .map(row)
        .collect())
}

fn rows_json(rows: &[Row]) -> Value {
    Value::Array(
        rows.iter()
            .map(|r| {
                json!({
                    "name": r.name,
                    "kind": r.kind,
                    "host": r.host,
                    "enabled": r.enabled,
                    "inAll": r.in_all,
                    "auth": r.auth,
                    "scope": r.scope,
                    "apiUrl": r.api_url,
                })
            })
            .collect(),
    )
}

fn yes_no(flag: bool) -> &'static str {
    if flag {
        "yes"
    } else {
        "no"
    }
}

fn table(rows: &[Row]) -> Table {
    let mut table = Table::new(vec![
        Column::fixed("Name"),
        Column::fixed("Kind"),
        Column::fixed("Host"),
        Column::fixed("Enabled"),
        Column::fixed("In all"),
        Column::fixed("Auth"),
        Column::flex("Scope"),
    ]);
    for r in rows {
        table.push(vec![
            Cell::plain(&r.name),
            Cell::plain(r.kind),
            Cell::plain(&r.host),
            Cell::plain(yes_no(r.enabled)),
            Cell::plain(yes_no(r.in_all)),
            Cell::plain(&r.auth),
            Cell::plain(&r.scope),
        ]);
    }
    table
}

/// Configured sources, read from config only. Nothing here touches the network or a keyring.
pub fn list(ctx: &Context) -> Result<(), CmdError> {
    let rows = rows(ctx)?;
    if let Some(text) = json::render(&ctx.args, FIELDS, &rows_json(&rows), ctx.out.tty) {
        return output::print(&text?);
    }
    if rows.is_empty() {
        return output::print(
            "No sources are configured yet.\nRun review-buddy --setup or review-buddy source add, or try --demo.\n",
        );
    }
    let mut text = ctx.out.table(&table(&rows));
    if ctx.is_demo() && ctx.out.tty {
        text.push_str(&format!("\nThese are demo sources {DEMO_LABEL}\n"));
    }
    output::print(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> SourceConfig {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn rows_describe_auth_and_scope() {
        let r = row(&cfg(
            "name = \"work\"\nkind = \"github\"\nhost = \"github.com\"\nauth = \"env:GH_T\"\nin_all = false\nenabled = false\n[scope]\norgs = [\"liminal-hq\"]\nuser = true\n",
        ));
        assert_eq!(r.auth, "env:GH_T");
        assert_eq!(r.scope, "orgs liminal-hq; your account");
        assert!(!r.enabled && !r.in_all);
    }

    #[test]
    fn defaults_read_as_cli_and_everything() {
        let r = row(&cfg(
            "name = \"a\"\nkind = \"gitlab\"\nhost = \"gitlab.com\"\n",
        ));
        assert_eq!(
            (r.auth.as_str(), r.scope.as_str()),
            ("cli", "everything you can see")
        );
        assert!(r.enabled && r.in_all);
    }

    #[test]
    fn tsv_has_one_headerless_row_per_source() {
        let rows = vec![row(&cfg(
            "name = \"a\"\nkind = \"github\"\nhost = \"github.com\"\n",
        ))];
        let text = output::render_tsv(&table(&rows));
        assert_eq!(text.lines().count(), 1);
        assert!(text.lines().all(|l| l.split('\t').count() == 7));
    }
}
