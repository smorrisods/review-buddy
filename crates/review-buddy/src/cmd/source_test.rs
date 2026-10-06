//! `review-buddy source test`: sign in to each source and say what it can do.
//!
//! Sign-in is the same check `auth status` runs. Capabilities come from the source's provider,
//! so what's printed here is what the interface will offer or hide.

// Without `live` there is no network, so some of this is only reachable from tests.
#![cfg_attr(not(feature = "live"), allow(dead_code, unused_imports))]

use rb_core::Capabilities;
use serde_json::{json, Value};

use super::auth::{auths_json, render_lines, require_signed_in, Method, SourceAuth, State, FIELDS};
use super::context::Context;
use super::error::CmdError;
use super::output::{self, json};
use super::probe::{detect, Detected};
use super::DEMO_LABEL;
use crate::cli::CapabilityArg;

struct Probe {
    auth: SourceAuth,
    caps: Option<Capabilities>,
    detected: Option<Detected>,
}

pub(super) const CAPABILITIES: &[(&str, &str, CapabilityArg)] = &[
    (
        "requestChanges",
        "Request changes",
        CapabilityArg::RequestChanges,
    ),
    (
        "rangeComments",
        "Range comments",
        CapabilityArg::RangeComments,
    ),
    ("viewedFiles", "Viewed files", CapabilityArg::ViewedFiles),
    ("suggestions", "Suggestions", CapabilityArg::Suggestions),
    (
        "resolveThreads",
        "Resolve threads",
        CapabilityArg::ResolveThreads,
    ),
    (
        "rerunFailed",
        "Re-run failed checks",
        CapabilityArg::RerunFailed,
    ),
];

pub(super) fn has(caps: &Capabilities, which: CapabilityArg) -> bool {
    match which {
        CapabilityArg::RequestChanges => caps.request_changes,
        CapabilityArg::ViewedFiles => caps.viewed_files,
        CapabilityArg::RangeComments => caps.range_comments,
        CapabilityArg::Suggestions => caps.suggestions,
        CapabilityArg::ResolveThreads => caps.resolve_threads,
        CapabilityArg::RerunFailed => caps.rerun_failed,
    }
}

pub(super) fn caps_json(caps: Option<&Capabilities>) -> Value {
    caps.map_or(Value::Null, |caps| {
        Value::Object(
            CAPABILITIES
                .iter()
                .map(|(key, _, which)| ((*key).to_string(), json!(has(caps, *which))))
                .collect(),
        )
    })
}

fn render(probe: &Probe, ctx: &Context) -> String {
    let mut text = render_lines(std::slice::from_ref(&probe.auth), &ctx.out.painter);
    match &probe.caps {
        Some(caps) => {
            if let Some(found) = &probe.detected {
                let forge = match probe.auth.kind {
                    rb_core::ForgeKind::GitHub => "GitHub",
                    rb_core::ForgeKind::GitLab => "GitLab",
                };
                text.push_str(&format!(
                    "    Detected              {}\n",
                    found.summary(forge)
                ));
            }
            for (_, label, which) in CAPABILITIES {
                let answer = if has(caps, *which) { "yes" } else { "no" };
                text.push_str(&format!("    {label:<22}{answer}\n"));
            }
            if let Some(found) = &probe.detected {
                for (_, why) in &found.outcome.reasons {
                    text.push_str(&format!("    {why}\n"));
                }
            }
        }
        None => text.push_str("    Capabilities weren't checked because sign-in failed.\n"),
    }
    text
}

fn demo_probes(ctx: &Context, name: Option<&str>) -> Result<Vec<Probe>, CmdError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut out = Vec::new();
    for source in ctx.sources()? {
        if name.is_some_and(|n| source.id.as_str() != n && source.label != n) {
            continue;
        }
        let provider = ctx.provider_for(&source)?;
        let user = runtime.block_on(provider.whoami())?;
        let auth = SourceAuth {
            name: source.label.clone(),
            kind: source.kind,
            host: source.host.clone(),
            api_url: None,
            method: Some(Method::Demo),
            state: State::SignedIn {
                user: user.login,
                scopes: Vec::new(),
                expires: None,
            },
            report: None,
        };
        let detected = detect(ctx, &runtime, &auth);
        out.push(Probe {
            caps: detected.as_ref().map(|d| d.outcome.capabilities),
            detected,
            auth,
        });
    }
    if out.is_empty() {
        if let Some(name) = name {
            return Err(unknown(
                name,
                &ctx.sources()?
                    .iter()
                    .map(|s| s.label.clone())
                    .collect::<Vec<_>>(),
            ));
        }
    }
    Ok(out)
}

fn unknown(name: &str, known: &[String]) -> CmdError {
    CmdError::usage(format!(
        "There's no source called {name}.\nConfigured sources: {}.",
        if known.is_empty() {
            "none".to_string()
        } else {
            known.join(", ")
        }
    ))
}

