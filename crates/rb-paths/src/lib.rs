//! XDG path resolution, config layering and private file creation.
//!
//! Everything is resolved from an injected [`Env`], so the logic never reads
//! `std::env` directly. Use [`SystemEnv`] in the binary and [`MapEnv`] in tests.

mod config;
mod env;
mod fs;
mod resolve;

pub use config::{ConfigFile, ConfigLayers, ConfigOrigin};
pub use env::{Env, MapEnv, Os, SystemEnv};
pub use fs::{ensure_private_dir, write_private_file};
pub use resolve::{PathsError, ResolvedPaths, APP_DIR};

use std::fmt;
use std::path::PathBuf;

/// Everything `review-buddy config paths` prints: resolved directories and the
/// config files considered, in merge order (later overrides earlier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathsReport {
    pub paths: ResolvedPaths,
    pub config_files: Vec<ConfigFile>,
    /// The file the app writes settings changes to.
    pub write_target: PathBuf,
}

impl PathsReport {
    /// Resolves directories and discovers config files.
    ///
    /// `cli_config` is the `--config <path>` value, which beats `$REVIEW_BUDDY_CONFIG`.
    pub fn build(env: &dyn Env, cli_config: Option<PathBuf>) -> Result<Self, PathsError> {
        let paths = ResolvedPaths::resolve(env)?;
        let layers = ConfigLayers::discover(&paths, env, cli_config);
        Ok(Self {
            write_target: layers.write_target.clone(),
            config_files: layers.files,
            paths,
        })
    }

    /// Only the files that exist and will be merged, in order.
    pub fn loaded(&self) -> impl Iterator<Item = &ConfigFile> {
        self.config_files.iter().filter(|f| f.exists)
    }
}

impl fmt::Display for PathsReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = &self.paths;
        writeln!(f, "Directories")?;
        writeln!(f, "  config   {}", p.config_dir.display())?;
        writeln!(f, "  data     {}", p.data_dir.display())?;
        writeln!(f, "  cache    {}", p.cache_dir.display())?;
        writeln!(f, "  state    {}", p.state_dir.display())?;
        match &p.runtime_dir {
            Some(r) => writeln!(f, "  runtime  {}", r.display())?,
            None => writeln!(f, "  runtime  {} (state fallback)", p.lock_dir().display())?,
        }
        for d in &p.system_config_dirs {
            writeln!(f, "  system config  {}", d.display())?;
        }
        for d in &p.system_data_dirs {
            writeln!(f, "  system data    {}", d.display())?;
        }
        writeln!(f)?;
        writeln!(f, "Config files (later overrides earlier)")?;
        for file in &self.config_files {
            let status = if file.exists { "loaded" } else { "not found" };
            writeln!(f, "  [{status}] {} ({})", file.path.display(), file.origin)?;
        }
        writeln!(f)?;
        write!(f, "Settings are written to {}", self.write_target.display())
    }
}

