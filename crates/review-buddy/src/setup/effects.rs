//! Runs the flow's [`Effect`]s: detection, token checks and org listing, and the config write.
//!
//! Each effect answers with an [`Input`] for [`Flow::apply`](super::Flow::apply). Programs, the
//! keyring and the filesystem are all injected through [`Services`], so tests use fakes and a
//! stub GitHub server named by `api_urls`.

use std::collections::HashMap;
use std::sync::Arc;

use rb_core::{Error, ForgeKind};
use rb_github::GithubClient;
use rb_platform::auth::{token_from_cli, CliTool};
use rb_platform::{CommandRunner, Secret, SecretStore, SystemRunner};

use super::detect::{self, tool_name, Roots};
use super::flow::{required_scopes, Credential, Effect, Input, ProbeInfo};
use super::write;
use crate::config::Config;

/// What the effects touch outside the process.
#[derive(Clone)]
pub struct Services {
    pub runner: Arc<dyn CommandRunner + Send + Sync>,
    pub store: Arc<dyn SecretStore>,
    pub roots: Roots,
    /// `api_url` per host, kept from an earlier config so a re-run doesn't lose it.
    pub api_urls: HashMap<String, String>,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}

impl Services {
    pub fn system(env: &dyn rb_paths::Env, config: &Config) -> Self {
        Self {
            runner: Arc::new(SystemRunner),
            store: Arc::new(rb_platform::KeyringStore::new()),
            roots: Roots::from_env(env),
            api_urls: api_urls(config),
        }
    }
}

/// The `api_url` of every configured source that sets one, by host.
pub fn api_urls(config: &Config) -> HashMap<String, String> {
    config
        .sources
        .iter()
        .filter_map(|s| s.api_url.clone().map(|url| (s.host.clone(), url)))
        .collect()
}

/// Runs one effect to completion.
pub async fn run_effect(services: &Arc<Services>, effect: Effect) -> Input {
    match effect {
        Effect::Detect => {
            let services = Arc::clone(services);
            let found = tokio::task::spawn_blocking(move || {
                detect::detect(services.runner.as_ref(), &services.roots)
            })
            .await
            .unwrap_or_default();
            Input::Detected(found)
        }
        Effect::Probe {
            host,
            kind,
            credential,
        } => {
            let result = probe(services, &host, kind, credential).await;
            Input::Probed { host, result }
        }
        Effect::Write(plan) => Input::Written(
            tokio::task::spawn_blocking(move || write::write(&plan).map_err(|e| e.to_string()))
                .await
                .unwrap_or_else(|_| Err("The config couldn't be written.".to_string())),
        ),
    }
}

/// Checks a sign-in or a pasted token. A pasted token that works is kept in the OS keyring
/// under the host name; it goes nowhere else.
pub async fn probe(
    services: &Arc<Services>,
    host: &str,
    kind: ForgeKind,
    credential: Credential,
) -> Result<ProbeInfo, String> {
    let (secret, pasted) = match credential {
        Credential::Pasted(secret) => (secret, true),
        Credential::Cli => (cli_token(services, host, kind).await?, false),
    };
    match kind {
        ForgeKind::GitHub => probe_github(services, host, secret, pasted).await,
        ForgeKind::GitLab => probe_gitlab(services, host, secret, pasted).await,
    }
}

fn cli_tool(kind: ForgeKind) -> CliTool {
    match kind {
        ForgeKind::GitHub => CliTool::Gh,
        ForgeKind::GitLab => CliTool::Glab,
    }
}

async fn cli_token(
    services: &Arc<Services>,
    host: &str,
    kind: ForgeKind,
) -> Result<Secret, String> {
    let tool = cli_tool(kind);
    let (runner, name) = (Arc::clone(&services.runner), host.to_string());
    let token =
        tokio::task::spawn_blocking(move || token_from_cli(tool, &name, runner.as_ref())).await;
    token.ok().and_then(Result::ok).ok_or_else(|| {
        let cli = tool_name(tool);
        format!(
            "{cli} didn't hand over a token for {host}. Run {cli} auth login --hostname {host}, or paste a token instead."
        )
    })
}

