//! Loading, validating and editing `config.toml`. See `docs/configuration.md`.

mod edit;
mod error;
mod schema;
mod sources;

use std::path::{Path, PathBuf};

use rb_paths::{ConfigFile, ConfigLayers, Env};

pub use edit::{write_atomic, ConfigEditor};
pub use error::{ConfigError, Location};
pub use schema::*;
pub use sources::{hide_pattern_problem, pattern_matches, source_from_config};

/// The merged configuration plus the files it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedConfig {
    pub config: Config,
    /// Files that existed and were merged, in merge order.
    pub loaded: Vec<PathBuf>,
}

impl LoadedConfig {
    /// Discovers nothing itself: reads the layers `rb_paths` resolved and
    /// applies environment overrides.
    pub fn load(layers: &ConfigLayers, env: &dyn Env) -> Result<Self, ConfigError> {
        let mut inputs = Vec::new();
        for ConfigFile { path, exists, .. } in &layers.files {
            if !exists {
                continue;
            }
            match std::fs::read_to_string(path) {
                Ok(text) => inputs.push((path.clone(), text)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(ConfigError::Read {
                        path: path.clone(),
                        source,
                    })
                }
            }
        }
        Self::from_texts(&inputs, env)
    }

    /// Merges already-read files, least important first.
    pub fn from_texts(inputs: &[(PathBuf, String)], env: &dyn Env) -> Result<Self, ConfigError> {
        let mut merged = toml::Table::new();
        for (path, text) in inputs {
            let table = validate_file(path, text)?;
            merge_tables(&mut merged, table);
        }
        let mut config: Config = merged.try_into().map_err(|e: toml::de::Error| {
            ConfigError::invalid(Path::new("merged config"), "", None, e.message())
        })?;
        apply_env(&mut config, env);
        Ok(Self {
            config,
            loaded: inputs.iter().map(|(p, _)| p.clone()).collect(),
        })
    }
}

/// Parses one file and returns its table, or the first problem with its line.
fn validate_file(path: &Path, text: &str) -> Result<toml::Table, ConfigError> {
    let table: toml::Table = toml::from_str(text)
        .map_err(|e| ConfigError::invalid(path, text, e.span(), e.message()))?;
    let config: Config = toml::from_str(text)
        .map_err(|e| ConfigError::invalid(path, text, e.span(), e.message()))?;
    sources::validate(path, text, &config.sources)?;
    Ok(table)
}

/// Tables merge key by key; everything else, arrays included, is replaced.
fn merge_tables(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge_tables(b, o),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

fn apply_env(config: &mut Config, env: &dyn Env) {
    if let Some(theme) = env.var("REVIEW_BUDDY_THEME").filter(|v| !v.is_empty()) {
        config.ui.theme = theme;
    }
    if let Some(position) = env
        .var("REVIEW_BUDDY_DETAIL_POSITION")
        .and_then(|v| parse_position(&v))
    {
        config.ui.detail_position = position;
    }
    if let Some(v) = env.var("REVIEW_BUDDY_REDUCED_MOTION") {
        if matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes") {
            config.ui.reduced_motion = true;
        }
    }
}

/// Reads `REVIEW_BUDDY_DETAIL_POSITION`; an unknown value is ignored, like the other env settings.
pub fn parse_position(value: &str) -> Option<DetailPosition> {
    [
        DetailPosition::Auto,
        DetailPosition::Right,
        DetailPosition::Left,
        DetailPosition::Top,
        DetailPosition::Bottom,
    ]
    .into_iter()
    .find(|p| p.as_str() == value.trim().to_ascii_lowercase())
}

