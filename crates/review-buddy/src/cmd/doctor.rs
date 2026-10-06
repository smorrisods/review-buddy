//! `review-buddy doctor`: the broad health check. Its auth section is the same code as
//! `auth status`.

use rb_github::API_VERSION;
use serde_json::{json, Value};

use super::auth::{self, SourceAuth};
use super::context::Context;
use super::error::CmdError;
use super::output::{self, json};
use super::DEMO_LABEL;
use rb_core::ForgeKind;

const FIELDS: &[&str] = &["version", "auth", "rateLimits", "apiVersions", "paths"];

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn resets_in(reset: u64) -> String {
    let minutes = reset.saturating_sub(now_epoch()).div_ceil(60);
    format!("resets in {minutes} min")
}

fn rate_lines(auths: &[SourceAuth]) -> Vec<String> {
    auths
        .iter()
        .filter_map(|a| {
            let line = match (&a.state, &a.report) {
                (_, Some(r)) => {
                    let mut text = format!(
                        "core {}/{} · {}",
                        r.core.remaining,
                        r.core.limit,
                        resets_in(r.core.reset)
                    );
                    if let Some(g) = r.graphql {
                        text.push_str(&format!(" · graphql {}/{}", g.remaining, g.limit));
                    }
                    text
                }
                _ if a.is_demo() => format!("not reported {DEMO_LABEL}"),
                _ => return None,
            };
            Some(format!("{}  {line}", a.label()))
        })
        .collect()
}

fn api_line(a: &SourceAuth) -> String {
    match a.kind {
        ForgeKind::GitHub => {
            let base = a.api_url.clone().unwrap_or_else(|| {
                if a.host == "github.com" {
                    "https://api.github.com".into()
                } else {
                    format!("https://{}/api/v3", a.host)
                }
            });
            if a.is_demo() {
                format!("{}  GitHub REST {API_VERSION} {DEMO_LABEL}", a.label())
            } else {
                format!("{}  GitHub REST {API_VERSION} · {base}", a.label())
            }
        }
        ForgeKind::GitLab => {
            let base = a
                .api_url
                .clone()
                .unwrap_or_else(|| format!("https://{}/api/v4", a.host));
            if a.is_demo() {
                format!("{}  GitLab REST v4 {DEMO_LABEL}", a.label())
            } else {
                format!("{}  GitLab REST v4 · {base}", a.label())
            }
        }
    }
}

fn to_json(ctx: &Context, auths: &[SourceAuth]) -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "auth": auth::auths_json(auths),
        "rateLimits": auths.iter().filter_map(|a| a.report.as_ref().map(|r| json!({
            "source": a.name,
            "host": a.host,
            "core": {"limit": r.core.limit, "remaining": r.core.remaining, "reset": r.core.reset},
            "graphql": r.graphql.map(|g| json!({
                "limit": g.limit, "remaining": g.remaining, "reset": g.reset,
            })),
        }))).collect::<Vec<_>>(),
        "apiVersions": auths.iter().filter(|a| a.kind == ForgeKind::GitHub).map(|a| json!({
            "source": a.name,
            "host": a.host,
            "rest": API_VERSION,
        })).collect::<Vec<_>>(),
        "paths": {
            "configDir": ctx.paths.paths.config_dir.display().to_string(),
            "dataDir": ctx.paths.paths.data_dir.display().to_string(),
            "cacheDir": ctx.paths.paths.cache_dir.display().to_string(),
            "stateDir": ctx.paths.paths.state_dir.display().to_string(),
        },
    })
}

fn section(text: &mut String, title: &str, body: &str) {
    text.push_str(&format!("\n{title}\n{body}"));
}

fn indented(lines: &[String]) -> String {
    lines.iter().map(|l| format!("  {l}\n")).collect()
}

pub fn run(ctx: &Context) -> Result<(), CmdError> {
    let auths = auth::gather(ctx)?;
    if let Some(text) = json::render(&ctx.args, FIELDS, &to_json(ctx, &auths), ctx.out.tty) {
        output::print(&text?)?;
        return auth::require_signed_in(&auths);
    }
    let mut text = format!("Review Buddy {}\n", env!("CARGO_PKG_VERSION"));
    if ctx.is_demo() {
        text.push_str(&format!("Running on demo fixtures {DEMO_LABEL}\n"));
    }
    let lines = if auths.is_empty() {
        "  No sources are configured yet. Add one with review-buddy source add.\n".to_string()
    } else {
        auth::render_lines(&auths, &ctx.out.painter)
            .lines()
            .map(|l| format!("  {l}\n"))
            .collect()
    };
    section(&mut text, "Auth", &lines);
    section(&mut text, "Rate limits", &indented(&rate_lines(&auths)));
    let api: Vec<String> = auths.iter().map(api_line).collect();
    section(&mut text, "API versions", &indented(&api));
    let paths: Vec<String> = ctx.paths.to_string().lines().map(str::to_string).collect();
    section(&mut text, "Paths", &indented(&paths));
    output::print(&text)?;
    auth::require_signed_in(&auths)
}