async fn probe_github(
    services: &Arc<Services>,
    host: &str,
    secret: Secret,
    pasted: bool,
) -> Result<ProbeInfo, String> {
    let api_url = services.api_urls.get(host).map(String::as_str);
    let client = GithubClient::new(host, api_url, secret.clone()).map_err(|e| e.to_string())?;
    let report = client
        .test_token()
        .await
        .map_err(|e| friendly(&e, host, pasted))?;
    if pasted {
        keep_token(services, host, secret).await?;
    }
    let mut hints = scope_hints(&report.scopes, pasted);
    hints.extend(report.sso_hint.clone());
    let orgs = match client.list_orgs().await {
        Ok(orgs) => orgs,
        Err(_) => {
            hints.push(
                "Couldn't list your organisations, so everything you can see is included."
                    .to_string(),
            );
            Vec::new()
        }
    };
    Ok(ProbeInfo {
        login: report.login,
        scopes: report.scopes,
        orgs,
        hints,
        checked: true,
    })
}

/// Hook for GitLab. `rb-gitlab` isn't wired in yet, so the sign-in is taken on trust: a pasted
/// token is still kept in the keyring, and the group list stays empty. Replace the body with a
/// token test (`api`, `read_user`) and a group listing once the provider exists.
async fn probe_gitlab(
    services: &Arc<Services>,
    host: &str,
    secret: Secret,
    pasted: bool,
) -> Result<ProbeInfo, String> {
    if pasted {
        keep_token(services, host, secret).await?;
    }
    Ok(ProbeInfo {
        hints: vec![
            "GitLab sign-ins aren't checked yet, so this one is taken as it is.".to_string(),
        ],
        ..ProbeInfo::default()
    })
}

async fn keep_token(services: &Arc<Services>, host: &str, secret: Secret) -> Result<(), String> {
    let store = Arc::clone(&services.store);
    let account = host.to_string();
    let saved = tokio::task::spawn_blocking(move || store.set(&account, &secret))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
    saved.map_err(|reason| {
        format!(
            "Couldn't save the token to the OS keyring ({reason}). You can keep it in an environment variable instead: set GITHUB_TOKEN and use auth = \"env:GITHUB_TOKEN\" in config.toml."
        )
    })
}

pub(crate) fn scope_hints(scopes: &[String], pasted: bool) -> Vec<String> {
    if scopes.is_empty() {
        return if pasted {
            vec![
                "This token doesn't list its scopes (fine-grained tokens don't), so they can't be checked here."
                    .to_string(),
            ]
        } else {
            Vec::new()
        };
    }
    let has = |want: &str| {
        scopes
            .iter()
            .any(|s| s == want || (want == "read:org" && (s == "write:org" || s == "admin:org")))
    };
    let missing: Vec<_> = required_scopes(ForgeKind::GitHub)
        .iter()
        .filter(|want| !has(want))
        .copied()
        .collect();
    if missing.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "This token is missing {}. Add it in your GitHub token settings if some reviews don't show up.",
            missing.join(" and ")
        )]
    }
}