impl Config {
    /// Enabled sources in config order, as domain sources.
    pub fn sources(&self) -> Vec<rb_core::Source> {
        self.sources
            .iter()
            .filter(|s| s.enabled)
            .map(source_from_config)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_paths::{MapEnv, ResolvedPaths};
    use std::time::Duration;

    fn env() -> MapEnv {
        MapEnv::new("/home/u")
    }

    fn load(files: &[(&str, &str)]) -> Result<LoadedConfig, ConfigError> {
        let inputs: Vec<_> = files
            .iter()
            .map(|(p, t)| (PathBuf::from(p), t.to_string()))
            .collect();
        LoadedConfig::from_texts(&inputs, &env())
    }

    #[test]
    fn no_files_gives_defaults() {
        let l = load(&[]).unwrap();
        assert_eq!(l.config, Config::default());
        assert!(l.loaded.is_empty());
    }

    #[test]
    fn tables_merge_key_by_key() {
        let l = load(&[
            ("/etc/a.toml", "[ui]\ntheme = \"sys\"\nmouse = false\n"),
            ("/home/c.toml", "[ui]\ntheme = \"mine\"\n"),
        ])
        .unwrap();
        assert_eq!(l.config.ui.theme, "mine");
        assert!(!l.config.ui.mouse);
        assert!(l.config.ui.jax);
    }

    #[test]
    fn background_settings_parse_with_per_theme_overrides() {
        let l = load(&[(
            "/c.toml",
            "[ui]\nbackground = \"yes\"\n\n[ui.theme_background]\ndusk = \"no\"\nafterglow-light = \"theme\"\n",
        )])
        .unwrap();
        assert_eq!(l.config.ui.background, Background::Yes);
        assert_eq!(l.config.ui.theme_background["dusk"], Background::No);
        assert_eq!(l.config.ui.theme_background.len(), 2);
        assert_eq!(Config::default().ui.background, Background::Theme);
    }

    #[test]
    fn a_bad_background_names_the_file_and_line() {
        let e = load(&[("/c.toml", "[ui]\nbackground = \"always\"\n")]).unwrap_err();
        assert!(e.to_string().contains("/c.toml:2"), "{e}");
        let e = load(&[("/c.toml", "[ui.theme_background]\ndusk = \"maybe\"\n")]).unwrap_err();
        assert!(e.to_string().contains("/c.toml:2"), "{e}");
    }

    #[test]
    fn arrays_and_sources_replace() {
        let src = |n: &str| {
            format!("[[source]]\nname = \"{n}\"\nkind = \"github\"\nhost = \"github.com\"\n")
        };
        let l = load(&[
            (
                "/a.toml",
                &format!(
                    "{}{}\n[triage]\nnoise_authors = [\"a\", \"b\"]\n",
                    src("one"),
                    src("two")
                ),
            ),
            (
                "/b.toml",
                &format!("{}\n[triage]\nnoise_authors = [\"c\"]\n", src("three")),
            ),
        ])
        .unwrap();
        let names: Vec<_> = l.config.sources.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["three"]);
        assert_eq!(l.config.triage.noise_authors, ["c"]);
    }

    #[test]
    fn later_layers_win_in_given_order() {
        let l = load(&[
            ("/10-work.toml", "[ui]\nlayout = \"split\"\n"),
            ("/20-personal.toml", "[ui]\nlayout = \"queue\"\n"),
        ])
        .unwrap();
        assert_eq!(l.config.ui.layout, Layout::Queue);
        assert_eq!(l.loaded.len(), 2);
    }

    #[test]
    fn env_overrides_win_over_files() {
        let env = env()
            .with_var("REVIEW_BUDDY_THEME", "afterglow-dark")
            .with_var("REVIEW_BUDDY_REDUCED_MOTION", "1");
        let inputs = vec![(PathBuf::from("/a"), "[ui]\ntheme = \"x\"\n".to_string())];
        let l = LoadedConfig::from_texts(&inputs, &env).unwrap();
        assert_eq!(l.config.ui.theme, "afterglow-dark");
        assert!(l.config.ui.reduced_motion);
    }

