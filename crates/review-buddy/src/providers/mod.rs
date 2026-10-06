//! Builds a provider for each configured source, and runs the TUI's live loads.
//!
//! Tokens are read when a provider is first built and held in memory only: from `gh`, the OS
//! keyring, an `env:VAR` reference or a `token_command`, never from `config.toml` itself.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rb_core::{Error, ForgeKind, Provider, Source, SourceId};
use rb_github::{Auth, AuthError, GithubClient, GithubProvider, TokenOrigin};
use rb_gitlab::{GitlabClient, GitlabProvider};
use rb_platform::auth::AuthMode;
use rb_platform::{CommandRunner, SecretStore, SystemRunner};

use crate::app::SourceFailure;
use crate::config::{AuthSetting, Config};

mod live;
pub mod probe;
pub mod refresh;

pub use live::Live;

pub type EnvLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// What resolving a token touches outside the process. Injected so tests spawn nothing.
#[derive(Clone)]
pub struct Deps {
    pub runner: Arc<dyn CommandRunner + Send + Sync>,
    pub store: Arc<dyn SecretStore>,
    pub getenv: EnvLookup,
}

impl Deps {
    pub fn system() -> Self {
        Self {
            runner: Arc::new(SystemRunner),
            store: system_store(),
            getenv: Arc::new(|key| std::env::var(key).ok()),
        }
    }
}

/// The OS keyring.
///
/// Test-only seam: when `REVIEW_BUDDY_TEST_KEYRING` is set, an in-memory store is used instead,
/// seeded from its value as `host=token,host=token`. It lets the binary's tests run without a
/// keyring and never touches the real one.
pub fn system_store() -> Arc<dyn SecretStore> {
    match std::env::var("REVIEW_BUDDY_TEST_KEYRING") {
        Ok(seed) => Arc::new(memory_store_from(&seed)),
        Err(_) => Arc::new(rb_platform::KeyringStore::new()),
    }
}

fn memory_store_from(seed: &str) -> rb_platform::MemorySecretStore {
    let store = rb_platform::MemorySecretStore::new();
    for pair in seed.split(',') {
        if let Some((host, token)) = pair.split_once('=') {
            let _ = store.set(host.trim(), &rb_platform::Secret::new(token.trim()));
        }
    }
    store
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("There's no source called {0}.")]
    UnknownSource(String),
    #[error("{0}")]
    Auth(AuthError),
    #[error("{0}")]
    Client(Error),
    #[error("{0}")]
    GitlabAuth(rb_gitlab::AuthError),
}

impl ProviderError {
    pub fn failure(&self, host: &str) -> SourceFailure {
        match self {
            Self::Auth(AuthError::NoToken { .. }) => SourceFailure::sign_in(host),
            Self::Auth(AuthError::Unavailable { reason, .. }) => {
                let mut failure = SourceFailure::sign_in(host);
                failure.summary = format!("Couldn't read the token for {host}: {reason}.");
                failure
            }
            Self::Client(error) => SourceFailure::from_error(error, host),
            Self::GitlabAuth(rb_gitlab::AuthError::NoToken { .. }) => {
                let mut failure = SourceFailure::sign_in(host);
                failure.next_step = format!(
                    "Run glab auth login --hostname {host}, or review-buddy auth login --host {host}."
                );
                failure
            }
            Self::GitlabAuth(rb_gitlab::AuthError::Unavailable { reason, .. }) => {
                let mut failure = SourceFailure::sign_in(host);
                failure.summary = format!("Couldn't read the token for {host}: {reason}.");
                failure
            }
            Self::UnknownSource(_) => SourceFailure::unavailable(
                self.to_string(),
                "Check the source's name in config.toml.",
            ),
        }
    }
}

struct Entry {
    source: Source,
    api_url: Option<String>,
    token_command: Option<String>,
    auth: AuthMode,
}

/// One provider per enabled `[[source]]`, built on first use and kept.
pub struct Factory {
    entries: Vec<Entry>,
    deps: Deps,
    built: Mutex<HashMap<SourceId, Arc<dyn Provider>>>,
}

