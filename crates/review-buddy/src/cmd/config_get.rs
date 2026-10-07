//! `review-buddy config get <key>` and `config list`: the effective value of each setting and
//! the layer it came from.
//!
//! Keys are dotted, like `ui.theme`. The origin is the last config file that sets the key (the
//! files merge in order, later ones winning), an environment override, or `default`.

use std::path::PathBuf;
use std::time::Duration;

use rb_paths::Env;
use serde_json::{json, Value};

use super::context::Context;
use super::error::CmdError;
use super::output::{self, json, Cell, Column, Table};
use super::DEMO_LABEL;
use crate::config::{AuthSetting, Config};

const FIELDS: &[&str] = &["key", "value", "origin"];

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub value: Value,
    pub origin: String,
}

fn duration_text(d: Duration) -> String {
    let secs = d.as_secs();
    for (unit, size) in [("w", 604_800), ("d", 86_400), ("h", 3_600), ("m", 60)] {
        if secs >= size && secs.is_multiple_of(size) {
            return format!("{}{unit}", secs / size);
        }
    }
    format!("{secs}s")
}

fn strings(items: &[String]) -> Value {
    json!(items)
}

/// Every setting in a stable order: the sections as in `config.example.toml`, then key
/// overrides by name, then each source's basics.
pub fn values(config: &Config) -> Vec<(String, Value)> {
    let (ui, review, diff) = (&config.ui, &config.review, &config.diff);
    let (refresh, triage, checkout) = (&config.refresh, &config.triage, &config.checkout);
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut put = |key: &str, value: Value| out.push((key.to_string(), value));
    put("ui.theme", json!(ui.theme));
    put("ui.layout", json!(ui.layout.as_str()));
    put("ui.sources", json!(ui.sources.as_str()));
    put("ui.detail", json!(ui.detail.as_str()));
    put("ui.detail_position", json!(ui.detail_position.as_str()));
    put("ui.jax", json!(ui.jax));
    put("ui.reduced_motion", json!(ui.reduced_motion));
    put("ui.unicode", json!(ui.unicode));
    put("ui.colour_depth", json!(ui.colour_depth.as_str()));
    put("ui.background", json!(ui.background.as_str()));
    for (theme, mode) in &ui.theme_background {
        put(
            &format!("ui.theme_background.{theme}"),
            json!(mode.as_str()),
        );
    }
    put("ui.mouse", json!(ui.mouse));
    put("ui.date_locale", json!(ui.date_locale));
    put("review.merge_method", json!(review.merge_method.as_str()));
    put("review.confirm_merge", json!(review.confirm_merge));
    put("review.confirm_post_now", json!(review.confirm_post_now));
    put(
        "review.delete_branch_on_merge",
        json!(review.delete_branch_on_merge),
    );
    put(
        "review.mark_viewed_on_open",
        json!(review.mark_viewed_on_open),
    );
    put(
        "review.request_changes_needs_summary",
        json!(review.request_changes_needs_summary),
    );
    put("diff.view", json!(diff.view.as_str()));
    put("diff.auto_side_by_side", json!(diff.auto_side_by_side));
    put(
        "diff.side_by_side_min_cols",
        json!(diff.side_by_side_min_cols),
    );
    put("diff.context_lines", json!(diff.context_lines));
    put("diff.ignore_whitespace", json!(diff.ignore_whitespace));
    put("diff.syntax_highlight", json!(diff.syntax_highlight));
    put("diff.tab_width", json!(diff.tab_width));
    put(
        "refresh.interval",
        json!(refresh
            .interval
            .map_or_else(|| "off".to_string(), duration_text)),
    );
    put("refresh.on_focus", json!(refresh.on_focus));
    put(
        "refresh.max_concurrency_per_host",
        json!(refresh.max_concurrency_per_host),
    );
    put("triage.noise_authors", strings(&triage.noise_authors));
    put("triage.bucket_limit", json!(triage.bucket_limit));
    let show: Vec<String> = triage.show.iter().map(|s| s.as_str().to_string()).collect();
    put("triage.show", strings(&show));
    put("triage.noise_collapsed", json!(triage.noise_collapsed));
    put(
        "triage.stale_after",
        json!(duration_text(triage.stale_after)),
    );
    put("checkout.root", json!(checkout.root));
    put(
        "checkout.use_worktree_if_dirty",
        json!(checkout.use_worktree_if_dirty),
    );
    put(
        "checkout.clone_if_missing",
        json!(checkout.clone_if_missing.as_str()),
    );
    for (name, chord) in &config.keys {
        put(&format!("keys.{name}"), json!(chord));
    }
    for s in &config.sources {
        let base = format!("source.{}", s.name);
        let kind = match s.kind {
            crate::config::Kind::Github => "github",
            crate::config::Kind::Gitlab => "gitlab",
        };
        put(&format!("{base}.kind"), json!(kind));
        put(&format!("{base}.host"), json!(s.host));
        put(&format!("{base}.enabled"), json!(s.enabled));
        put(&format!("{base}.in_all"), json!(s.in_all));
        put(
            &format!("{base}.auth"),
            json!(s
                .auth
                .as_ref()
                .map_or_else(|| "cli".to_string(), AuthSetting::to_string)),
        );
        if let Some(url) = &s.api_url {
            put(&format!("{base}.api_url"), json!(url));
        }
    }
    out
}

