//! `review-buddy doctor`: the broad health check. Its auth section is the same code as
//! `auth status`.

use rb_github::API_VERSION;
use rb_platform::Secret;
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
    "endpoints",
    "clock",
    "capabilities",
    "paths",
];

/// How far the server's clock may differ from ours before `doctor` says so. Token expiry
/// checks compare dates, so a skewed clock can make a valid token look expired.
pub const CLOCK_SKEW_WARN_SECS: i64 = 300;

/// What a forge said about itself. Capability work can build on this without another probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versions {
    /// `GitHub.com`, `GitHub Enterprise Server` or `GitLab`.
    pub product: String,
    /// The server's own version: the GHES release, or GitLab's `version`. `None` on github.com.
    pub server: Option<String>,
    /// GitLab's build revision.
    pub revision: Option<String>,
    /// The API version Review Buddy talks: the pinned GitHub REST date, or `v4`.
    pub api: String,
    /// The server's clock in Unix seconds, from its `Date` header.
    pub server_time: Option<i64>,
}

impl Versions {
    #[cfg_attr(not(feature = "live"), allow(dead_code))]
    pub fn github(info: &rb_github::ServerInfo, host: &str) -> Self {
        let product = match (&info.enterprise_version, host) {
            (Some(_), _) => "GitHub Enterprise Server",
            (None, "github.com") => "GitHub.com",
            (None, _) => "GitHub",
        };
        Self {
            product: product.into(),
            server: info.enterprise_version.clone(),
            revision: None,
            api: API_VERSION.into(),
            server_time: info.server_time,
        }
    }

    #[cfg_attr(not(feature = "live"), allow(dead_code))]
    pub fn gitlab(info: &rb_gitlab::ServerInfo) -> Self {
        Self {
            product: "GitLab".into(),
            server: Some(info.version.clone()),
            revision: info.revision.clone(),
            api: "v4".into(),
            server_time: info.server_time,
        }
    }

    /// `server_time - local`: positive when the server's clock is ahead.
    pub fn skew_secs(&self, local_now: i64) -> Option<i64> {
        self.server_time.map(|t| t - local_now)
    }

    fn describe(&self) -> String {
        let mut text = self.product.clone();
        if let Some(v) = &self.server {
            text.push_str(&format!(" {v}"));
        }
        if let Some(r) = &self.revision {
            text.push_str(&format!(" ({r})"));
        }
        text
    }
}

/// The addresses a source will use, worked out from its host and `api_url`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub rest: String,
    pub graphql: Option<String>,
    pub web: String,
}

impl Endpoints {
    pub fn of(a: &SourceAuth) -> Option<Self> {
        let api = a.api_url.as_deref();
        match a.kind {
            ForgeKind::GitHub => {
                let c = rb_github::GithubClient::new(&a.host, api, Secret::new("")).ok()?;
                Some(Self {
                    rest: c.rest_base().as_str().trim_end_matches('/').to_string(),
                    graphql: Some(c.graphql_url().to_string()),
                    web: c.web_base().as_str().trim_end_matches('/').to_string(),
                })
            }
            ForgeKind::GitLab => {
                let c = rb_gitlab::GitlabClient::new(&a.host, api, Secret::new("")).ok()?;
                Some(Self {
                    rest: c.api_base().as_str().trim_end_matches('/').to_string(),
                    graphql: None,
                    web: c.web_base().as_str().trim_end_matches('/').to_string(),
                })
            }
        }
    }

    fn describe(&self) -> String {
        let mut text = format!("REST {}", self.rest);
        if let Some(g) = &self.graphql {
            text.push_str(&format!(" · GraphQL {g}"));
        }
        text.push_str(&format!(" · web {}", self.web));
        text
    }
}

/// What probing one source found. `versions` is `None` when the source isn't signed in or the
/// server wouldn't say; `note` then says why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    pub versions: Option<Versions>,
    pub note: Option<String>,
}