// These tests use POSIX-style absolute paths in an injected environment, so they run on Unix hosts.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn env(home: &str) -> MapEnv {
        MapEnv::new(home)
    }

    #[test]
    fn unix_defaults() {
        let p = ResolvedPaths::resolve(&env("/home/a")).unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/home/a/.config/review-buddy"));
        assert_eq!(
            p.data_dir,
            PathBuf::from("/home/a/.local/share/review-buddy")
        );
        assert_eq!(p.cache_dir, PathBuf::from("/home/a/.cache/review-buddy"));
        assert_eq!(
            p.state_dir,
            PathBuf::from("/home/a/.local/state/review-buddy")
        );
        assert_eq!(p.runtime_dir, None);
        assert_eq!(p.lock_dir(), p.state_dir);
        assert_eq!(
            p.system_config_dirs,
            vec![PathBuf::from("/etc/xdg/review-buddy")]
        );
        assert_eq!(
            p.system_data_dirs,
            vec![
                PathBuf::from("/usr/local/share/review-buddy"),
                PathBuf::from("/usr/share/review-buddy")
            ]
        );
    }

    #[test]
    fn variables_override_and_invalid_ones_are_ignored() {
        let e = env("/Users/a")
            .with_var("XDG_CONFIG_HOME", "/x/cfg")
            .with_var("XDG_CACHE_HOME", "relative/cache")
            .with_var("XDG_STATE_HOME", "")
            .with_var("XDG_RUNTIME_DIR", "/run/user/1");
        let p = ResolvedPaths::resolve(&e).unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/x/cfg/review-buddy"));
        assert_eq!(p.cache_dir, PathBuf::from("/Users/a/.cache/review-buddy"));
        assert_eq!(
            p.state_dir,
            PathBuf::from("/Users/a/.local/state/review-buddy")
        );
        assert_eq!(p.lock_dir(), PathBuf::from("/run/user/1/review-buddy"));
    }

    #[test]
    fn runtime_dir_ignored_when_not_private() {
        let mut e = env("/h").with_var("XDG_RUNTIME_DIR", "/run/user/1");
        e.private_runtime = false;
        assert_eq!(ResolvedPaths::resolve(&e).unwrap().runtime_dir, None);
    }

    #[test]
    fn dir_lists_skip_relative_entries() {
        let e = env("/h").with_var("XDG_CONFIG_DIRS", "/a::rel:/b");
        let p = ResolvedPaths::resolve(&e).unwrap();
        assert_eq!(
            p.system_config_dirs,
            vec![
                PathBuf::from("/a/review-buddy"),
                PathBuf::from("/b/review-buddy")
            ]
        );
        let e = env("/h").with_var("XDG_DATA_DIRS", "rel");
        assert_eq!(
            ResolvedPaths::resolve(&e).unwrap().system_data_dirs.len(),
            2
        );
    }

    #[test]
    fn windows_layout_and_xdg_override() {
        let e = env("/home/a")
            .with_os(Os::Windows)
            .with_var("APPDATA", "/roam")
            .with_var("LOCALAPPDATA", "/local");
        let p = ResolvedPaths::resolve(&e).unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/roam/review-buddy"));
        assert_eq!(p.data_dir, PathBuf::from("/roam/review-buddy"));
        assert_eq!(p.cache_dir, PathBuf::from("/local/review-buddy/cache"));
        assert_eq!(p.state_dir, PathBuf::from("/local/review-buddy/state"));
        let p = ResolvedPaths::resolve(&e.with_var("XDG_CONFIG_HOME", "/x")).unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/x/review-buddy"));
    }

    #[test]
    fn missing_home_is_an_error_only_when_needed() {
        let mut e = MapEnv::new("/h");
        e.home = None;
        assert_eq!(ResolvedPaths::resolve(&e), Err(PathsError::NoHome));
        let e2 = MapEnv { home: None, ..e }
            .with_var("XDG_CONFIG_HOME", "/c")
            .with_var("XDG_DATA_HOME", "/d")
            .with_var("XDG_CACHE_HOME", "/e")
            .with_var("XDG_STATE_HOME", "/s");
        assert!(ResolvedPaths::resolve(&e2).is_ok());
    }

    #[test]
    fn theme_search_order() {
        let p = ResolvedPaths::resolve(&env("/h")).unwrap();
        let t = p.theme_dirs();
        assert_eq!(t[0], PathBuf::from("/h/.config/review-buddy/themes"));
        assert_eq!(t[1], PathBuf::from("/h/.local/share/review-buddy/themes"));
        assert_eq!(t[2], PathBuf::from("/usr/local/share/review-buddy/themes"));
        assert_eq!(t.len(), 4);
    }

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
    }

    fn fixture() -> (tempfile::TempDir, MapEnv) {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        let e = env(r.to_str().unwrap())
            .with_var("XDG_CONFIG_HOME", r.join("cfg").to_str().unwrap())
            .with_var(
                "XDG_CONFIG_DIRS",
                &format!("{}:{}", r.join("sys1").display(), r.join("sys2").display()),
            );
        (t, e)
    }

    fn names(report: &PathsReport, root: &Path) -> Vec<String> {
        report
            .config_files
            .iter()
            .map(|f| {
                format!(
                    "{}{}",
                    f.path.strip_prefix(root).unwrap().display(),
                    if f.exists { "*" } else { "" }
                )
            })
            .collect()
    }

    #[test]
    fn layering_order_without_override() {
        let (t, e) = fixture();
        let r = t.path();
        touch(&r.join("sys1/review-buddy/config.toml"));
        touch(&r.join("cfg/review-buddy/config.toml"));
        touch(&r.join("cfg/review-buddy/config.d/20-b.toml"));
        touch(&r.join("cfg/review-buddy/config.d/10-a.toml"));
        touch(&r.join("cfg/review-buddy/config.d/notes.txt"));
        let rep = PathsReport::build(&e, None).unwrap();
        assert_eq!(
            names(&rep, r),
            vec![
                "sys2/review-buddy/config.toml",
                "sys1/review-buddy/config.toml*",
                "cfg/review-buddy/config.toml*",
                "cfg/review-buddy/config.d/10-a.toml*",
                "cfg/review-buddy/config.d/20-b.toml*",
            ]
        );
        assert_eq!(rep.write_target, r.join("cfg/review-buddy/config.toml"));
        assert_eq!(rep.loaded().count(), 4);
    }

    #[test]
    fn env_override_replaces_user_and_drop_ins_and_cli_beats_env() {
        let (t, e) = fixture();
        let r = t.path();
        touch(&r.join("cfg/review-buddy/config.d/10-a.toml"));
        let envf = r.join("env.toml");
        let clif = r.join("cli.toml");
        touch(&envf);
        let e = e.with_var("REVIEW_BUDDY_CONFIG", envf.to_str().unwrap());

        let rep = PathsReport::build(&e, None).unwrap();
        let last = rep.config_files.last().unwrap();
        assert_eq!((last.origin, &last.path), (ConfigOrigin::EnvVar, &envf));
        assert_eq!(rep.config_files.len(), 3);
        assert_eq!(rep.write_target, envf);

        let rep = PathsReport::build(&e, Some(clif.clone())).unwrap();
        let last = rep.config_files.last().unwrap();
        assert_eq!(
            (last.origin, last.exists),
            (ConfigOrigin::CommandLine, false)
        );
        assert_eq!(rep.write_target, clif);
        assert!(rep
            .config_files
            .iter()
            .all(|f| f.origin != ConfigOrigin::EnvVar));
    }

    #[test]
    fn report_text_mentions_files_and_origins() {
        let (t, e) = fixture();
        touch(&t.path().join("cfg/review-buddy/config.toml"));
        let text = PathsReport::build(&e, None).unwrap().to_string();
        assert!(text.contains("[loaded]"));
        assert!(text.contains("[not found]"));
        assert!(text.contains("$XDG_CONFIG_DIRS"));
        assert!(text.contains("Settings are written to"));
    }
}

