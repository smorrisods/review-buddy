use crate::env::Env;
use crate::resolve::ResolvedPaths;
use std::fmt;
use std::path::{Path, PathBuf};

/// Why a config file is in the merge list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigOrigin {
    /// A `$XDG_CONFIG_DIRS` entry (admin or distro defaults).
    SystemDir,
    /// `$XDG_CONFIG_HOME/review-buddy/config.toml`.
    User,
    /// `$XDG_CONFIG_HOME/review-buddy/config.d/*.toml`.
    DropIn,
    /// `$REVIEW_BUDDY_CONFIG`.
    EnvVar,
    /// `--config <path>`.
    CommandLine,
}

impl fmt::Display for ConfigOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SystemDir => "$XDG_CONFIG_DIRS",
            Self::User => "user config",
            Self::DropIn => "config.d",
            Self::EnvVar => "$REVIEW_BUDDY_CONFIG",
            Self::CommandLine => "--config",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFile {
    pub path: PathBuf,
    pub origin: ConfigOrigin,
    pub exists: bool,
}

/// Config files in merge order (later overrides earlier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLayers {
    pub files: Vec<ConfigFile>,
    /// The only file the app writes settings to.
    pub write_target: PathBuf,
}

impl ConfigLayers {
    /// Builds the layer list from `paths`, checking the filesystem for existence
    /// and for `config.d` entries. An override (`--config`, else
    /// `$REVIEW_BUDDY_CONFIG`) replaces the user file and `config.d`.
    pub fn discover(paths: &ResolvedPaths, env: &dyn Env, cli_config: Option<PathBuf>) -> Self {
        let mut files = Vec::new();
        for dir in paths.system_config_dirs.iter().rev() {
            files.push(file(dir.join("config.toml"), ConfigOrigin::SystemDir));
        }

        let env_config = env
            .var("REVIEW_BUDDY_CONFIG")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let over = cli_config
            .map(|p| (p, ConfigOrigin::CommandLine))
            .or_else(|| env_config.map(|p| (p, ConfigOrigin::EnvVar)));

        let write_target = match over {
            Some((path, origin)) => {
                files.push(file(path.clone(), origin));
                path
            }
            None => {
                let user = paths.config_dir.join("config.toml");
                files.push(file(user.clone(), ConfigOrigin::User));
                for p in drop_ins(&paths.config_dir.join("config.d")) {
                    files.push(file(p, ConfigOrigin::DropIn));
                }
                user
            }
        };
        Self {
            files,
            write_target,
        }
    }
}

fn file(path: PathBuf, origin: ConfigOrigin) -> ConfigFile {
    let exists = path.is_file();
    ConfigFile {
        path,
        origin,
        exists,
    }
}

/// `*.toml` files in lexical order; a missing or unreadable directory is empty.
fn drop_ins(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml") && p.is_file())
        .collect();
    out.sort();
    out
}
