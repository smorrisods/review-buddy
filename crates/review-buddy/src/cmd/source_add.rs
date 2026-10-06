//! `review-buddy source add`: append a `[[source]]` to the config file.
//!
//! Previews the block, asks on a terminal (default No), then goes through `setup::add_source`
//! so comments and ordering in the file survive. A token test follows unless `--no-test`.

// Without `live` there is no network, so some of this is only reachable from tests.
#![cfg_attr(not(feature = "live"), allow(dead_code, unused_imports))]

use rb_core::ForgeKind;

use super::context::Context;
use super::error::CmdError;
use super::host::{infer_kind, Target};
use super::output;
use super::prompt::confirm_write;
use super::DEMO_LABEL;
use crate::cli::{KindArg, SourceAddArgs};
use crate::config::AuthSetting;
use crate::setup::{add_source, AddError, AuthKind, SourceSpec};

fn parse_auth(args: &SourceAddArgs) -> Result<AuthKind, CmdError> {
    let method = args.auth.trim();
    let kind = match method {
        "cli" => AuthKind::Cli,
        "token" => AuthKind::Token,
        "command" => {
            let command = args.token_command.as_deref().map(str::trim).unwrap_or("");
            if command.is_empty() {
                return Err(CmdError::usage(
                    "--auth command needs --token-command, the command that prints a token.\nFor example: --token-command \"pass show github\"",
                ));
            }
            AuthKind::Command(command.to_string())
        }
        other => match other.strip_prefix("env:").filter(|v| !v.is_empty()) {
            Some(var) => AuthKind::Env(var.to_string()),
            None => {
                return Err(CmdError::usage(format!(
                    "Unknown auth method {other}.\nUse cli, token, command or env:VAR_NAME."
                )))
            }
        },
    };
    if args.token_command.is_some() && !matches!(kind, AuthKind::Command(_)) {
        return Err(CmdError::usage(
            "--token-command only goes with --auth command.",
        ));
    }
    Ok(kind)
}

/// Turns the flags into the source to write. Pure: nothing is read or written.
pub fn parse_spec(args: &SourceAddArgs) -> Result<SourceSpec, CmdError> {
    let kind = match (args.kind, &args.host) {
        (Some(KindArg::Github), _) => ForgeKind::GitHub,
        (Some(KindArg::Gitlab), _) => ForgeKind::GitLab,
        (None, Some(host)) => infer_kind(host),
        (None, None) => ForgeKind::GitHub,
    };
    let host = args.host.clone().unwrap_or_else(|| {
        match kind {
            ForgeKind::GitHub => "github.com",
            ForgeKind::GitLab => "gitlab.com",
        }
        .to_string()
    });
    let (owners, wrong_flag) = match kind {
        ForgeKind::GitHub => (&args.org, (!args.group.is_empty()).then_some("--group")),
        ForgeKind::GitLab => (
            &args.group,
            (!args.org.is_empty())
                .then_some("--org")
                .or_else(|| args.user.then_some("--user")),
        ),
    };
    if let Some(flag) = wrong_flag {
        let (forge, instead) = match kind {
            ForgeKind::GitHub => ("GitHub", "--org"),
            ForgeKind::GitLab => ("GitLab", "--group"),
        };
        return Err(CmdError::usage(format!(
            "{flag} doesn't apply to a {forge} source.\nUse {instead} for the {forge} scope."
        )));
    }
    Ok(SourceSpec {
        name: args.name.clone().unwrap_or_else(|| host.clone()),
        kind,
        host,
        api_url: args.api_url.clone(),
        auth: parse_auth(args)?,
        scope_user: args.user,
        owners: owners.clone(),
    })
}

fn target_of(spec: &SourceSpec) -> Target {
    let (auth, token_command) = match &spec.auth {
        AuthKind::Cli => (AuthSetting::Cli, None),
        AuthKind::Token => (AuthSetting::Token, None),
        AuthKind::Env(var) => (AuthSetting::Env(var.clone()), None),
        AuthKind::Command(command) => (AuthSetting::Command, Some(command.clone())),
    };
    Target {
        host: spec.host.clone(),
        kind: spec.kind,
        api_url: spec.api_url.clone(),
        source: Some(spec.name.clone()),
        auth: Some(auth),
        token_command,
    }
}

fn preview(spec: &SourceSpec, path: &std::path::Path) -> String {
    format!(
        "This adds a source to {}:\n\n{}",
        path.display(),
        spec.to_toml()
    )
}