fn friendly(error: &Error, host: &str, pasted: bool) -> String {
    match error {
        Error::Unauthorized { .. } if pasted => {
            "That token was rejected. Check that you copied all of it, then try again.".to_string()
        }
        Error::Unauthorized { .. } => format!(
            "The token from gh was rejected. Run gh auth login --hostname {host}, or paste a token instead."
        ),
        Error::Network { .. } => {
            format!("Couldn't reach {host}. Check your connection, then try again.")
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::flow::Plan;
    use rb_platform::{CommandOutput, MemorySecretStore, PlatformError};
    use serde_json::json;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct GhRunner(Option<&'static str>);

    impl CommandRunner for GhRunner {
        fn run(
            &self,
            program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            match (program, self.0) {
                ("gh", Some(token)) => Ok(CommandOutput {
                    success: true,
                    stdout: format!("{token}\n"),
                    stderr: String::new(),
                }),
                _ => Err(PlatformError::Spawn {
                    program: program.into(),
                    reason: "not found".into(),
                }),
            }
        }
    }

    fn services(
        server: &MockServer,
        runner: GhRunner,
        store: Arc<MemorySecretStore>,
    ) -> Arc<Services> {
        let t = tempfile::tempdir().unwrap();
        Arc::new(Services {
            runner: Arc::new(runner),
            store,
            roots: Roots {
                git_configs: Vec::new(),
                scan_root: t.path().join("none"),
            },
            api_urls: HashMap::from([("ghe.test".to_string(), server.uri())]),
        })
    }

    async fn stub(server: &MockServer, status: u16, scopes: &str) {
        Mock::given(path("/user"))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("x-oauth-scopes", scopes)
                    .set_body_json(json!({"login": "octo", "name": "Octo"})),
            )
            .mount(server)
            .await;
        Mock::given(path("/rate_limit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "resources": {"core": {"limit": 5000, "remaining": 4999, "reset": 1700000100}}
            })))
            .mount(server)
            .await;
        Mock::given(path("/user/orgs"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!([{"login": "liminal-hq"}, {"login": "acme"}])),
            )
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_pasted_token_is_tested_then_kept_in_the_keyring() {
        let server = MockServer::start().await;
        stub(&server, 200, "repo, read:org").await;
        let store = Arc::new(MemorySecretStore::new());
        let s = services(&server, GhRunner(None), store.clone());
        let info = probe(
            &s,
            "ghe.test",
            ForgeKind::GitHub,
            Credential::Pasted(Secret::new("ghp_secret")),
        )
        .await
        .unwrap();
        assert_eq!(info.login, "octo");
        assert_eq!(info.orgs, ["liminal-hq", "acme"]);
        assert!(info.hints.is_empty(), "{:?}", info.hints);
        assert!(info.checked);
        assert_eq!(
            store.get("ghe.test").unwrap().unwrap().expose(),
            "ghp_secret"
        );
        let seen = server.received_requests().await.unwrap();
        assert!(seen.iter().all(|r| r
            .headers
            .get("authorization")
            .is_some_and(|v| v.to_str().unwrap() == "Bearer ghp_secret")));
    }

    #[tokio::test]
    async fn a_rejected_token_is_not_saved_and_the_message_says_what_to_do() {
        let server = MockServer::start().await;
        stub(&server, 401, "").await;
        let store = Arc::new(MemorySecretStore::new());
        let s = services(&server, GhRunner(None), store.clone());
        let err = probe(
            &s,
            "ghe.test",
            ForgeKind::GitHub,
            Credential::Pasted(Secret::new("ghp_bad")),
        )
        .await
        .unwrap_err();
        assert!(
            err.contains("rejected") && err.contains("try again"),
            "{err}"
        );
        assert!(!err.contains("ghp_bad"));
        assert!(store.get("ghe.test").unwrap().is_none());
    }

    #[tokio::test]
    async fn missing_scopes_are_hinted_without_failing() {
        let server = MockServer::start().await;
        stub(&server, 200, "repo").await;
        let s = services(&server, GhRunner(None), Arc::new(MemorySecretStore::new()));
        let info = probe(
            &s,
            "ghe.test",
            ForgeKind::GitHub,
            Credential::Pasted(Secret::new("t")),
        )
        .await
        .unwrap();
        assert_eq!(info.hints.len(), 1);
        assert!(info.hints[0].contains("read:org"));
    }

    #[tokio::test]
    async fn reusing_gh_reads_its_token_and_keeps_nothing() {
        let server = MockServer::start().await;
        stub(&server, 200, "repo, read:org").await;
        let store = Arc::new(MemorySecretStore::new());
        let s = services(&server, GhRunner(Some("gho_x")), store.clone());
        let info = probe(&s, "ghe.test", ForgeKind::GitHub, Credential::Cli)
            .await
            .unwrap();
        assert_eq!(info.login, "octo");
        assert!(store.get("ghe.test").unwrap().is_none());
    }

    #[tokio::test]
    async fn gh_without_a_token_points_at_gh_auth_login() {
        let server = MockServer::start().await;
        let s = services(&server, GhRunner(None), Arc::new(MemorySecretStore::new()));
        let err = probe(&s, "ghe.test", ForgeKind::GitHub, Credential::Cli)
            .await
            .unwrap_err();
        assert!(err.contains("gh auth login --hostname ghe.test"), "{err}");
    }

    #[tokio::test]
    async fn an_unreachable_host_is_calm() {
        let store = Arc::new(MemorySecretStore::new());
        let t = tempfile::tempdir().unwrap();
        let s = Arc::new(Services {
            runner: Arc::new(GhRunner(None)),
            store,
            roots: Roots {
                git_configs: Vec::new(),
                scan_root: t.path().to_path_buf(),
            },
            api_urls: HashMap::from([("ghe.test".to_string(), "http://127.0.0.1:1".to_string())]),
        });
        let err = probe(
            &s,
            "ghe.test",
            ForgeKind::GitHub,
            Credential::Pasted(Secret::new("t")),
        )
        .await
        .unwrap_err();
        assert!(err.starts_with("Couldn't reach ghe.test"), "{err}");
    }

    struct BrokenStore;

    impl SecretStore for BrokenStore {
        fn get(&self, _: &str) -> Result<Option<Secret>, PlatformError> {
            Ok(None)
        }
        fn set(&self, _: &str, _: &Secret) -> Result<(), PlatformError> {
            Err(PlatformError::StoreUnavailable("no secret service".into()))
        }
        fn delete(&self, _: &str) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_keyring_that_wont_open_suggests_an_env_var() {
        let server = MockServer::start().await;
        stub(&server, 200, "repo, read:org").await;
        let mut s = services(&server, GhRunner(None), Arc::new(MemorySecretStore::new()));
        Arc::get_mut(&mut s).unwrap().store = Arc::new(BrokenStore);
        let err = probe(
            &s,
            "ghe.test",
            ForgeKind::GitHub,
            Credential::Pasted(Secret::new("tok")),
        )
        .await
        .unwrap_err();
        assert!(err.contains("keyring") && err.contains("env:"), "{err}");
        assert!(!err.contains("tok\""));
    }

    #[tokio::test]
    async fn gitlab_tokens_are_kept_and_noted_as_unchecked() {
        let server = MockServer::start().await;
        let store = Arc::new(MemorySecretStore::new());
        let s = services(&server, GhRunner(None), store.clone());
        let info = probe(
            &s,
            "gitlab.work.ca",
            ForgeKind::GitLab,
            Credential::Pasted(Secret::new("glpat-1")),
        )
        .await
        .unwrap();
        assert!(!info.checked && info.orgs.is_empty());
        assert_eq!(
            store.get("gitlab.work.ca").unwrap().unwrap().expose(),
            "glpat-1"
        );
    }

    #[tokio::test]
    async fn detect_and_write_effects_answer_with_inputs() {
        let server = MockServer::start().await;
        let s = services(&server, GhRunner(None), Arc::new(MemorySecretStore::new()));
        let Input::Detected(d) = run_effect(&s, Effect::Detect).await else {
            panic!("expected detection");
        };
        assert!(d.hosts.is_empty());
        let t = tempfile::tempdir().unwrap();
        let plan = Plan {
            target: t.path().join("config.toml"),
            theme: "dusk".into(),
            jax: true,
            sources: Vec::new(),
            replace: false,
        };
        let Input::Written(result) = run_effect(&s, Effect::Write(plan)).await else {
            panic!("expected a write result");
        };
        assert!(result.unwrap_err().contains("nothing to write"));
    }
}