// Windows-style (drive letter) fixtures; `Path::is_absolute` is host-specific, so these run on Windows hosts.
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    fn env() -> MapEnv {
        MapEnv::new(r"C:\Users\a").with_os(Os::Windows)
    }

    #[test]
    fn appdata_layout() {
        let e = env()
            .with_var("APPDATA", r"C:\Users\a\AppData\Roaming")
            .with_var("LOCALAPPDATA", r"C:\Users\a\AppData\Local");
        let p = ResolvedPaths::resolve(&e).unwrap();
        assert_eq!(
            p.config_dir,
            PathBuf::from(r"C:\Users\a\AppData\Roaming\review-buddy")
        );
        assert_eq!(
            p.data_dir,
            PathBuf::from(r"C:\Users\a\AppData\Roaming\review-buddy")
        );
        assert_eq!(
            p.cache_dir,
            PathBuf::from(r"C:\Users\a\AppData\Local\review-buddy\cache")
        );
        assert_eq!(
            p.state_dir,
            PathBuf::from(r"C:\Users\a\AppData\Local\review-buddy\state")
        );
    }

    #[test]
    fn falls_back_to_home_when_appdata_is_unset() {
        let p = ResolvedPaths::resolve(&env()).unwrap();
        assert_eq!(
            p.config_dir,
            PathBuf::from(r"C:\Users\a")
                .join("AppData/Roaming")
                .join("review-buddy")
        );
        assert_eq!(
            p.cache_dir,
            PathBuf::from(r"C:\Users\a")
                .join("AppData/Local")
                .join("review-buddy")
                .join("cache")
        );
    }

    #[test]
    fn xdg_override_still_wins() {
        let e = env().with_var("XDG_CONFIG_HOME", r"D:\cfg");
        let p = ResolvedPaths::resolve(&e).unwrap();
        assert_eq!(p.config_dir, PathBuf::from(r"D:\cfg\review-buddy"));
    }

    #[test]
    fn relative_appdata_is_ignored() {
        let e = env().with_var("APPDATA", r"roam");
        let p = ResolvedPaths::resolve(&e).unwrap();
        assert_eq!(
            p.config_dir,
            PathBuf::from(r"C:\Users\a")
                .join("AppData/Roaming")
                .join("review-buddy")
        );
    }

    #[test]
    fn directory_lists_split_on_semicolons_not_drive_colons() {
        let e = env().with_var("XDG_CONFIG_DIRS", r"C:\a;rel;D:\b");
        let p = ResolvedPaths::resolve(&e).unwrap();
        assert_eq!(
            p.system_config_dirs,
            vec![
                PathBuf::from(r"C:\a\review-buddy"),
                PathBuf::from(r"D:\b\review-buddy")
            ]
        );
    }
}