/// Where a key's value was set. `files` are the merged config files in merge order.
fn origin_of(key: &str, files: &[(PathBuf, toml::Table)], env: &dyn Env) -> String {
    let overridden = match key {
        "ui.theme" => env
            .var("REVIEW_BUDDY_THEME")
            .filter(|v| !v.is_empty())
            .map(|_| "$REVIEW_BUDDY_THEME"),
        "ui.background" => env
            .var("REVIEW_BUDDY_BACKGROUND")
            .filter(|v| v.parse::<rb_theme::BackgroundMode>().is_ok())
            .map(|_| "$REVIEW_BUDDY_BACKGROUND"),
        "ui.detail_position" => env
            .var("REVIEW_BUDDY_DETAIL_POSITION")
            .filter(|v| crate::config::parse_position(v).is_some())
            .map(|_| "$REVIEW_BUDDY_DETAIL_POSITION"),
        "ui.reduced_motion" => env
            .var("REVIEW_BUDDY_REDUCED_MOTION")
            .filter(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .map(|_| "$REVIEW_BUDDY_REDUCED_MOTION"),
        _ => None,
    };
    if let Some(var) = overridden {
        return var.to_string();
    }
    let parts: Vec<&str> = key.split('.').collect();
    let found = files
        .iter()
        .rev()
        .find(|(_, table)| match parts.as_slice() {
            ["source", name, _] => table
                .get("source")
                .and_then(toml::Value::as_array)
                .is_some_and(|list| {
                    list.iter()
                        .any(|s| s.get("name").and_then(toml::Value::as_str) == Some(name))
                }),
            parts => lookup(table, parts),
        });
    found.map_or_else(
        || "default".to_string(),
        |(path, _)| path.display().to_string(),
    )
}

fn lookup(table: &toml::Table, parts: &[&str]) -> bool {
    let Some((first, rest)) = parts.split_first() else {
        return false;
    };
    match (table.get(*first), rest.is_empty()) {
        (Some(_), true) => true,
        (Some(toml::Value::Table(inner)), false) => lookup(inner, rest),
        _ => false,
    }
}

fn read_layers(ctx: &Context) -> Vec<(PathBuf, toml::Table)> {
    ctx.paths
        .config_files
        .iter()
        .filter(|f| f.exists)
        .filter_map(|f| {
            let text = std::fs::read_to_string(&f.path).ok()?;
            Some((f.path.clone(), toml::from_str(&text).ok()?))
        })
        .collect()
}

pub fn entries(ctx: &Context) -> Result<Vec<Entry>, CmdError> {
    if let Some(problem) = ctx.config_problem() {
        return Err(CmdError::failed(format!(
            "{problem}\nFix the config file, or see review-buddy config paths."
        )));
    }
    let layers = if ctx.is_demo() {
        Vec::new()
    } else {
        read_layers(ctx)
    };
    Ok(values(&ctx.config)
        .into_iter()
        .map(|(key, value)| Entry {
            origin: origin_of(&key, &layers, ctx.env.as_ref()),
            key,
            value,
        })
        .collect())
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// The known keys closest to `key`, best first.
pub fn suggestions(key: &str, known: &[String]) -> Vec<String> {
    let mut scored: Vec<(usize, &String)> = known
        .iter()
        .map(|k| {
            let near = distance(key, k);
            let shared = k.starts_with(key) || key.starts_with(k.as_str());
            (if shared { 0 } else { near }, k)
        })
        .filter(|(d, _)| *d <= 3)
        .collect();
    scored.sort();
    scored.into_iter().take(3).map(|(_, k)| k.clone()).collect()
}

fn unknown_key(key: &str, all: &[Entry]) -> CmdError {
    let known: Vec<String> = all.iter().map(|e| e.key.clone()).collect();
    let close = suggestions(key, &known);
    let hint = if close.is_empty() {
        "Run review-buddy config list to see every key.".to_string()
    } else {
        format!("Did you mean {}?", close.join(", "))
    };
    CmdError::usage(format!("There's no setting called {key}.\n{hint}"))
}

fn entries_json(entries: &[Entry]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|e| json!({"key": e.key, "value": e.value, "origin": e.origin}))
            .collect(),
    )
}

pub fn get(ctx: &Context, key: &str) -> Result<(), CmdError> {
    let all = entries(ctx)?;
    let entry = all
        .iter()
        .find(|e| e.key == key)
        .ok_or_else(|| unknown_key(key, &all))?;
    let value = entries_json(std::slice::from_ref(entry));
    if let Some(text) = json::render(&ctx.args, FIELDS, &value, ctx.out.tty) {
        let text = text?;
        return output::print(&text);
    }
    let mut text = value_text(&entry.value);
    if ctx.out.tty {
        text.push_str(&format!("  (from {})", entry.origin));
        if ctx.is_demo() {
            text.push_str(&format!(" {DEMO_LABEL}"));
        }
    }
    text.push('\n');
    output::print(&text)
}

