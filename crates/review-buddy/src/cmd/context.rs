//! What every command shares: sources, providers, cache, theme and terminal state.
//!
//! Built the way the interface builds them: the same `rb-paths` resolution, config layering
//! and `DemoProvider` swap. Under `--demo` nothing real is read, and nothing is written outside
//! a throwaway directory.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use rb_core::{Provider, Source};
use rb_paths::{ConfigLayers, Env, PathsReport, SystemEnv};
use rb_store::Store;
use rb_theme::{ColourDepth, Palette, Theme, DEFAULT_THEME_ID};

use super::error::CmdError;
use super::git;
use super::output::{colour_enabled, pager, OutputMode, Painter};
use super::prompt::Interaction;
use super::selector::{self, Inference, RepoRef, Selector, Target};
use crate::cli::GlobalArgs;
use crate::config::{ColourDepth as DepthSetting, Config, LoadedConfig};

/// Terminal facts, injected so tests can pretend to be either kind of terminal.
#[derive(Debug, Clone, Copy)]
pub struct Terminal {
    pub stdout_tty: bool,
    pub stdin_tty: bool,
    pub width: Option<usize>,
}

impl Terminal {
    pub fn detect() -> Self {
        let stdout_tty = std::io::stdout().is_terminal();
        Self {
            stdout_tty,
            stdin_tty: std::io::stdin().is_terminal(),
            width: stdout_tty
                .then(|| crossterm::terminal::size().ok())
                .flatten()
                .map(|(cols, _)| usize::from(cols)),
        }
    }
}

/// Demo mode points every XDG location at its throwaway directory and ignores
/// `$REVIEW_BUDDY_CONFIG`; terminal and colour variables still come from the real environment.
#[cfg(feature = "demo")]
struct DemoProcessEnv(rb_paths::MapEnv);

#[cfg(feature = "demo")]
impl Env for DemoProcessEnv {
    fn var(&self, key: &str) -> Option<String> {
        if key.starts_with("XDG_") || key == "REVIEW_BUDDY_CONFIG" {
            self.0.var(key)
        } else {
            SystemEnv.var(key)
        }
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.0.home_dir()
    }

    fn os(&self) -> rb_paths::Os {
        self.0.os()
    }

    fn runtime_dir_is_private(&self, path: &std::path::Path) -> bool {
        self.0.runtime_dir_is_private(path)
    }
}

const DEFAULT_WIDTH: usize = 80;

pub struct Context {
    pub args: GlobalArgs,
    pub env: Box<dyn Env>,
    pub out: OutputMode,
    pub interaction: Interaction,
    pub paths: PathsReport,
    pub config: Config,
    config_error: Option<String>,
    #[cfg(feature = "live")]
    factory: Arc<crate::providers::Factory>,
    #[cfg(feature = "demo")]
    demo: Option<crate::demo::Demo>,
}

impl Context {
    pub fn build(args: GlobalArgs, terminal: Terminal) -> Result<Self, CmdError> {
        if args.demo {
            return Self::build_demo(args, terminal);
        }
        Self::from_env(args, terminal, Box::new(SystemEnv))
    }

    #[cfg(feature = "demo")]
    fn build_demo(args: GlobalArgs, terminal: Terminal) -> Result<Self, CmdError> {
        let demo = crate::demo::Demo::start(args.frozen_time.as_deref())
            .map_err(|e| CmdError::usage(format!("{e:#}")))?;
        let root = demo.env.root();
        let sub = |name: &str| root.join(name).display().to_string();
        let env = DemoProcessEnv(
            rb_paths::MapEnv::new(root)
                .with_var("XDG_CONFIG_HOME", &sub("config"))
                .with_var("XDG_DATA_HOME", &sub("data"))
                .with_var("XDG_CACHE_HOME", &sub("cache"))
                .with_var("XDG_STATE_HOME", &sub("state"))
                .with_var("XDG_CONFIG_DIRS", &sub("system-config"))
                .with_var("XDG_DATA_DIRS", &sub("system-data")),
        );
        let paths =
            PathsReport::build(&env, None).map_err(|e| CmdError::failed(format!("{e}.")))?;
        let mut ctx = Self::assemble(
            args,
            terminal,
            Box::new(env),
            paths,
            Config::default(),
            None,
        );
        ctx.demo = Some(demo);
        Ok(ctx)
    }