impl Factory {
    pub fn from_config(config: &Config, deps: Deps) -> Self {
        let entries = config
            .sources
            .iter()
            .filter(|s| s.enabled)
            .map(|s| Entry {
                source: crate::config::source_from_config(s),
                api_url: s.api_url.clone(),
                token_command: s.token_command.clone(),
                auth: match &s.auth {
                    None | Some(AuthSetting::Cli) => AuthMode::Cli,
                    Some(AuthSetting::Token) => AuthMode::Token,
                    Some(AuthSetting::Env(var)) => AuthMode::Env(var.clone()),
                    Some(AuthSetting::Command) => AuthMode::Command,
                },
            })
            .collect();
        Self {
            entries,
            deps,
            built: Mutex::default(),
        }
    }

    pub fn sources(&self) -> Vec<Source> {
        self.entries.iter().map(|e| e.source.clone()).collect()
    }

    /// The provider for one source. Resolves the token the first time; blocking, so call it
    /// off the async threads.
    pub fn provider(&self, id: &SourceId) -> Result<Arc<dyn Provider>, ProviderError> {
        if let Some(found) = self.lock().get(id) {
            return Ok(Arc::clone(found));
        }
        let entry = self
            .entry(id)
            .ok_or_else(|| ProviderError::UnknownSource(id.to_string()))?;
        let provider = self.build(entry)?;
        self.lock().insert(id.clone(), Arc::clone(&provider));
        Ok(provider)
    }

    /// How the source's token is configured to be found, whether or not that works.
    pub fn auth_mode(&self, id: &SourceId) -> Option<AuthMode> {
        self.entry(id).map(|e| e.auth.clone())
    }

    /// A signed-in HTTP client for one GitHub source, and where its token came from. Resolves
    /// the token afresh each call; blocking. The token stays inside the client.
    pub fn github_client(
        &self,
        id: &SourceId,
    ) -> Result<(GithubClient, TokenOrigin), ProviderError> {
        let entry = self
            .entry(id)
            .ok_or_else(|| ProviderError::UnknownSource(id.to_string()))?;
        self.client_for(entry)
    }

    fn entry(&self, id: &SourceId) -> Option<&Entry> {
        self.entries.iter().find(|e| &e.source.id == id)
    }

    /// A signed-in HTTP client for one GitLab source, and where its token came from. Tokens
    /// read from `glab` go out as `Authorization: Bearer` (they may be OAuth tokens); all others
    /// use `PRIVATE-TOKEN`. Resolves the token afresh each call; blocking.
    pub fn gitlab_client(
        &self,
        id: &SourceId,
    ) -> Result<(GitlabClient, rb_gitlab::TokenOrigin), ProviderError> {
        let entry = self
            .entry(id)
            .ok_or_else(|| ProviderError::UnknownSource(id.to_string()))?;
        self.gitlab_client_for(entry)
    }

    fn gitlab_client_for(
        &self,
        entry: &Entry,
    ) -> Result<(GitlabClient, rb_gitlab::TokenOrigin), ProviderError> {
        let source = &entry.source;
        let mut auth = rb_gitlab::Auth::new(&source.host, entry.auth.clone());
        if let Some(command) = &entry.token_command {
            auth = auth.with_token_command(command);
        }
        let getenv = &self.deps.getenv;
        let token = auth
            .resolve(&*self.deps.runner, &*self.deps.store, |key| getenv(key))
            .map_err(ProviderError::GitlabAuth)?;
        let mut client = GitlabClient::new(&source.host, entry.api_url.as_deref(), token.secret)
            .map_err(ProviderError::Client)?;
        if token.origin == rb_gitlab::TokenOrigin::GlabCli {
            client = client.with_bearer();
        }
        Ok((client, token.origin))
    }

    fn client_for(&self, entry: &Entry) -> Result<(GithubClient, TokenOrigin), ProviderError> {
        let source = &entry.source;
        let mut auth = Auth::new(&source.host, entry.auth.clone());
        if let Some(command) = &entry.token_command {
            auth = auth.with_token_command(command);
        }
        let getenv = &self.deps.getenv;
        let token = auth
            .resolve(&*self.deps.runner, &*self.deps.store, |key| getenv(key))
            .map_err(ProviderError::Auth)?;
        let client = GithubClient::new(&source.host, entry.api_url.as_deref(), token.secret)
            .map_err(ProviderError::Client)?;
        Ok((client, token.origin))
    }