    #[test]
    fn empty_or_falsy_env_is_ignored() {
        let env = env()
            .with_var("REVIEW_BUDDY_THEME", "")
            .with_var("REVIEW_BUDDY_REDUCED_MOTION", "0");
        let l = LoadedConfig::from_texts(&[], &env).unwrap();
        assert_eq!(l.config.ui.theme, "liminal-hq");
        assert!(!l.config.ui.reduced_motion);
    }

    #[test]
    fn durations_and_off() {
        let l = load(&[(
            "/a",
            "[refresh]\ninterval = \"off\"\n[triage]\nstale_after = \"7d\"\n",
        )])
        .unwrap();
        assert_eq!(l.config.refresh.interval, None);
        assert_eq!(l.config.triage.stale_after, Duration::from_secs(7 * 86_400));
    }

    #[test]
    fn syntax_errors_name_file_and_line() {
        let e = load(&[("/c/config.toml", "[ui]\ntheme = \"ok\"\nmouse = \n")]).unwrap_err();
        assert_eq!(e.path(), Path::new("/c/config.toml"));
        assert_eq!(e.line(), Some(3));
        assert!(e.to_string().starts_with("/c/config.toml:3:"));
    }

    #[test]
    fn bad_values_list_the_choices() {
        let e = load(&[("/c.toml", "[ui]\njax = true\nlayout = \"wide\"\n")]).unwrap_err();
        assert_eq!(e.line(), Some(3));
        let msg = e.to_string();
        assert!(msg.contains("wide") && msg.contains("panes"), "{msg}");
    }

    #[test]
    fn layout_options_parse_and_default_to_auto() {
        let l = load(&[("/c.toml", "[ui]\nsources = \"top\"\ndetail = \"closed\"\n")]).unwrap();
        assert_eq!(l.config.ui.sources, SourcesLayout::Top);
        assert_eq!(l.config.ui.detail, DetailMode::Closed);
        let d = load(&[("/c.toml", "")]).unwrap();
        assert_eq!(d.config.ui.sources, SourcesLayout::Auto);
        assert_eq!(d.config.ui.detail, DetailMode::Auto);
    }

    #[test]
    fn detail_position_parses_defaults_and_reads_the_env() {
        let l = load(&[("/c.toml", "[ui]\ndetail_position = \"left\"\n")]).unwrap();
        assert_eq!(l.config.ui.detail_position, DetailPosition::Left);
        let d = load(&[("/c.toml", "")]).unwrap();
        assert_eq!(d.config.ui.detail_position, DetailPosition::Auto);
        let inputs = vec![(PathBuf::from("/c.toml"), String::new())];
        let e = env().with_var("REVIEW_BUDDY_DETAIL_POSITION", "Bottom");
        let l = LoadedConfig::from_texts(&inputs, &e).unwrap();
        assert_eq!(l.config.ui.detail_position, DetailPosition::Bottom);
        let e = env().with_var("REVIEW_BUDDY_DETAIL_POSITION", "sideways");
        let l = LoadedConfig::from_texts(&inputs, &e).unwrap();
        assert_eq!(l.config.ui.detail_position, DetailPosition::Auto);
    }

    #[test]
    fn a_bad_detail_position_names_the_file_line_and_choices() {
        let e = load(&[(
            "/c/config.toml",
            "[ui]\njax = true\ndetail_position = \"middle\"\n",
        )])
        .unwrap_err();
        assert_eq!(e.line(), Some(3));
        let msg = e.to_string();
        assert!(msg.starts_with("/c/config.toml:3:"), "{msg}");
        assert!(msg.contains("bottom") && msg.contains("auto"), "{msg}");
    }

    #[test]
    fn bad_layout_values_name_the_file_line_and_choices() {
        let e = load(&[(
            "/c/config.toml",
            "[ui]\nmouse = true\nsources = \"bottom\"\n",
        )])
        .unwrap_err();
        assert_eq!(e.path(), Path::new("/c/config.toml"));
        assert_eq!(e.line(), Some(3));
        let msg = e.to_string();
        assert!(
            msg.contains("bottom") && msg.contains("left") && msg.contains("top"),
            "{msg}"
        );
        let e = load(&[("/c.toml", "[ui]\ndetail = \"hidden\"\n")]).unwrap_err();
        assert_eq!(e.line(), Some(2));
        assert!(e.to_string().contains("closed"), "{e}");
    }