#[cfg(feature = "live")]
async fn probe_source(a: &SourceAuth, factory: &crate::providers::Factory) -> Probe {
    use rb_core::SourceId;
    if !matches!(a.state, auth::State::SignedIn { .. }) {
        return Probe::default();
    }
    let id = SourceId::new(&a.name);
    let result = match a.kind {
        ForgeKind::GitHub => match factory.github_client(&id) {
            Ok((client, _)) => client
                .probe()
                .await
                .map(|info| Versions::github(&info, &a.host)),
            Err(e) => return note(e.to_string()),
        },
        ForgeKind::GitLab => match factory.gitlab_client(&id) {
            Ok((client, _)) => client.probe().await.map(|info| Versions::gitlab(&info)),
            Err(e) => return note(e.to_string()),
        },
    };
    match result {
        Ok(versions) => Probe {
            versions: Some(versions),
            note: None,
        },
        Err(e) => note(format!("the server didn't report its version ({e})")),
    }
}

#[cfg(feature = "live")]
fn note(text: String) -> Probe {
    Probe {
        versions: None,
        note: Some(text),
    }
}

fn probe_all(ctx: &Context, auths: &[SourceAuth]) -> Vec<Probe> {
    #[cfg(feature = "live")]
    if !ctx.is_demo() {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return vec![Probe::default(); auths.len()];
        };
        let factory = ctx.factory();
        return runtime.block_on(async {
            let mut out = Vec::new();
            for a in auths {
                out.push(probe_source(a, &factory).await);
            }
            out
        });
    }
    let _ = ctx;
    vec![Probe::default(); auths.len()]
}

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

fn api_line(a: &SourceAuth, probe: &Probe) -> String {
    let api = match a.kind {
        ForgeKind::GitHub => format!("REST {API_VERSION}"),
        ForgeKind::GitLab => "REST v4".to_string(),
    };
    if a.is_demo() {
        let product = match a.kind {
            ForgeKind::GitHub => "GitHub",
            ForgeKind::GitLab => "GitLab",
        };
        return format!("{}  {product} {api} {DEMO_LABEL}", a.label());
    }
    let product = probe.versions.as_ref().map_or_else(
        || match a.kind {
            ForgeKind::GitHub => "GitHub".to_string(),
            ForgeKind::GitLab => "GitLab".to_string(),
        },
        Versions::describe,
    );
    let mut line = format!("{}  {product} · {api}", a.label());
    if let Some(note) = &probe.note {
        line.push_str(&format!(" · {note}"));
    }
    line
}

fn endpoint_line(a: &SourceAuth) -> String {
    if a.is_demo() {
        return format!("{}  not used {DEMO_LABEL}", a.label());
    }
    match Endpoints::of(a) {
        Some(e) => format!("{}  {}", a.label(), e.describe()),
        None => format!(
            "{}  the API address isn't a valid URL · check `api_url` in the source",
            a.label()
        ),
    }
}

fn skew_text(skew: i64) -> String {
    let secs = skew.unsigned_abs();
    let amount = if secs >= 2 * 86_400 {
        format!("{} days", secs / 86_400)
    } else if secs >= 2 * 3600 {
        format!("{} hours", secs / 3600)
    } else if secs >= 120 {
        format!("{} min", secs.div_ceil(60))
    } else {
        format!("{secs} s")
    };
    let direction = if skew >= 0 { "ahead of" } else { "behind" };
    format!("{amount} {direction} this machine")
}

fn clock_lines(auths: &[SourceAuth], probes: &[Probe]) -> Vec<String> {
    let now = now_epoch() as i64;
    auths
        .iter()
        .zip(probes)
        .filter_map(|(a, p)| {
            if a.is_demo() {
                return Some(format!("{}  not checked {DEMO_LABEL}", a.label()));
            }
            let skew = p.versions.as_ref()?.skew_secs(now)?;
            let text = if skew.abs() > CLOCK_SKEW_WARN_SECS {
                format!(
                    "the server's clock is {}. Token expiry checks may be wrong; sync this machine's clock",
                    skew_text(skew)
                )
            } else {
                "in step with this machine".to_string()
            };
            Some(format!("{}  {text}", a.label()))
        })
        .collect()
}