    #[cfg(not(feature = "demo"))]
    fn build_demo(_args: GlobalArgs, _terminal: Terminal) -> Result<Self, CmdError> {
        Err(CmdError::usage(
            "This build doesn't include demo mode.\nInstall a release build, or rebuild with the `demo` feature.",
        ))
    }

    /// Builds a context from an explicit environment, reading the config files it names.
    pub fn from_env(
        args: GlobalArgs,
        terminal: Terminal,
        env: Box<dyn Env>,
    ) -> Result<Self, CmdError> {
        let paths = PathsReport::build(env.as_ref(), args.config.clone())
            .map_err(|e| CmdError::failed(format!("{e}.")))?;
        let layers = ConfigLayers {
            files: paths.config_files.clone(),
            write_target: paths.write_target.clone(),
        };
        let (config, config_error) = match LoadedConfig::load(&layers, env.as_ref()) {
            Ok(loaded) => (loaded.config, None),
            Err(e) => (Config::default(), Some(e.to_string())),
        };
        Ok(Self::assemble(
            args,
            terminal,
            env,
            paths,
            config,
            config_error,
        ))
    }

    fn assemble(
        args: GlobalArgs,
        terminal: Terminal,
        env: Box<dyn Env>,
        paths: PathsReport,
        config: Config,
        config_error: Option<String>,
    ) -> Self {
        let enabled = colour_enabled(args.color, args.no_color, env.as_ref(), terminal.stdout_tty);
        let theme = Theme::builtin(&config.ui.theme)
            .or_else(|_| Theme::builtin(DEFAULT_THEME_ID))
            .unwrap_or_default();
        let depth = match config.ui.colour_depth {
            DepthSetting::Auto => {
                ColourDepth::detect(env.var("COLORTERM").as_deref(), env.var("TERM").as_deref())
            }
            other => ColourDepth::from_str(other.as_str()).unwrap_or(ColourDepth::Ansi16),
        };
        let painter = Painter::new(Palette::new(theme, depth, false), enabled);
        let width = terminal
            .width
            .or_else(|| env.var("COLUMNS").and_then(|c| c.parse().ok()))
            .filter(|w| *w > 0)
            .unwrap_or(DEFAULT_WIDTH);
        let out = OutputMode {
            tty: terminal.stdout_tty,
            width,
            painter,
            pager: pager::resolve_pager(
                env.var("REVIEW_BUDDY_PAGER").as_deref(),
                env.var("PAGER").as_deref(),
            ),
        };
        let prompts_off = env
            .var("REVIEW_BUDDY_PROMPT_DISABLED")
            .is_some_and(|v| !v.is_empty() && v != "0");
        let interaction = Interaction {
            yes: args.yes,
            interactive: terminal.stdin_tty && !prompts_off,
        };
        Self {
            args,
            env,
            out,
            interaction,
            paths,
            #[cfg(feature = "live")]
            factory: Arc::new(crate::providers::Factory::from_config(
                &config,
                crate::providers::Deps::system(),
            )),
            config,
            config_error,
            #[cfg(feature = "demo")]
            demo: None,
        }
    }