fn table(entries: &[Entry]) -> Table {
    let mut table = Table::new(vec![
        Column::fixed("Key"),
        Column::fixed("Value"),
        Column::flex("From"),
    ]);
    for e in entries {
        table.push(vec![
            Cell::plain(&e.key),
            Cell::plain(value_text(&e.value)),
            Cell::plain(&e.origin),
        ]);
    }
    table
}

pub fn list(ctx: &Context) -> Result<(), CmdError> {
    let all = entries(ctx)?;
    if let Some(text) = json::render(&ctx.args, FIELDS, &entries_json(&all), ctx.out.tty) {
        return output::print(&text?);
    }
    let mut text = ctx.out.table(&table(&all));
    if ctx.is_demo() && ctx.out.tty {
        text.push_str(&format!("\nThese are demo defaults {DEMO_LABEL}\n"));
    }
    output::print(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_paths::MapEnv;

    fn files(list: &[(&str, &str)]) -> Vec<(PathBuf, toml::Table)> {
        list.iter()
            .map(|(p, t)| (PathBuf::from(p), toml::from_str(t).unwrap()))
            .collect()
    }

    fn env() -> MapEnv {
        MapEnv::new("/home/u")
    }

    #[test]
    fn the_last_layer_that_sets_a_key_is_its_origin() {
        let layers = files(&[
            ("/etc/a.toml", "[ui]\ntheme = \"sys\"\njax = false\n"),
            ("/home/b.toml", "[ui]\ntheme = \"mine\"\n"),
        ]);
        assert_eq!(origin_of("ui.theme", &layers, &env()), "/home/b.toml");
        assert_eq!(origin_of("ui.jax", &layers, &env()), "/etc/a.toml");
        assert_eq!(origin_of("ui.mouse", &layers, &env()), "default");
    }

    #[test]
    fn env_overrides_beat_files() {
        let layers = files(&[("/home/b.toml", "[ui]\ntheme = \"mine\"\n")]);
        let env = env().with_var("REVIEW_BUDDY_THEME", "dusk");
        assert_eq!(origin_of("ui.theme", &layers, &env), "$REVIEW_BUDDY_THEME");
    }

    #[test]
    fn source_keys_point_at_the_file_that_names_the_source() {
        let layers = files(&[
            (
                "/a.toml",
                "[[source]]\nname = \"one\"\nkind = \"github\"\nhost = \"h\"\n",
            ),
            (
                "/b.toml",
                "[[source]]\nname = \"two\"\nkind = \"github\"\nhost = \"h2\"\n",
            ),
        ]);
        assert_eq!(origin_of("source.two.host", &layers, &env()), "/b.toml");
        assert_eq!(origin_of("source.one.host", &layers, &env()), "/a.toml");
    }

    #[test]
    fn defaults_render_as_the_config_would_write_them() {
        let all: std::collections::HashMap<_, _> = values(&Config::default()).into_iter().collect();
        assert_eq!(all["ui.theme"], json!("liminal-hq"));
        assert_eq!(all["refresh.interval"], json!("5m"));
        assert_eq!(all["triage.stale_after"], json!("2w"));
        assert_eq!(
            all["triage.show"],
            json!(["reviewing", "assigned", "authored"])
        );
        assert_eq!(all["diff.tab_width"], json!(4));
    }

    #[test]
    fn background_settings_are_listed_with_the_env_origin() {
        let mut config = Config::default();
        config
            .ui
            .theme_background
            .insert("dusk".into(), crate::config::Background::Yes);
        let all: std::collections::HashMap<_, _> = values(&config).into_iter().collect();
        assert_eq!(all["ui.background"], json!("theme"));
        assert_eq!(all["ui.theme_background.dusk"], json!("yes"));
        let env = env().with_var("REVIEW_BUDDY_BACKGROUND", "no");
        assert_eq!(
            origin_of("ui.background", &[], &env),
            "$REVIEW_BUDDY_BACKGROUND"
        );
    }

    #[test]
    fn detail_position_is_listed_with_the_env_origin() {
        let all: std::collections::HashMap<_, _> = values(&Config::default()).into_iter().collect();
        assert_eq!(all["ui.detail_position"], json!("auto"));
        let env = env().with_var("REVIEW_BUDDY_DETAIL_POSITION", "left");
        assert_eq!(
            origin_of("ui.detail_position", &[], &env),
            "$REVIEW_BUDDY_DETAIL_POSITION"
        );
    }

    #[test]
    fn the_order_is_stable() {
        let keys: Vec<_> = values(&Config::default())
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys[0], "ui.theme");
        assert_eq!(keys.last().unwrap(), "checkout.clone_if_missing");
        let mut sorted = keys.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len());
    }

    #[test]
    fn typos_get_suggestions() {
        let known: Vec<String> = values(&Config::default())
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(suggestions("ui.thme", &known)[0], "ui.theme");
        assert!(suggestions("ui", &known)
            .iter()
            .all(|k| k.starts_with("ui.")));
        assert!(suggestions("zzzzzzzzzz.yyyy", &known).is_empty());
    }
}
