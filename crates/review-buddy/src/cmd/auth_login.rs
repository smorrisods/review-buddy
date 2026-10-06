//! `review-buddy auth login`: test a token, then keep it in the OS keyring.
//!
//! The token is read from standard input (`--with-token`) or a hidden prompt, held as a
//! redacting `Secret`, and goes to the keyring and the forge's token test. It is never printed,
//! logged or written to a file.

// Without `live` there is no network, so some of this is only reachable from tests.
#![cfg_attr(not(feature = "live"), allow(dead_code, unused_imports))]

use std::io::Read;

use rb_platform::{Secret, SecretStore};

use super::context::Context;
use super::error::CmdError;
use super::host::{self, Target};
use super::output;
use super::DEMO_LABEL;

#[cfg(feature = "live")]
use rb_core::Error;

fn read_stdin_token(input: &mut dyn Read) -> Result<Secret, CmdError> {
    let mut text = String::new();
    input.read_to_string(&mut text)?;
    let token = text.trim();
    if token.is_empty() {
        return Err(CmdError::usage(
            "No token arrived on standard input.\nPipe one in, for example: pass show github | review-buddy auth login --with-token",
        ));
    }
    Ok(Secret::new(token))
}

#[cfg(feature = "live")]
fn ask_for_token(host: &str) -> Result<Secret, CmdError> {
    let prompt = format!("Paste a token for {host} (it won't be shown):");
    let line = crate::setup::plain::hidden_reader(&prompt)?;
    let token = line.trim();
    if token.is_empty() || token == "skip" {
        return Err(CmdError::Cancelled(
            "No token entered. Nothing was changed.".into(),
        ));
    }
    Ok(Secret::new(token))
}

fn rejected(host: &str) -> CmdError {
    CmdError::AuthNeeded(format!(
        "{host} rejected that token.\nCheck that you copied all of it, then try again with review-buddy auth login --host {host}."
    ))
}

#[cfg(feature = "live")]
fn tell(err: Error, target: &Target) -> CmdError {
    match err {
        Error::Unauthorized { .. } => rejected(&target.host),
        Error::Forbidden { reason, .. } => CmdError::AuthNeeded(format!(
            "{} refused that token: {reason}.\nCheck its scopes and any organisation SSO approval, then try again.",
            target.host
        )),
        other => other.into(),
    }
}

fn scopes_wanted(target: &Target) -> &'static str {
    match target.kind {
        rb_core::ForgeKind::GitHub => "repo and read:org",
        rb_core::ForgeKind::GitLab => "api and read_user",
    }
}

/// Tests `secret` against the forge and, only if it works, stores it. Returns the message to print.
#[cfg(feature = "live")]
pub async fn sign_in(
    target: &Target,
    secret: Secret,
    store: &dyn SecretStore,
) -> Result<String, CmdError> {
    let tested = host::test_token(target, secret.clone())
        .await
        .map_err(|e| tell(e, target))?;
    store.set(&target.host, &secret).map_err(|e| {
        CmdError::failed(format!(
            "The token works, but the OS keyring wouldn't keep it: {e}.\nSet it in an environment variable instead, and use auth = \"env:VAR\" on the source."
        ))
    })?;
    let mut text = format!(
        "✓ Signed in to {} as {} · {}\nThe token is kept in the OS keyring as review-buddy/{}. It isn't written to config.toml.\n",
        target.host,
        tested.login,
        tested.scope_text(),
        target.host
    );
    if tested.scopes.is_empty() {
        text.push_str(&format!(
            "It didn't list its scopes, so check it has {}.\n",
            scopes_wanted(target)
        ));
    }
    if let Some(name) = &target.source {
        if target.auth != Some(crate::config::AuthSetting::Token) {
            text.push_str(&format!(
                "Source {name} doesn't use it yet. Set auth = \"token\" on that source to switch.\n"
            ));
        }
    }
    Ok(text)
}