#[cfg(feature = "live")]
fn test_sign_in(spec: &SourceSpec) -> Result<String, CmdError> {
    use super::auth_token::{resolve, Sources};
    let target = target_of(spec);
    let missing_token = || {
        format!(
            "Added {}, but there's no token for {} yet.\nRun review-buddy auth login --host {} to store one.",
            spec.name, spec.host, spec.host
        )
    };
    let deps = crate::providers::Deps::system();
    let from = Sources {
        runner: deps.runner,
        store: deps.store,
        getenv: deps.getenv,
    };
    let (secret, origin) = match resolve(&target, &from) {
        Ok(found) => found,
        Err(CmdError::AuthNeeded(_)) => return Err(CmdError::AuthNeeded(missing_token())),
        Err(other) => return Err(other),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let tested = runtime
        .block_on(super::host::test_token(&target, secret))
        .map_err(|e| match CmdError::from(e) {
            CmdError::AuthNeeded(why) => CmdError::AuthNeeded(format!(
                "Added {}, but signing in failed.\n{why}",
                spec.name
            )),
            other => other,
        })?;
    Ok(format!(
        "✓ {} works: signed in as {} · {}. {origin}\n",
        spec.host,
        tested.login,
        tested.scope_text()
    ))
}

pub fn run(ctx: &Context, args: &SourceAddArgs) -> Result<(), CmdError> {
    let spec = parse_spec(args)?;
    let path = &ctx.paths.write_target;
    let shown = preview(&spec, path);
    if ctx.is_demo() {
        return output::print(&format!("{shown}\nNothing was written {DEMO_LABEL}\n"));
    }
    if ctx.interaction.yes {
        output::print(&format!("{shown}\n"))?;
    }
    let stdin = std::io::stdin();
    confirm_write(
        &shown,
        ctx.interaction,
        &mut stdin.lock(),
        &mut std::io::stdout(),
    )?;
    add_source(path, &spec).map_err(|e| match e {
        AddError::Duplicate(_) => CmdError::usage(e.to_string()),
        AddError::Config(_) => CmdError::failed(format!("{e}\nNothing was changed.")),
    })?;
    output::print(&format!(
        "Added source {} to {}.\n",
        spec.name,
        path.display()
    ))?;
    if args.no_test {
        return Ok(());
    }
    #[cfg(feature = "live")]
    {
        output::print(&test_sign_in(&spec)?)
    }
    #[cfg(not(feature = "live"))]
    {
        let _ = target_of(&spec);
        output::print("Skipped the sign-in test: this build has no network support.\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: impl FnOnce(&mut SourceAddArgs)) -> SourceAddArgs {
        let mut a = SourceAddArgs {
            auth: "cli".into(),
            ..SourceAddArgs::default()
        };
        extra(&mut a);
        a
    }

    #[test]
    fn defaults_follow_the_kind_and_the_host_is_the_name() {
        let spec = parse_spec(&args(|_| {})).unwrap();
        assert_eq!(
            (spec.kind, spec.host.as_str(), spec.name.as_str()),
            (ForgeKind::GitHub, "github.com", "github.com")
        );
        let spec = parse_spec(&args(|a| a.kind = Some(KindArg::Gitlab))).unwrap();
        assert_eq!(spec.host, "gitlab.com");
        let spec = parse_spec(&args(|a| a.host = Some("gitlab.work.ca".into()))).unwrap();
        assert_eq!(spec.kind, ForgeKind::GitLab);
    }

    #[test]
    fn auth_forms_parse_and_bad_ones_are_usage_errors() {
        let auth = |text: &str, cmd: Option<&str>| {
            parse_spec(&args(|a| {
                a.auth = text.into();
                a.token_command = cmd.map(Into::into);
            }))
        };
        assert_eq!(auth("token", None).unwrap().auth, AuthKind::Token);
        assert_eq!(
            auth("env:GH_T", None).unwrap().auth,
            AuthKind::Env("GH_T".into())
        );
        assert_eq!(
            auth("command", Some("pass gh")).unwrap().auth,
            AuthKind::Command("pass gh".into())
        );
        for bad in [
            auth("command", None),
            auth("env:", None),
            auth("magic", None),
            auth("cli", Some("x")),
        ] {
            assert_eq!(bad.unwrap_err().exit().code(), 2);
        }
    }

    #[test]
    fn scope_flags_must_match_the_forge() {
        let gh = parse_spec(&args(|a| {
            a.org = vec!["liminal-hq".into()];
            a.user = true;
        }))
        .unwrap();
        assert_eq!(
            (gh.owners, gh.scope_user),
            (vec!["liminal-hq".to_string()], true)
        );
        let err = parse_spec(&args(|a| a.group = vec!["g".into()])).unwrap_err();
        assert!(err.to_string().contains("Use --org"));
        let err = parse_spec(&args(|a| {
            a.kind = Some(KindArg::Gitlab);
            a.org = vec!["o".into()];
        }))
        .unwrap_err();
        assert!(err.to_string().contains("Use --group"));
    }

    #[test]
    fn the_preview_shows_the_block_and_the_file() {
        let spec = parse_spec(&args(|a| a.name = Some("work".into()))).unwrap();
        let text = preview(&spec, std::path::Path::new("/x/config.toml"));
        assert!(text.starts_with("This adds a source to /x/config.toml:"));
        assert!(text.contains("[[source]]\nname = \"work\""));
    }
}