    #[test]
    fn typos_in_known_tables_are_errors() {
        let e = load(&[("/c.toml", "[review]\nconfim_merge = false\n")]).unwrap_err();
        assert_eq!(e.line(), Some(2));
        assert!(e.to_string().contains("confim_merge"));
    }

    #[test]
    fn the_second_file_is_blamed_when_it_is_the_bad_one() {
        let e = load(&[
            ("/a.toml", "[ui]\njax = false\n"),
            ("/b.toml", "[ui]\njax = 3\n"),
        ])
        .unwrap_err();
        assert_eq!(e.path(), Path::new("/b.toml"));
        assert_eq!(e.line(), Some(2));
    }

    #[test]
    fn bad_duration_is_reported() {
        let e = load(&[("/c.toml", "[triage]\nstale_after = \"soon\"\n")]).unwrap_err();
        assert_eq!(e.line(), Some(2));
        assert!(e.to_string().contains("isn't a duration"));
    }

    #[test]
    fn load_reads_discovered_layers_in_order() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("cfg");
        let sys = t.path().join("sys");
        for d in [home.join("review-buddy/config.d"), sys.join("review-buddy")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let w = |p: PathBuf, s: &str| std::fs::write(p, s).unwrap();
        w(
            sys.join("review-buddy/config.toml"),
            "[ui]\ntheme = \"sys\"\njax = false\n",
        );
        w(
            home.join("review-buddy/config.toml"),
            "[ui]\ntheme = \"user\"\n",
        );
        w(
            home.join("review-buddy/config.d/20-b.toml"),
            "[ui]\nlayout = \"queue\"\n",
        );
        w(
            home.join("review-buddy/config.d/10-a.toml"),
            "[ui]\nlayout = \"split\"\ntheme = \"drop\"\n",
        );
        let env = MapEnv::new(t.path())
            .with_var("XDG_CONFIG_HOME", home.to_str().unwrap())
            .with_var("XDG_CONFIG_DIRS", sys.to_str().unwrap());
        let paths = ResolvedPaths::resolve(&env).unwrap();
        let layers = ConfigLayers::discover(&paths, &env, None);
        let l = LoadedConfig::load(&layers, &env).unwrap();
        assert_eq!(l.config.ui.theme, "drop");
        assert_eq!(l.config.ui.layout, Layout::Queue);
        assert!(!l.config.ui.jax);
        assert_eq!(l.loaded.len(), 4);
    }

    #[test]
    fn explicit_config_replaces_user_layers() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("cfg");
        std::fs::create_dir_all(home.join("review-buddy")).unwrap();
        std::fs::write(
            home.join("review-buddy/config.toml"),
            "[ui]\ntheme = \"user\"\n",
        )
        .unwrap();
        let alt = t.path().join("alt.toml");
        std::fs::write(&alt, "[ui]\nmouse = false\n").unwrap();
        let env = MapEnv::new(t.path()).with_var("XDG_CONFIG_HOME", home.to_str().unwrap());
        let paths = ResolvedPaths::resolve(&env).unwrap();
        let layers = ConfigLayers::discover(&paths, &env, Some(alt));
        let l = LoadedConfig::load(&layers, &env).unwrap();
        assert_eq!(l.config.ui.theme, "liminal-hq");
        assert!(!l.config.ui.mouse);
    }

    #[test]
    fn the_example_config_loads() {
        let text = include_str!("../../../../config.example.toml");
        let l = load(&[("config.example.toml", text)]).unwrap();
        assert_eq!(l.config.sources.len(), 4);
        assert_eq!(l.config.sources()[2].host, "gitlab.work.ca");
    }
}