pub fn run(ctx: &Context, host: Option<&str>, with_token: bool) -> Result<(), CmdError> {
    let target = host::resolve(ctx, host)?;
    if ctx.is_demo() {
        return output::print(&format!(
            "Would test a token for {} and keep it in the OS keyring {DEMO_LABEL}\nNothing was read or saved.\n",
            target.host
        ));
    }
    #[cfg(not(feature = "live"))]
    {
        let _ = (with_token, scopes_wanted(&target), rejected(""));
        Err(host::no_network())
    }
    #[cfg(feature = "live")]
    {
        let secret = if with_token {
            read_stdin_token(&mut std::io::stdin().lock())?
        } else if ctx.interaction.interactive {
            ask_for_token(&target.host)?
        } else {
            return Err(CmdError::usage(
                "There's no terminal to ask for a token.\nPipe it in with --with-token, for example: pass show github | review-buddy auth login --with-token",
            ));
        };
        let store = crate::providers::system_store();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let text = runtime.block_on(sign_in(&target, secret, store.as_ref()))?;
        super::probe::forget(ctx, target.kind, &target.host);
        output::print(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_token_is_trimmed_and_must_not_be_empty() {
        let secret = read_stdin_token(&mut "  ghp_abc\n".as_bytes()).unwrap();
        assert_eq!(secret.expose(), "ghp_abc");
        let err = read_stdin_token(&mut "\n".as_bytes()).unwrap_err();
        assert_eq!(err.exit().code(), 2);
        assert!(err.to_string().contains("--with-token"));
    }
}

#[cfg(all(test, feature = "live"))]
mod live_tests {
    use super::*;
    use rb_core::ForgeKind;
    use rb_platform::MemorySecretStore;
    use serde_json::json;
    use wiremock::matchers::{header, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TOKEN: &str = "ghp_unit_test_secret_value";

    fn target(server: &MockServer, kind: ForgeKind) -> Target {
        Target {
            host: "ghe.test".into(),
            kind,
            api_url: Some(server.uri()),
            source: Some("work".into()),
            auth: None,
            token_command: None,
        }
    }

    async fn github(status: u16) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(path("/user"))
            .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("x-oauth-scopes", "repo, read:org")
                    .set_body_json(json!({"login": "smorris", "name": null})),
            )
            .mount(&server)
            .await;
        Mock::given(path("/rate_limit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "resources": {"core": {"limit": 5000, "remaining": 1, "reset": 1},
                              "graphql": {"limit": 5000, "remaining": 1, "reset": 1}}
            })))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn a_working_token_is_stored_and_the_message_names_user_and_scopes() {
        let server = github(200).await;
        let store = MemorySecretStore::new();
        let text = sign_in(
            &target(&server, ForgeKind::GitHub),
            Secret::new(TOKEN),
            &store,
        )
        .await
        .unwrap();
        assert!(text.starts_with("✓ Signed in to ghe.test as smorris · scopes repo, read:org\n"));
        assert!(text.contains("review-buddy/ghe.test"));
        assert!(text.contains("auth = \"token\""));
        assert_eq!(store.get("ghe.test").unwrap().unwrap().expose(), TOKEN);
        assert!(!text.contains(TOKEN));
    }

    #[tokio::test]
    async fn a_rejected_token_is_not_stored_and_says_what_to_do() {
        let server = github(401).await;
        let store = MemorySecretStore::new();
        let err = sign_in(
            &target(&server, ForgeKind::GitHub),
            Secret::new(TOKEN),
            &store,
        )
        .await
        .unwrap_err();
        assert_eq!(err.exit().code(), 4);
        assert!(err.to_string().contains("rejected that token"));
        assert!(!err.to_string().contains(TOKEN));
        assert!(store.get("ghe.test").unwrap().is_none());
    }

    #[tokio::test]
    async fn gitlab_tokens_are_tested_with_the_private_token_header() {
        let server = MockServer::start().await;
        Mock::given(path("/user"))
            .and(header("private-token", "glpat-unit"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"username": "sam", "name": null})),
            )
            .mount(&server)
            .await;
        Mock::given(path("/personal_access_tokens/self"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"scopes": ["api", "read_user"], "expires_at": null})),
            )
            .mount(&server)
            .await;
        let store = MemorySecretStore::new();
        let mut t = target(&server, ForgeKind::GitLab);
        t.auth = Some(crate::config::AuthSetting::Token);
        let text = sign_in(&t, Secret::new("glpat-unit"), &store)
            .await
            .unwrap();
        assert!(text.contains("as sam · scopes api, read_user"));
        assert!(!text.contains("doesn't use it yet"));
        assert!(store.get("ghe.test").unwrap().is_some());
    }
}
