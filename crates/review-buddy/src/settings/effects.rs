//! Runs Settings' [`Effect`]s: reading the layered config, testing a token, and writing
//! changes to the write target. Everything outside the process is injected through
//! [`Services`], so tests use fakes and a stub GitHub named by `api_url`.

use std::sync::Arc;

use rb_core::ForgeKind;
use rb_paths::{ConfigOrigin, PathsReport};

use super::edit::{self, EditError};
use super::state::{Change, Effect, Input, Origin, Saved, Snapshot, SourceRow, TestInfo};
use crate::config::{AuthSetting, Config, Kind, SourceConfig};
use crate::setup::{self, AuthKind, SourceSpec};

/// The config as it is on disk right now.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub snapshot: Snapshot,
    pub config: Config,
}

type Load = Arc<dyn Fn() -> Result<Loaded, String> + Send + Sync>;

/// What the effects touch outside the process.
#[derive(Clone)]
pub struct Services {
    /// Detection, the keyring and token checks, shared with first run.
    pub setup: Arc<setup::Services>,
    load: Load,
    #[cfg(feature = "live")]
    deps: crate::providers::Deps,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}

impl Services {
    pub fn new(
        setup: Arc<setup::Services>,
        load: impl Fn() -> Result<Loaded, String> + Send + Sync + 'static,
    ) -> Self {
        #[cfg(feature = "live")]
        let deps = crate::providers::Deps {
            runner: Arc::clone(&setup.runner),
            store: Arc::clone(&setup.store),
            getenv: Arc::new(|key| std::env::var(key).ok()),
        };
        Self {
            setup,
            load: Arc::new(load),
            #[cfg(feature = "live")]
            deps,
        }
    }

    /// Reads the config the way the interface does at launch, from the real environment.
    pub fn system(args: crate::cli::GlobalArgs, setup: Arc<setup::Services>) -> Self {
        Self::new(setup, move || {
            let ctx = crate::cmd::context::Context::build(
                args.clone(),
                crate::cmd::context::Terminal::detect(),
            )
            .map_err(|e| e.to_string())?;
            if let Some(problem) = ctx.config_problem() {
                return Err(problem.to_string());
            }
            Ok(Loaded {
                snapshot: snapshot(&ctx.paths, &ctx.config),
                config: ctx.config.clone(),
            })
        })
    }
}

/// The sources in `config` and where they come from.
///
/// Later layers replace earlier `[[source]]` lists, so the file that counts is the last one
/// that defines any.
pub fn snapshot(paths: &PathsReport, config: &Config) -> Snapshot {
    let defines_sources = |path: &std::path::Path| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|table| {
                table
                    .get("source")
                    .and_then(|v| v.as_array().map(|a| !a.is_empty()))
            })
            .unwrap_or(false)
    };
    let home = paths
        .config_files
        .iter()
        .rfind(|f| f.exists && defines_sources(&f.path));
    let origin = home
        .filter(|f| f.path != paths.write_target)
        .map(|f| Origin {
            path: f.path.clone(),
            label: origin_label(&f.path, f.origin),
        });
    Snapshot {
        rows: config.sources.iter().map(row).collect(),
        write_target: paths.write_target.clone(),
        editable: origin.is_none(),
        origin,
    }
}

fn origin_label(path: &std::path::Path, origin: ConfigOrigin) -> String {
    match (origin, path.file_name()) {
        (ConfigOrigin::DropIn, Some(name)) => format!("from config.d/{}", name.to_string_lossy()),
        _ => format!("from {}", path.display()),
    }
}