    /// The clock ages are measured against: frozen in demo mode, the real time otherwise.
    pub fn now(&self) -> rb_core::Timestamp {
        #[cfg(feature = "demo")]
        if let Some(demo) = &self.demo {
            return demo.world.now();
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        rb_core::Timestamp(i64::try_from(secs).unwrap_or(i64::MAX))
    }

    /// The demo world, when running with `--demo`.
    #[cfg(feature = "demo")]
    pub fn demo_world(&self) -> Option<&crate::demo::DemoWorld> {
        self.demo.as_ref().map(|d| &d.world)
    }

    pub fn is_demo(&self) -> bool {
        self.args.demo
    }

    /// Every configured source, narrowed by `--source`. A broken config file is reported here
    /// rather than at start-up, so `config paths` still works to diagnose it.
    pub fn sources(&self) -> Result<Vec<Source>, CmdError> {
        if let Some(problem) = &self.config_error {
            return Err(CmdError::failed(format!(
                "{problem}\nFix the config file, or see review-buddy config paths."
            )));
        }
        let all = self.all_sources();
        let wanted = &self.args.sources;
        if wanted.is_empty() {
            return Ok(all.into_iter().filter(|s| s.in_all).collect());
        }
        wanted
            .iter()
            .map(|name| {
                all.iter()
                    .find(|s| s.id.as_str() == name || &s.label == name)
                    .cloned()
                    .ok_or_else(|| {
                        let known: Vec<_> = all.iter().map(|s| s.id.as_str()).collect();
                        CmdError::usage(format!(
                            "There's no source called {name}.\nConfigured sources: {}.",
                            if known.is_empty() {
                                "none".into()
                            } else {
                                known.join(", ")
                            }
                        ))
                    })
            })
            .collect()
    }

    fn all_sources(&self) -> Vec<Source> {
        #[cfg(feature = "demo")]
        if let Some(demo) = &self.demo {
            return demo.world.sources();
        }
        self.config.sources()
    }

    /// The provider that talks to `source`'s forge.
    pub fn provider_for(&self, source: &Source) -> Result<Arc<dyn Provider>, CmdError> {
        #[cfg(feature = "demo")]
        if let Some(demo) = &self.demo {
            return Ok(Arc::new(demo.world.provider(source.kind)));
        }
        #[cfg(feature = "live")]
        {
            use crate::providers::ProviderError;
            self.factory.provider(&source.id).map_err(|e| match e {
                ProviderError::Auth(_) => CmdError::AuthNeeded(format!(
                    "{}\n{}",
                    e.failure(&source.host).summary,
                    crate::app::failure::SIGN_IN_STEP
                )),
                ProviderError::GitlabLater => {
                    CmdError::Unsupported(format!("{e}\nGitHub sources work today."))
                }
                ProviderError::UnknownSource(_) => CmdError::usage(e.to_string()),
                ProviderError::Client(_) => CmdError::failed(e.to_string()),
            })
        }
        #[cfg(not(feature = "live"))]
        {
            let _ = source;
            Err(CmdError::usage(
                "This build has no network support.\nTry --demo.",
            ))
        }
    }

    #[cfg(feature = "live")]
    pub fn factory(&self) -> Arc<crate::providers::Factory> {
        Arc::clone(&self.factory)
    }

    /// The cache, opened on first use. Demo mode gets a throwaway in-memory one.
    pub fn cache(&self) -> Result<Store, CmdError> {
        let store = if self.is_demo() {
            Store::open_in_memory()
        } else {
            Store::open(&self.paths.paths.cache_dir.join("cache.sqlite"))
        };
        store.map_err(|e| {
            CmdError::failed(format!(
                "Couldn't open the cache: {e}.\nIt's safe to delete; Review Buddy will rebuild it."
            ))
        })
    }

    /// Matches what the user typed to a source, repository and change.
    pub fn resolve_selector(&self, input: Option<&str>) -> Result<Target, CmdError> {
        let selector = selector::parse(input)?;
        let sources = self.sources()?;
        let repo_flag = self.args.repo.as_deref().map(RepoRef::parse).transpose()?;
        let needs_git = repo_flag.is_none()
            && matches!(
                selector,
                Selector::Number(_) | Selector::Branch(_) | Selector::Current
            );
        let info = if needs_git && !self.is_demo() {
            git::inspect(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        } else {
            git::GitInfo::default()
        };
        let inference = Inference {
            sources: &sources,
            only_sources: &[],
            repo_flag,
            git_remote: info.remote,
            current_branch: info.branch,
        };
        Ok(selector::resolve(&selector, &inference)?)
    }
}

#[cfg(all(test, feature = "demo"))]
mod tests {
    use super::*;
    use rb_core::SourceId;

    fn tty() -> Terminal {
        Terminal {
            stdout_tty: true,
            stdin_tty: true,
            width: Some(100),
        }
    }

    fn pipe() -> Terminal {
        Terminal {
            stdout_tty: false,
            stdin_tty: false,
            width: None,
        }
    }

    fn demo_args() -> GlobalArgs {
        GlobalArgs {
            demo: true,
            frozen_time: Some("2026-10-05T10:00".into()),
            ..GlobalArgs::default()
        }
    }

    #[test]
    fn demo_sources_and_providers_come_from_the_demo_world() {
        let ctx = Context::build(demo_args(), pipe()).unwrap();
        let sources = ctx.sources().unwrap();
        assert_eq!(sources.len(), 4);
        for source in &sources {
            assert_eq!(ctx.provider_for(source).unwrap().kind(), source.kind);
        }
        assert!(ctx.cache().is_ok());
    }

    #[test]
    fn source_flag_narrows_and_unknown_names_are_usage_errors() {
        let first = Context::build(demo_args(), pipe())
            .unwrap()
            .sources()
            .unwrap()[0]
            .id
            .clone();
        let args = GlobalArgs {
            sources: vec![first.to_string()],
            ..demo_args()
        };
        let sources = Context::build(args, pipe()).unwrap().sources().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, first);

        let args = GlobalArgs {
            sources: vec!["nope".into()],
            ..demo_args()
        };
        let err = Context::build(args, pipe()).unwrap().sources().unwrap_err();
        assert_eq!(err.exit().code(), 2);
        assert!(err.to_string().contains("no source called nope"));
        let _ = SourceId::new("x");
    }

    #[test]
    fn demo_directories_are_throwaway() {
        let ctx = Context::build(demo_args(), pipe()).unwrap();
        assert!(ctx
            .paths
            .paths
            .config_dir
            .to_string_lossy()
            .contains("review-buddy-demo-"));
    }

    #[test]
    fn output_mode_follows_the_terminal() {
        let piped = Context::build(demo_args(), pipe()).unwrap();
        assert!(!piped.out.tty && !piped.out.colour());
        let ctx = Context::build(demo_args(), tty()).unwrap();
        assert!(ctx.out.tty);
        assert_eq!(ctx.out.width, 100);
        assert!(ctx.interaction.interactive && !ctx.interaction.yes);
    }

    #[test]
    fn yes_and_terminal_state_set_the_interaction() {
        let args = GlobalArgs {
            yes: true,
            ..demo_args()
        };
        let ctx = Context::build(args, pipe()).unwrap();
        assert!(ctx.interaction.yes && !ctx.interaction.interactive);
    }

    #[test]
    fn sources_the_config_doesnt_know_are_usage_errors() {
        let ctx = Context::build(demo_args(), pipe()).unwrap();
        let source = ctx.sources().unwrap().remove(0);
        let real = Context::from_env(
            GlobalArgs::default(),
            pipe(),
            Box::new(rb_paths::MapEnv::new("/nonexistent-home")),
        )
        .unwrap();
        let err = real.provider_for(&source).err().unwrap();
        assert_eq!(err.exit().code(), 2);
    }

    #[test]
    fn demo_selectors_resolve_without_touching_git() {
        let ctx = Context::build(demo_args(), pipe()).unwrap();
        let err = ctx.resolve_selector(Some("214")).unwrap_err();
        assert_eq!(err.exit().code(), 2);
        assert!(err.to_string().contains("--repo"));
        let url = ctx
            .resolve_selector(Some("https://nowhere.example/a/b/pull/1"))
            .unwrap_err();
        assert!(url.to_string().contains("No configured source"));
    }
}
