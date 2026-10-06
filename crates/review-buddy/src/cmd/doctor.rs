//! `review-buddy doctor`: the broad health check. Its auth section is the same code as
//! `auth status`.

use rb_github::API_VERSION;
use serde_json::{json, Value};

use super::auth::{self, SourceAuth};
use super::context::Context;
use super::error::CmdError;
use super::output::{self, json};
use super::probe::{detect, Detected};
use super::source_test::{caps_json, has, CAPABILITIES};
use super::DEMO_LABEL;
use rb_core::ForgeKind;

const FIELDS: &[&str] = &[
    "version",
    "auth",
    "rateLimits",
    "apiVersions",
    "capabilities",
    "paths",
];

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

fn to_json(ctx: &Context, auths: &[SourceAuth], found: &[Option<Detected>]) -> Value {
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
        "capabilities": auths.iter().zip(found).filter_map(|(a, d)| d.as_ref().map(|d| json!({
            "source": a.name,
            "host": a.host,
            "capabilities": caps_json(Some(&d.outcome.capabilities)),
            "probe": d.json(),
        }))).collect::<Vec<_>>(),
        "paths": {
            "configDir": ctx.paths.paths.config_dir.display().to_string(),
            "dataDir": ctx.paths.paths.data_dir.display().to_string(),
            "cacheDir": ctx.paths.paths.cache_dir.display().to_string(),
            "stateDir": ctx.paths.paths.state_dir.display().to_string(),
        },
    })
}

fn detect_all(ctx: &Context, auths: &[SourceAuth]) -> Result<Vec<Option<Detected>>, CmdError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    Ok(auths.iter().map(|a| detect(ctx, &runtime, a)).collect())
}

fn capability_lines(auths: &[SourceAuth], found: &[Option<Detected>]) -> Vec<String> {
    auths
        .iter()
        .zip(found)
        .filter_map(|(a, d)| {
            let d = d.as_ref()?;
            let forge = match a.kind {
                ForgeKind::GitHub => "GitHub",
                ForgeKind::GitLab => "GitLab",
            };
            let off: Vec<String> = CAPABILITIES
                .iter()
                .filter(|(_, _, which)| !has(&d.outcome.capabilities, *which))
                .map(|(_, label, _)| label.to_lowercase())
                .collect();
            let unavailable = if off.is_empty() {
                "everything available".to_string()
            } else {
                format!("not available: {}", off.join(", "))
            };
            Some(format!(
                "{}  {} · {unavailable}",
                a.label(),
                d.summary(forge)
            ))
        })
        .collect()
}

fn section(text: &mut String, title: &str, body: &str) {
    text.push_str(&format!("\n{title}\n{body}"));
}

fn indented(lines: &[String]) -> String {
    lines.iter().map(|l| format!("  {l}\n")).collect()
}

pub fn run(ctx: &Context) -> Result<(), CmdError> {
    let auths = auth::gather(ctx)?;
    let found = detect_all(ctx, &auths)?;
    if let Some(text) = json::render(
        &ctx.args,
        FIELDS,
        &to_json(ctx, &auths, &found),
        ctx.out.tty,
    ) {
        output::print(&text?)?;
        return auth::require_signed_in(&auths);
    }
    let mut text = format!("Review Buddy {}\n", env!("CARGO_PKG_VERSION"));
    if ctx.is_demo() {
        text.push_str(&format!("Running on demo fixtures {DEMO_LABEL}\n"));
    }
    let lines = if auths.is_empty() {
        "  No sources are configured yet. Run review-buddy --setup or review-buddy source add.\n"
            .to_string()
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
    section(
        &mut text,
        "Capabilities",
        &indented(&capability_lines(&auths, &found)),
    );
    let paths: Vec<String> = ctx.paths.to_string().lines().map(str::to_string).collect();
    section(&mut text, "Paths", &indented(&paths));
    output::print(&text)?;
    auth::require_signed_in(&auths)
}