fn row(s: &SourceConfig) -> SourceRow {
    let (kind, owners, repos) = match s.kind {
        Kind::Github => (ForgeKind::GitHub, &s.scope.orgs, &s.scope.repos),
        Kind::Gitlab => (ForgeKind::GitLab, &s.scope.groups, &s.scope.projects),
    };
    SourceRow {
        spec: SourceSpec {
            name: s.name.clone(),
            kind,
            host: s.host.clone(),
            api_url: s.api_url.clone(),
            auth: match &s.auth {
                None | Some(AuthSetting::Cli) => AuthKind::Cli,
                Some(AuthSetting::Token) => AuthKind::Token,
                Some(AuthSetting::Env(var)) => AuthKind::Env(var.clone()),
                Some(AuthSetting::Command) => {
                    AuthKind::Command(s.token_command.clone().unwrap_or_default())
                }
            },
            scope_user: s.scope.user,
            owners: owners.clone(),
        },
        enabled: s.enabled,
        in_all: s.in_all,
        include_drafts: s.include_drafts,
        tag_colour: s.tag_colour.clone(),
        repos: repos.clone(),
    }
}

/// The answer when this session has no way to run `effect`.
pub fn unavailable(effect: Effect) -> Input {
    let why =
        "Settings can't reach your config in this session. Restart review-buddy and try again.";
    match effect {
        Effect::Load => Input::Loaded(Err(why.to_string())),
        Effect::Detect => Input::Detected(setup::Detection::default()),
        Effect::Test { name } => Input::Tested {
            name,
            result: Err(why.to_string()),
        },
        Effect::Save { .. } => Input::Saved(Err(why.to_string())),
    }
}

/// Runs one effect to completion.
pub async fn run_effect(services: &Arc<Services>, effect: Effect) -> Input {
    match effect {
        Effect::Load => {
            let load = Arc::clone(&services.load);
            let loaded = tokio::task::spawn_blocking(move || load())
                .await
                .unwrap_or_else(|_| Err("The config couldn't be read.".to_string()));
            Input::Loaded(loaded.map(|l| l.snapshot))
        }
        Effect::Detect => {
            let setup = Arc::clone(&services.setup);
            let found = tokio::task::spawn_blocking(move || {
                setup::detect::detect(setup.runner.as_ref(), &setup.roots)
            })
            .await
            .unwrap_or_default();
            Input::Detected(found)
        }
        Effect::Test { name } => {
            let result = test(services, &name).await;
            Input::Tested { name, result }
        }
        Effect::Save { target, change } => Input::Saved(save(services, &target, change).await),
    }
}

#[cfg(feature = "live")]
async fn test(services: &Arc<Services>, name: &str) -> Result<TestInfo, String> {
    use crate::cmd::auth::{check_source, State};
    use crate::providers::Factory;

    let load = Arc::clone(&services.load);
    let loaded = tokio::task::spawn_blocking(move || load())
        .await
        .map_err(|_| "The config couldn't be read.".to_string())??;
    let mut source = loaded
        .config
        .sources
        .iter()
        .find(|s| s.name == name)
        .cloned()
        .ok_or_else(|| format!("{name} isn't in your config any more."))?;
    source.enabled = true;
    let config = Config {
        sources: vec![source.clone()],
        ..Config::default()
    };
    let factory = Factory::from_config(&config, services.deps.clone());
    let checked = check_source(&source, &factory).await;
    match checked.state {
        State::SignedIn {
            user,
            scopes,
            expires,
        } => {
            let note = (checked.kind == ForgeKind::GitHub)
                .then(|| {
                    setup::effects::scope_hints(&scopes, false)
                        .into_iter()
                        .next()
                })
                .flatten();
            Ok(TestInfo {
                user,
                scopes,
                expires,
                note,
            })
        }
        State::Failed { reason, fix } => Err(if fix.is_empty() {
            format!("Couldn't sign in to {}: {reason}.", checked.host)
        } else {
            format!(
                "Couldn't sign in to {}: {reason}. To fix it, {fix}.",
                checked.host
            )
        }),
    }
}

#[cfg(not(feature = "live"))]
async fn test(_services: &Arc<Services>, _name: &str) -> Result<TestInfo, String> {
    Err("This build has no network support, so tokens can't be checked.".to_string())
}