fn to_json(
    ctx: &Context,
    auths: &[SourceAuth],
    probes: &[Probe],
    found: &[Option<Detected>],
) -> Value {
    let now = now_epoch() as i64;
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
        "apiVersions": auths.iter().zip(probes).map(|(a, p)| {
            let v = p.versions.as_ref();
            json!({
                "source": a.name,
                "host": a.host,
                "kind": auth::kind_name(a.kind),
                "rest": match a.kind {
                    ForgeKind::GitHub => API_VERSION,
                    ForgeKind::GitLab => "v4",
                },
                "product": v.map(|v| v.product.clone()),
                "server": v.and_then(|v| v.server.clone()),
                "revision": v.and_then(|v| v.revision.clone()),
                "note": p.note,
            })
        }).collect::<Vec<_>>(),
        "endpoints": auths.iter().filter(|a| !a.is_demo()).map(|a| {
            let e = Endpoints::of(a);
            json!({
                "source": a.name,
                "host": a.host,
                "rest": e.as_ref().map(|e| e.rest.clone()),
                "graphql": e.as_ref().and_then(|e| e.graphql.clone()),
                "web": e.as_ref().map(|e| e.web.clone()),
            })
        }).collect::<Vec<_>>(),
        "clock": auths.iter().zip(probes).filter_map(|(a, p)| {
            let skew = p.versions.as_ref()?.skew_secs(now)?;
            Some(json!({
                "source": a.name,
                "host": a.host,
                "skewSeconds": skew,
                "warning": skew.abs() > CLOCK_SKEW_WARN_SECS,
            }))
        }).collect::<Vec<_>>(),
        "capabilities": auths.iter().zip(found).filter_map(|(a, d)| d.as_ref().map(|d| json!({
            "source": a.name,
            "host": a.host,
            "capabilities": caps_json(Some(&d.outcome.capabilities)),
            "probe": d.json(),
        }))).collect::<Vec<_>>(),
        "paths": super::config::paths_json(&ctx.paths),
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
    let probes = probe_all(ctx, &auths);
    let found = detect_all(ctx, &auths)?;
    if let Some(text) = json::render(
        &ctx.args,
        FIELDS,
        &to_json(ctx, &auths, &probes, &found),
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
    let api: Vec<String> = auths
        .iter()
        .zip(&probes)
        .map(|(a, p)| api_line(a, p))
        .collect();
    section(&mut text, "API versions", &indented(&api));
    let endpoints: Vec<String> = auths.iter().map(endpoint_line).collect();
    section(&mut text, "Endpoints", &indented(&endpoints));
    section(&mut text, "Clock", &indented(&clock_lines(&auths, &probes)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skew_reads_in_the_nearest_sensible_unit() {
        assert_eq!(skew_text(45), "45 s ahead of this machine");
        assert_eq!(skew_text(-480), "8 min behind this machine");
        assert_eq!(skew_text(3 * 3600 + 5), "3 hours ahead of this machine");
        assert_eq!(skew_text(-3 * 86_400), "3 days behind this machine");
    }

    #[test]
    fn versions_describe_themselves_and_report_skew() {
        let v = Versions {
            product: "GitLab".into(),
            server: Some("16.11.2-ee".into()),
            revision: Some("abc123".into()),
            api: "v4".into(),
            server_time: Some(1_000),
        };
        assert_eq!(v.describe(), "GitLab 16.11.2-ee (abc123)");
        assert_eq!(v.skew_secs(400), Some(600));
        let gh = Versions::github(
            &rb_github::ServerInfo {
                enterprise_version: None,
                server_time: None,
            },
            "github.com",
        );
        assert_eq!(gh.describe(), "GitHub.com");
        assert_eq!(gh.skew_secs(0), None);
    }
}
