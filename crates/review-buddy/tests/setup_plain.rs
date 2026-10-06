//! Scripts the plain prompt flow end to end with a fake `gh`, an in-memory keyring and a stub
//! GitHub server.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rb_platform::{CommandOutput, CommandRunner, MemorySecretStore, PlatformError, SecretStore};
use review_buddy::config::LoadedConfig;
use review_buddy::setup::{self, plain, Flow, Outcome, Roots, Services};
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

struct FakeGh;

impl CommandRunner for FakeGh {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        _: Option<&[u8]>,
    ) -> Result<CommandOutput, PlatformError> {
        let ok = |stdout: &str| {
            Ok(CommandOutput {
                success: true,
                stdout: stdout.into(),
                stderr: String::new(),
            })
        };
        match (program, args.first().copied(), args.get(1).copied()) {
            ("gh", Some("auth"), Some("status")) => {
                ok("ghe.test\n  ✓ Logged in to ghe.test account octo (keyring)\n")
            }
            ("gh", Some("auth"), Some("token")) => ok("gho_fake\n"),
            _ => Err(PlatformError::Spawn {
                program: program.into(),
                reason: "not found".into(),
            }),
        }
    }
}

async fn github() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/user"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-oauth-scopes", "repo, read:org")
                .set_body_json(json!({"login": "octo"})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/rate_limit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resources": {"core": {"limit": 5000, "remaining": 1, "reset": 1}}
        })))
        .mount(&server)
        .await;
    Mock::given(path("/user/orgs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"login": "liminal-hq"}])))
        .mount(&server)
        .await;
    server
}

fn services(server: &MockServer, dir: &Path, store: Arc<MemorySecretStore>) -> Arc<Services> {
    Arc::new(Services {
        runner: Arc::new(FakeGh),
        store,
        roots: Roots {
            git_configs: vec![dir.join(".gitconfig")],
            scan_root: dir.join("src"),
        },
        api_urls: HashMap::from([("ghe.test".to_string(), server.uri())]),
    })
}

fn run(
    server: &MockServer,
    dir: &Path,
    existing: bool,
    answers: &str,
) -> (plain::Run, String, Arc<MemorySecretStore>) {
    let store = Arc::new(MemorySecretStore::new());
    let services = services(server, dir, store.clone());
    let flow = Flow::new(dir.join("config.toml"), existing, "liminal-hq", true);
    let mut out = Vec::new();
    let run = plain::run(flow, &services, &mut answers.as_bytes(), &mut out, false).unwrap();
    (run, String::from_utf8(out).unwrap(), store)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_whole_run_writes_a_commented_config() {
    let server = github().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/app/.git")).unwrap();
    std::fs::write(
        dir.path().join("src/app/.git/config"),
        "[remote \"origin\"]\n\turl = git@gitlab.work.ca:p/app.git\n",
    )
    .unwrap();
    let dir_path = dir.path().to_path_buf();
    let server = Arc::new(server);
    let s = Arc::clone(&server);
    let (run, out, store) = tokio::task::spawn_blocking(move || {
        // connect both hosts (the second needs a token), include the org, Dusk, no Jax, save
        run(&s, &dir_path, false, "1,2\nglpat-1\n2\n2\nn\ny\n")
    })
    .await
    .unwrap();
    assert!(matches!(run.outcome, Outcome::Written { .. }), "{out}");
    assert!(out.contains("signed in as octo via gh · 1 org"), "{out}");
    assert!(!out.contains("glpat-1\n"), "the token isn't echoed back");
    let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(!text.contains("glpat"), "tokens never reach the file");
    let loaded = LoadedConfig::from_texts(
        &[(dir.path().join("config.toml"), text.clone())],
        &rb_paths::MapEnv::new(""),
    )
    .unwrap();
    assert_eq!(loaded.config.ui.theme, "dusk");
    assert!(!loaded.config.ui.jax);
    let names: Vec<_> = loaded
        .config
        .sources
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["ghe.test", "gitlab.work.ca"]);
    assert_eq!(loaded.config.sources[0].scope.orgs, ["liminal-hq"]);
    assert!(text.contains("# Tokens are never stored here"));
    assert_eq!(
        store.get("gitlab.work.ca").unwrap().unwrap().expose(),
        "glpat-1"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn declining_to_replace_an_existing_file_changes_nothing() {
    let server = Arc::new(github().await);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), "# mine\n").unwrap();
    let p = dir.path().to_path_buf();
    let s = Arc::clone(&server);
    let (run, out, _) = tokio::task::spawn_blocking(move || run(&s, &p, true, "\n\n\n\n\n\n"))
        .await
        .unwrap();
    assert_eq!(run.outcome, Outcome::Skipped, "{out}");
    assert!(out.contains("Replace it? [y/N]"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        "# mine\n"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn running_out_of_input_skips_without_writing() {
    let server = Arc::new(github().await);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().to_path_buf();
    let s = Arc::clone(&server);
    let (run, _, _) = tokio::task::spawn_blocking(move || run(&s, &p, false, ""))
        .await
        .unwrap();
    assert_eq!(run.outcome, Outcome::Skipped);
    assert!(!dir.path().join("config.toml").exists());
    let _ = setup::needs_first_run;
}