async fn save(
    services: &Arc<Services>,
    target: &std::path::Path,
    change: Change,
) -> Result<Saved, String> {
    let target = target.to_path_buf();
    match change {
        Change::Add { spec, token } => {
            if let Some(token) = token {
                keep_token(services, &spec, token).await?;
            }
            let (path, added) = (target, spec.clone());
            write(move || edit::add(&path, &added)).await?;
            Ok(Saved {
                message: format!("Added {}.", spec.name),
                select: Some(spec.name),
            })
        }
        Change::Edit {
            before,
            after,
            token,
        } => {
            if let Some(token) = token {
                keep_token(services, &after, token).await?;
            }
            let (path, old, new) = (target, before, after.clone());
            write(move || edit::update(&path, &old, &new)).await?;
            Ok(Saved {
                message: format!("Saved {}.", after.name),
                select: Some(after.name),
            })
        }
        Change::Toggle { name, enabled } => {
            let (path, key) = (target, name.clone());
            write(move || edit::set_enabled(&path, &key, enabled)).await?;
            let message = if enabled {
                format!("{name} is on.")
            } else {
                format!("{name} is off. It stays in your config.")
            };
            Ok(Saved {
                message,
                select: Some(name),
            })
        }
        Change::Remove { spec, forget_token } => {
            let (path, key) = (target, spec.name.clone());
            write(move || edit::remove(&path, &key)).await?;
            let message = remove_message(services, &spec, forget_token).await;
            Ok(Saved {
                message,
                select: None,
            })
        }
    }
}

async fn remove_message(services: &Arc<Services>, spec: &SourceSpec, forget: bool) -> String {
    let name = &spec.name;
    if forget {
        let (store, host) = (Arc::clone(&services.setup.store), spec.host.clone());
        let deleted = tokio::task::spawn_blocking(move || store.delete(&host)).await;
        return match deleted {
            Ok(Ok(())) => format!("Removed {name} and its token from the keyring."),
            _ => format!(
                "Removed {name}, but couldn't remove its token from the keyring. Remove review-buddy/{} there yourself.",
                spec.host
            ),
        };
    }
    if spec.auth == AuthKind::Token {
        format!("Removed {name}. Its token is still in your keyring.")
    } else {
        format!("Removed {name}.")
    }
}

/// Tests a typed token against the source's own host (GitHub or GitLab, through the same
/// `host::test_token` the CLI uses) and keeps it in the keyring if it works.
#[cfg(feature = "live")]
async fn keep_token(
    services: &Arc<Services>,
    spec: &SourceSpec,
    token: rb_platform::Secret,
) -> Result<(), String> {
    use rb_core::Error;
    let target = crate::cmd::host::Target {
        host: spec.host.clone(),
        kind: spec.kind,
        api_url: spec.api_url.clone(),
        source: Some(spec.name.clone()),
        auth: None,
        token_command: None,
    };
    let host = &spec.host;
    crate::cmd::host::test_token(&target, token.clone())
        .await
        .map_err(|e| match e {
            Error::Unauthorized { .. } => {
                "That token was rejected. Check that you copied all of it, then try again."
                    .to_string()
            }
            Error::Network { .. } => {
                format!("Couldn't reach {host}. Check your connection, then try again.")
            }
            other => other.to_string(),
        })?;
    let (store, account) = (Arc::clone(&services.setup.store), host.clone());
    let saved = tokio::task::spawn_blocking(move || store.set(&account, &token))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
    saved.map_err(|reason| {
        format!("Couldn't save the token to the OS keyring ({reason}). Keep it in an environment variable instead and choose \"environment variable\" as the sign-in.")
    })
}

#[cfg(not(feature = "live"))]
async fn keep_token(
    _services: &Arc<Services>,
    _spec: &SourceSpec,
    _token: rb_platform::Secret,
) -> Result<(), String> {
    Err("This build has no network support, so a token can't be checked or saved.".to_string())
}