    fn build(&self, entry: &Entry) -> Result<Arc<dyn Provider>, ProviderError> {
        if entry.source.kind == ForgeKind::GitLab {
            let (client, _) = self.gitlab_client_for(entry)?;
            return Ok(Arc::new(
                GitlabProvider::new(client).with_source_id(entry.source.id.clone()),
            ));
        }
        let (client, _) = self.client_for(entry)?;
        Ok(Arc::new(
            GithubProvider::new(client).with_source_id(entry.source.id.clone()),
        ))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SourceId, Arc<dyn Provider>>> {
        self.built.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl std::fmt::Debug for Factory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Factory")
            .field("sources", &self.entries.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use rb_platform::{CommandOutput, MemorySecretStore, PlatformError, Secret};

    use super::*;

    #[test]
    fn the_test_keyring_seed_reads_host_token_pairs() {
        let store = memory_store_from("a.test=tok1, b.test=tok2,junk");
        assert_eq!(store.get("a.test").unwrap().unwrap().expose(), "tok1");
        assert_eq!(store.get("b.test").unwrap().unwrap().expose(), "tok2");
        assert!(memory_store_from("").get("a.test").unwrap().is_none());
    }

    #[derive(Default)]
    struct FakeRunner {
        stdout: Option<String>,
        calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl CommandRunner for FakeRunner {
        fn run(
            &self,
            program: &str,
            args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            self.calls
                .lock()
                .unwrap()
                .push((program.into(), args.iter().map(|a| a.to_string()).collect()));
            match &self.stdout {
                Some(out) => Ok(CommandOutput {
                    success: true,
                    stdout: out.clone(),
                    stderr: String::new(),
                }),
                None => Err(PlatformError::CommandFailed {
                    program: program.into(),
                }),
            }
        }
    }

    fn deps(runner: Arc<FakeRunner>, env: &[(&str, &str)]) -> (Deps, Arc<MemorySecretStore>) {
        let store = Arc::new(MemorySecretStore::new());
        let env: HashMap<String, String> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let deps = Deps {
            runner,
            store: store.clone(),
            getenv: Arc::new(move |k| env.get(k).cloned()),
        };
        (deps, store)
    }

    fn config(text: &str) -> Config {
        toml::from_str(text).unwrap()
    }

    const GH: &str = r#"
[[source]]
name = "work"
kind = "github"
host = "github.com"
"#;

    fn id(name: &str) -> SourceId {
        SourceId::new(name)
    }

    #[test]
    fn cli_auth_reuses_gh_for_the_host_and_keeps_the_provider() {
        let runner = Arc::new(FakeRunner {
            stdout: Some("gho_abc\n".into()),
            ..FakeRunner::default()
        });
        let (deps, _) = deps(runner.clone(), &[]);
        let factory = Factory::from_config(&config(GH), deps);
        let first = factory.provider(&id("work")).unwrap();
        assert_eq!(first.kind(), ForgeKind::GitHub);
        factory.provider(&id("work")).unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, ["auth", "token", "--hostname", "github.com"]);
    }

    #[test]
    fn missing_sign_in_is_a_calm_sign_in_failure() {
        let (deps, _) = deps(Arc::new(FakeRunner::default()), &[]);
        let factory = Factory::from_config(&config(GH), deps);
        let err = factory.provider(&id("work")).err().unwrap();
        assert!(matches!(
            err,
            ProviderError::Auth(AuthError::NoToken { .. })
        ));
        let failure = err.failure("github.com");
        assert!(failure.next_step.contains("gh auth login"));
    }

    #[test]
    fn keyring_env_and_command_modes() {
        let text = r#"
[[source]]
name = "kr"
kind = "github"
host = "github.com"
auth = "token"

[[source]]
name = "ev"
kind = "github"
host = "ghe.example.com"
auth = "env:GHE_TOKEN"
api_url = "https://ghe.example.com/api/v3"

[[source]]
name = "cmd"
kind = "github"
host = "ghe2.example.com"
auth = "command"
token_command = "pass show gh"
"#;
        let runner = Arc::new(FakeRunner {
            stdout: Some("ghp_cmd\n".into()),
            ..FakeRunner::default()
        });
        let (deps, store) = deps(runner.clone(), &[("GHE_TOKEN", "ghp_env")]);
        let factory = Factory::from_config(&config(text), deps);
        assert!(factory.provider(&id("kr")).is_err());
        store.set("github.com", &Secret::new("ghp_kr")).unwrap();
        assert!(factory.provider(&id("kr")).is_ok());
        assert!(factory.provider(&id("ev")).is_ok());
        assert!(factory.provider(&id("cmd")).is_ok());
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].1.iter().any(|a| a.contains("pass show gh")));
    }

    #[test]
    fn a_missing_env_var_explains_itself_without_the_token() {
        let text = r#"
[[source]]
name = "ev"
kind = "github"
host = "github.com"
auth = "env:NOPE_TOKEN"
"#;
        let (deps, _) = deps(Arc::new(FakeRunner::default()), &[]);
        let factory = Factory::from_config(&config(text), deps);
        let err = factory.provider(&id("ev")).err().unwrap();
        let failure = err.failure("github.com");
        assert!(failure.summary.contains("NOPE_TOKEN"));
    }

    #[test]
    fn gitlab_sources_build_a_provider_using_glab() {
        let text = r#"
[[source]]
name = "lab"
kind = "gitlab"
host = "gitlab.example.com"
api_url = "http://127.0.0.1:9/api/v4"
"#;
        let runner = Arc::new(FakeRunner {
            stdout: Some("glpat-abc\n".into()),
            ..FakeRunner::default()
        });
        let (deps, _) = deps(runner.clone(), &[]);
        let factory = Factory::from_config(&config(text), deps);
        assert_eq!(factory.sources().len(), 1);
        let provider = factory.provider(&id("lab")).unwrap();
        assert_eq!(provider.kind(), ForgeKind::GitLab);
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].0, "glab");
        assert_eq!(
            calls[0].1,
            ["config", "get", "token", "--host", "gitlab.example.com"]
        );
    }

    #[test]
    fn gitlab_without_a_token_asks_to_sign_in_with_glab() {
        let text = "[[source]]\nname = \"lab\"\nkind = \"gitlab\"\nhost = \"gitlab.com\"\n";
        let (deps, _) = deps(Arc::new(FakeRunner::default()), &[]);
        let factory = Factory::from_config(&config(text), deps);
        let err = factory.provider(&id("lab")).err().unwrap();
        assert!(matches!(
            err,
            ProviderError::GitlabAuth(rb_gitlab::AuthError::NoToken { .. })
        ));
        assert!(err
            .failure("gitlab.com")
            .next_step
            .contains("glab auth login"));
    }

    #[test]
    fn disabled_and_unknown_sources_are_not_built() {
        let text = format!("{GH}\n[[source]]\nname = \"off\"\nkind = \"github\"\nhost = \"github.com\"\nenabled = false\n");
        let (deps, _) = deps(Arc::new(FakeRunner::default()), &[]);
        let factory = Factory::from_config(&config(&text), deps);
        assert_eq!(factory.sources().len(), 1);
        assert!(matches!(
            factory.provider(&id("off")),
            Err(ProviderError::UnknownSource(_))
        ));
    }

    #[test]
    fn enterprise_hosts_and_api_url_overrides_build() {
        let text = r#"
[[source]]
name = "ghe"
kind = "github"
host = "ghe.example.com"
auth = "env:T"

[[source]]
name = "stub"
kind = "github"
host = "ghe.example.com"
auth = "env:T"
api_url = "http://127.0.0.1:9/api/v3"
"#;
        let (deps, _) = deps(Arc::new(FakeRunner::default()), &[("T", "x")]);
        let factory = Factory::from_config(&config(text), deps);
        let provider = factory.provider(&id("ghe")).unwrap();
        assert_eq!(
            provider
                .web_url(&rb_core::ChangeId {
                    source_id: id("ghe"),
                    kind: ForgeKind::GitHub,
                    repo: "a/b".into(),
                    number: 1
                })
                .as_str(),
            "https://ghe.example.com/a/b/pull/1"
        );
        assert!(factory.provider(&id("stub")).is_ok());
    }
}
