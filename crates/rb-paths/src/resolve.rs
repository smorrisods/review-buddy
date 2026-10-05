use crate::env::{Env, Os};
use std::path::{Path, PathBuf};

/// Directory name appended to every base directory.
pub const APP_DIR: &str = "review-buddy";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathsError {
    NoHome,
}

impl std::fmt::Display for PathsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoHome => f.write_str(
                "can't find your home directory; set HOME (or the XDG_* variables) and try again",
            ),
        }
    }
}

impl std::error::Error for PathsError {}

/// All resolved application directories (each already includes `review-buddy`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub state_dir: PathBuf,
    /// Present only when `$XDG_RUNTIME_DIR` is usable.
    pub runtime_dir: Option<PathBuf>,
    /// Most important first, as in the spec.
    pub system_config_dirs: Vec<PathBuf>,
    /// Most important first, as in the spec.
    pub system_data_dirs: Vec<PathBuf>,
}

impl ResolvedPaths {
    pub fn resolve(env: &dyn Env) -> Result<Self, PathsError> {
        let home = env.home_dir();
        let xdg = |key: &str| abs_var(env, key);
        let from_home = |rel: &str| home.as_ref().map(|h| h.join(rel)).ok_or(PathsError::NoHome);
        let app = |p: PathBuf| p.join(APP_DIR);

        let (config, data, cache, state);
        if env.os() == Os::Windows {
            let win = |key: &str| abs_var(env, key);
            config = match xdg("XDG_CONFIG_HOME").or_else(|| win("APPDATA")) {
                Some(p) => app(p),
                None => app(from_home("AppData/Roaming")?),
            };
            data = match xdg("XDG_DATA_HOME").or_else(|| win("APPDATA")) {
                Some(p) => app(p),
                None => app(from_home("AppData/Roaming")?),
            };
            let local = win("LOCALAPPDATA");
            let local_base = |name: &str| -> Result<PathBuf, PathsError> {
                Ok(match &local {
                    Some(p) => app(p.clone()).join(name),
                    None => app(from_home("AppData/Local")?).join(name),
                })
            };
            cache = match xdg("XDG_CACHE_HOME") {
                Some(p) => app(p),
                None => local_base("cache")?,
            };
            state = match xdg("XDG_STATE_HOME") {
                Some(p) => app(p),
                None => local_base("state")?,
            };
        } else {
            config = app(xdg("XDG_CONFIG_HOME").map_or_else(|| from_home(".config"), Ok)?);
            data = app(xdg("XDG_DATA_HOME").map_or_else(|| from_home(".local/share"), Ok)?);
            cache = app(xdg("XDG_CACHE_HOME").map_or_else(|| from_home(".cache"), Ok)?);
            state = app(xdg("XDG_STATE_HOME").map_or_else(|| from_home(".local/state"), Ok)?);
        }

        let runtime_dir = xdg("XDG_RUNTIME_DIR")
            .filter(|p| env.runtime_dir_is_private(p))
            .map(app);

        let system_config_dirs = dir_list(env, "XDG_CONFIG_DIRS", "/etc/xdg");
        let system_data_dirs = dir_list(env, "XDG_DATA_DIRS", "/usr/local/share:/usr/share");

        Ok(Self {
            config_dir: config,
            data_dir: data,
            cache_dir: cache,
            state_dir: state,
            runtime_dir,
            system_config_dirs,
            system_data_dirs,
        })
    }

    /// Where `instance.lock` lives: the runtime dir, else the state dir.
    pub fn lock_dir(&self) -> &Path {
        self.runtime_dir.as_deref().unwrap_or(&self.state_dir)
    }

    /// Theme directories in search order; the first match by id wins.
    pub fn theme_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.config_dir.join("themes"), self.data_dir.join("themes")];
        dirs.extend(self.system_data_dirs.iter().map(|d| d.join("themes")));
        dirs
    }
}

fn abs_var(env: &dyn Env, key: &str) -> Option<PathBuf> {
    env.var(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// Parses a path list (`;`-separated on Windows, where `:` belongs to drive letters, otherwise `:`),
/// dropping empty and relative entries.
fn dir_list(env: &dyn Env, key: &str, default: &str) -> Vec<PathBuf> {
    let sep = if env.os() == Os::Windows { ';' } else { ':' };
    let parse = |s: &str| -> Vec<PathBuf> {
        s.split(sep)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| p.join(APP_DIR))
            .collect()
    };
    let list = env.var(key).map(|v| parse(&v)).unwrap_or_default();
    if list.is_empty() {
        parse(default)
    } else {
        list
    }
}