async fn write(job: impl FnOnce() -> Result<(), EditError> + Send + 'static) -> Result<(), String> {
    let done = tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| "The config couldn't be written. Nothing was changed.".to_string())?;
    done.map_err(|e| {
        let text = e.to_string();
        format!("{}. Nothing was changed.", text.trim_end_matches('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_paths::{ConfigFile, MapEnv};
    use rb_platform::{
        CommandOutput, CommandRunner, MemorySecretStore, PlatformError, Secret, SecretStore,
    };
    #[cfg(feature = "live")]
    use serde_json::json;
    #[cfg(feature = "live")]
    use wiremock::matchers::path;
    #[cfg(feature = "live")]
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct NoRunner;

    impl CommandRunner for NoRunner {
        fn run(
            &self,
            program: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> Result<CommandOutput, PlatformError> {
            Err(PlatformError::Spawn {
                program: program.into(),
                reason: "not found".into(),
            })
        }
    }

    fn setup_services(
        store: Arc<MemorySecretStore>,
        dir: &std::path::Path,
    ) -> Arc<setup::Services> {
        Arc::new(setup::Services {
            runner: Arc::new(NoRunner),
            store,
            roots: setup::Roots {
                git_configs: Vec::new(),
                scan_root: dir.join("none"),
            },
            api_urls: Default::default(),
        })
    }

    fn services(
        file: &std::path::Path,
        store: Arc<MemorySecretStore>,
        dir: &std::path::Path,
    ) -> Arc<Services> {
        let file = file.to_path_buf();
        Arc::new(Services::new(setup_services(store, dir), move || {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let loaded =
                crate::config::LoadedConfig::from_texts(&[(file.clone(), text)], &MapEnv::new(""))
                    .map_err(|e| e.to_string())?;
            let paths = PathsReport {
                paths: rb_paths::ResolvedPaths::resolve(&MapEnv::new("/home/u")).unwrap(),
                config_files: vec![ConfigFile {
                    path: file.clone(),
                    origin: ConfigOrigin::User,
                    exists: file.exists(),
                }],
                write_target: file.clone(),
            };
            Ok(Loaded {
                snapshot: snapshot(&paths, &loaded.config),
                config: loaded.config,
            })
        }))
    }

    #[cfg(feature = "live")]
    async fn stub(server: &MockServer, status: u16) {
        Mock::given(path("/user"))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("x-oauth-scopes", "repo")
                    .set_body_json(json!({"login": "octo"})),
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(server)
            .await;
    }

    #[cfg(feature = "live")]
    fn spec(server: &MockServer) -> SourceSpec {
        SourceSpec {
            name: "ghe".into(),
            kind: ForgeKind::GitHub,
            host: "ghe.test".into(),
            api_url: Some(server.uri()),
            auth: AuthKind::Token,
            scope_user: false,
            owners: Vec::new(),
        }
    }

    #[cfg(feature = "live")]
    #[tokio::test]
    async fn adding_with_a_token_tests_it_keeps_it_in_the_keyring_and_writes_no_secret() {
        let server = MockServer::start().await;
        stub(&server, 200).await;
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("config.toml");
        std::fs::write(&file, "# mine\n[ui]\njax = false\n").unwrap();
        let store = Arc::new(MemorySecretStore::new());
        let s = services(&file, store.clone(), t.path());
        let out = run_effect(
            &s,
            Effect::Save {
                target: file.clone(),
                change: Change::Add {
                    spec: spec(&server),
                    token: Some(Secret::new("ghp_topsecret")),
                },
            },
        )
        .await;
        let Input::Saved(Ok(saved)) = out else {
            panic!("{out:?}");
        };
        assert_eq!(saved.message, "Added ghe.");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("# mine\n") && text.contains("name = \"ghe\""));
        assert!(!text.contains("ghp_topsecret"));
        assert_eq!(
            store.get("ghe.test").unwrap().unwrap().expose(),
            "ghp_topsecret"
        );
        let Input::Loaded(Ok(snap)) = run_effect(&s, Effect::Load).await else {
            panic!()
        };
        assert_eq!(snap.rows.len(), 1);
        assert!(snap.editable && snap.origin.is_none());
    }

    #[cfg(feature = "live")]
    #[tokio::test]
    async fn a_rejected_token_writes_nothing_and_says_what_to_do() {
        let server = MockServer::start().await;
        stub(&server, 401).await;
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("config.toml");
        let store = Arc::new(MemorySecretStore::new());
        let s = services(&file, store.clone(), t.path());
        let out = run_effect(
            &s,
            Effect::Save {
                target: file.clone(),
                change: Change::Add {
                    spec: spec(&server),
                    token: Some(Secret::new("nope")),
                },
            },
        )
        .await;
        let Input::Saved(Err(why)) = out else {
            panic!("{out:?}");
        };
        assert!(why.contains("rejected"), "{why}");
        assert!(!file.exists());
        assert!(store.get("ghe.test").unwrap().is_none());
    }

    #[tokio::test]
    async fn toggling_editing_and_removing_round_trip_through_the_file() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("config.toml");
        std::fs::write(
            &file,
            "# keep\n[[source]]\nname = \"a\"   # first\nkind = \"github\"\nhost = \"github.com\"\nauth = \"token\"\n",
        )
        .unwrap();
        let store = Arc::new(MemorySecretStore::new());
        store.set("github.com", &Secret::new("t")).unwrap();
        let s = services(&file, store.clone(), t.path());
        let save = |change| Effect::Save {
            target: file.clone(),
            change,
        };
        let out = run_effect(
            &s,
            save(Change::Toggle {
                name: "a".into(),
                enabled: false,
            }),
        )
        .await;
        assert!(matches!(out, Input::Saved(Ok(ref m)) if m.message.contains("is off")));
        let Input::Loaded(Ok(snap)) = run_effect(&s, Effect::Load).await else {
            panic!()
        };
        assert!(!snap.rows[0].enabled);

        let spec = snap.rows[0].spec.clone();
        let renamed = SourceSpec {
            name: "b".into(),
            ..spec.clone()
        };
        run_effect(
            &s,
            save(Change::Edit {
                before: spec,
                after: renamed.clone(),
                token: None,
            }),
        )
        .await;
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("# keep") && text.contains("# first"),
            "{text}"
        );
        assert!(text.contains("name = \"b\""));

        let out = run_effect(
            &s,
            save(Change::Remove {
                spec: renamed.clone(),
                forget_token: false,
            }),
        )
        .await;
        assert!(
            matches!(out, Input::Saved(Ok(ref m)) if m.message == "Removed b. Its token is still in your keyring.")
        );
        assert!(store.get("github.com").unwrap().is_some());
        assert!(!std::fs::read_to_string(&file)
            .unwrap()
            .contains("[[source]]"));
    }

    #[tokio::test]
    async fn removing_can_forget_the_keyring_token_too() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("config.toml");
        std::fs::write(
            &file,
            "[[source]]\nname = \"a\"\nkind = \"github\"\nhost = \"github.com\"\nauth = \"token\"\n",
        )
        .unwrap();
        let store = Arc::new(MemorySecretStore::new());
        store.set("github.com", &Secret::new("t")).unwrap();
        let s = services(&file, store.clone(), t.path());
        let spec = SourceSpec {
            name: "a".into(),
            kind: ForgeKind::GitHub,
            host: "github.com".into(),
            api_url: None,
            auth: AuthKind::Token,
            scope_user: false,
            owners: Vec::new(),
        };
        let out = run_effect(
            &s,
            Effect::Save {
                target: file.clone(),
                change: Change::Remove {
                    spec,
                    forget_token: true,
                },
            },
        )
        .await;
        assert!(
            matches!(out, Input::Saved(Ok(ref m)) if m.message == "Removed a and its token from the keyring.")
        );
        assert!(store.get("github.com").unwrap().is_none());
    }

    #[tokio::test]
    async fn a_missing_source_is_a_calm_error() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("config.toml");
        std::fs::write(&file, "# nothing\n").unwrap();
        let s = services(&file, Arc::new(MemorySecretStore::new()), t.path());
        let out = run_effect(
            &s,
            Effect::Save {
                target: file.clone(),
                change: Change::Toggle {
                    name: "gone".into(),
                    enabled: false,
                },
            },
        )
        .await;
        let Input::Saved(Err(why)) = out else {
            panic!()
        };
        assert!(
            why.contains("gone") && why.ends_with("Nothing was changed."),
            "{why}"
        );
    }

    #[cfg(feature = "live")]
    #[tokio::test]
    async fn testing_a_token_reports_the_user_and_scopes_or_a_next_step() {
        let server = MockServer::start().await;
        stub(&server, 200).await;
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("config.toml");
        std::fs::write(
            &file,
            format!(
                "[[source]]\nname = \"ghe\"\nkind = \"github\"\nhost = \"ghe.test\"\napi_url = \"{}\"\nauth = \"token\"\nenabled = false\n",
                server.uri()
            ),
        )
        .unwrap();
        let store = Arc::new(MemorySecretStore::new());
        let s = services(&file, store.clone(), t.path());
        let out = run_effect(&s, Effect::Test { name: "ghe".into() }).await;
        let Input::Tested {
            result: Err(why), ..
        } = out
        else {
            panic!("no token yet");
        };
        assert!(
            why.contains("ghe.test") && why.contains("To fix it"),
            "{why}"
        );

        store.set("ghe.test", &Secret::new("ghp_x")).unwrap();
        let out = run_effect(&s, Effect::Test { name: "ghe".into() }).await;
        let Input::Tested {
            result: Ok(info), ..
        } = out
        else {
            panic!("{out:?}");
        };
        assert_eq!(info.user, "octo");
        assert_eq!(info.scopes, ["repo"]);
        assert!(info.note.unwrap().contains("read:org"));
    }

    #[test]
    fn sources_from_another_layer_are_read_only_with_their_origin() {
        let t = tempfile::tempdir().unwrap();
        let user = t.path().join("user.toml");
        let dropin = t.path().join("config.d/10-work.toml");
        std::fs::create_dir_all(dropin.parent().unwrap()).unwrap();
        std::fs::write(&user, "[ui]\njax = false\n").unwrap();
        let body = "[[source]]\nname = \"w\"\nkind = \"github\"\nhost = \"github.com\"\n";
        std::fs::write(&dropin, body).unwrap();
        let file = |path: &std::path::Path, origin| ConfigFile {
            path: path.to_path_buf(),
            origin,
            exists: true,
        };
        let paths = PathsReport {
            paths: rb_paths::ResolvedPaths::resolve(&MapEnv::new("/home/u")).unwrap(),
            config_files: vec![
                file(&user, ConfigOrigin::User),
                file(&dropin, ConfigOrigin::DropIn),
            ],
            write_target: user.clone(),
        };
        let config: Config = toml::from_str(body).unwrap();
        let snap = snapshot(&paths, &config);
        assert!(!snap.editable);
        assert_eq!(snap.origin.unwrap().label, "from config.d/10-work.toml");

        let system = t.path().join("etc/config.toml");
        std::fs::create_dir_all(system.parent().unwrap()).unwrap();
        std::fs::write(&system, body).unwrap();
        let paths = PathsReport {
            config_files: vec![
                file(&system, ConfigOrigin::SystemDir),
                file(&user, ConfigOrigin::User),
            ],
            ..paths
        };
        let snap = snapshot(&paths, &config);
        assert_eq!(
            snap.origin.unwrap().label,
            format!("from {}", system.display())
        );

        std::fs::write(&user, body).unwrap();
        let snap = snapshot(&paths, &config);
        assert!(
            snap.editable && snap.origin.is_none(),
            "the write target wins when it defines them"
        );

        let empty: Config = Config::default();
        std::fs::write(&system, "").unwrap();
        std::fs::write(&user, "").unwrap();
        let snap = snapshot(&paths, &empty);
        assert!(snap.editable && snap.rows.is_empty());
    }

    #[test]
    fn an_unavailable_session_answers_every_effect_calmly() {
        assert!(matches!(unavailable(Effect::Load), Input::Loaded(Err(_))));
        assert!(matches!(
            unavailable(Effect::Test { name: "x".into() }),
            Input::Tested { result: Err(_), .. }
        ));
    }
}