#[cfg(feature = "live")]
fn live_probes(ctx: &Context, name: Option<&str>) -> Result<Vec<Probe>, CmdError> {
    let all = ctx.configured()?;
    let flagged: Vec<&str> = ctx.args.sources.iter().map(String::as_str).collect();
    let wanted: Vec<&str> = name.into_iter().chain(flagged).collect();
    let mut chosen = Vec::new();
    if wanted.is_empty() {
        chosen.extend(all.iter().filter(|s| s.enabled));
    }
    for want in &wanted {
        match all.iter().find(|s| s.name.eq_ignore_ascii_case(want)) {
            Some(found) => chosen.push(found),
            None => {
                let known: Vec<String> = all.iter().map(|s| s.name.clone()).collect();
                return Err(unknown(want, &known));
            }
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let factory = ctx.factory();
    let mut out = Vec::new();
    for cfg in chosen {
        let auth = runtime.block_on(super::auth::check_source(cfg, &factory));
        let signed_in = matches!(auth.state, State::SignedIn { .. });
        let detected = if signed_in && cfg.enabled {
            detect(ctx, &runtime, &auth)
        } else {
            None
        };
        let caps = detected.as_ref().map(|d| d.outcome.capabilities);
        out.push(Probe {
            auth,
            caps,
            detected,
        });
    }
    Ok(out)
}

fn require_capabilities(probes: &[Probe], required: &[CapabilityArg]) -> Result<(), CmdError> {
    let mut lacking = Vec::new();
    for probe in probes {
        let Some(caps) = &probe.caps else { continue };
        for which in required {
            if !has(caps, *which) {
                let label = CAPABILITIES
                    .iter()
                    .find(|(_, _, w)| w == which)
                    .map_or("that", |(_, label, _)| *label);
                lacking.push(format!(
                    "  {} doesn't support {}",
                    probe.auth.host,
                    label.to_lowercase()
                ));
            }
        }
    }
    if lacking.is_empty() {
        Ok(())
    } else {
        Err(CmdError::Unsupported(format!(
            "A source can't do everything you asked for:\n{}\nLeave a comment instead, or use the web page.",
            lacking.join("\n")
        )))
    }
}

pub fn run(ctx: &Context, name: Option<&str>, require: &[CapabilityArg]) -> Result<(), CmdError> {
    let probes = if ctx.is_demo() {
        demo_probes(ctx, name)?
    } else {
        #[cfg(feature = "live")]
        {
            live_probes(ctx, name)?
        }
        #[cfg(not(feature = "live"))]
        {
            return Err(super::host::no_network());
        }
    };
    let auths: Vec<SourceAuth> = probes.iter().map(|p| p.auth.clone()).collect();
    let mut fields: Vec<&str> = FIELDS.to_vec();
    fields.push("capabilities");
    fields.push("probe");
    let mut value = auths_json(&auths);
    if let Value::Array(items) = &mut value {
        for (item, probe) in items.iter_mut().zip(&probes) {
            item["capabilities"] = caps_json(probe.caps.as_ref());
            item["probe"] = probe.detected.as_ref().map_or(Value::Null, Detected::json);
        }
    }
    if let Some(text) = json::render(&ctx.args, &fields, &value, ctx.out.tty) {
        output::print(&text?)?;
    } else if probes.is_empty() {
        output::print("No sources are configured yet.\nAdd one with review-buddy source add, or try --demo.\n")?;
    } else {
        let mut text: String = probes.iter().map(|p| render(p, ctx)).collect();
        if ctx.is_demo() {
            text.push_str(&format!("\nThese are demo sources {DEMO_LABEL}\n"));
        }
        output::print(&text)?;
    }
    require_signed_in(&auths)?;
    require_capabilities(&probes, require)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::ForgeKind;

    fn probe(host: &str, caps: Option<Capabilities>) -> Probe {
        Probe {
            auth: SourceAuth {
                name: host.into(),
                kind: ForgeKind::GitLab,
                host: host.into(),
                api_url: None,
                method: Some(Method::Glab),
                state: State::SignedIn {
                    user: "me".into(),
                    scopes: Vec::new(),
                    expires: None,
                },
                report: None,
            },
            caps,
            detected: None,
        }
    }

    #[test]
    fn json_lists_every_capability() {
        let value = caps_json(Some(&Capabilities::none()));
        assert_eq!(value.as_object().unwrap().len(), CAPABILITIES.len());
        assert_eq!(value["requestChanges"], false);
        assert_eq!(caps_json(None), Value::Null);
    }

    #[test]
    fn missing_required_capabilities_exit_5_and_name_the_host() {
        let caps = Capabilities {
            request_changes: false,
            ..Capabilities::all()
        };
        let probes = vec![
            probe("gl.test", Some(caps)),
            probe("gh.test", Some(Capabilities::all())),
        ];
        assert!(require_capabilities(&probes, &[CapabilityArg::ViewedFiles]).is_ok());
        let err = require_capabilities(&probes, &[CapabilityArg::RequestChanges]).unwrap_err();
        assert_eq!(err.exit().code(), 5);
        assert!(err
            .to_string()
            .contains("gl.test doesn't support request changes"));
        assert!(!err.to_string().contains("gh.test"));
    }

    #[test]
    fn unchecked_sources_are_not_blamed() {
        assert!(require_capabilities(&[probe("x", None)], &[CapabilityArg::RerunFailed]).is_ok());
    }
}
