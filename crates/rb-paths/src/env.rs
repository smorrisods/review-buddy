use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Platform family; only Windows changes where directories live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Unix,
    Windows,
}

/// Source of environment variables and filesystem facts used during resolution.
pub trait Env {
    fn var(&self, key: &str) -> Option<String>;
    fn home_dir(&self) -> Option<PathBuf>;
    fn os(&self) -> Os;
    /// Whether a runtime directory is owned by the current user with mode `0700`.
    fn runtime_dir_is_private(&self, path: &Path) -> bool;
}

/// The real process environment.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        let key = if self.os() == Os::Windows {
            "USERPROFILE"
        } else {
            "HOME"
        };
        std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    }

    fn os(&self) -> Os {
        if cfg!(windows) {
            Os::Windows
        } else {
            Os::Unix
        }
    }

    #[cfg(unix)]
    fn runtime_dir_is_private(&self, path: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        // Without libc, the owner of $HOME stands in for the current uid.
        let owner_ok = match self.home_dir().and_then(|h| std::fs::metadata(h).ok()) {
            Some(home) => home.uid() == meta.uid(),
            None => false,
        };
        meta.is_dir() && meta.mode() & 0o777 == 0o700 && owner_ok
    }

    #[cfg(not(unix))]
    fn runtime_dir_is_private(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

/// An in-memory environment for tests.
#[derive(Debug, Clone)]
pub struct MapEnv {
    pub vars: HashMap<String, String>,
    pub home: Option<PathBuf>,
    pub os: Os,
    pub private_runtime: bool,
}

impl MapEnv {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            vars: HashMap::new(),
            home: Some(home.into()),
            os: if cfg!(windows) { Os::Windows } else { Os::Unix },
            private_runtime: true,
        }
    }

    pub fn with_var(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_os(mut self, os: Os) -> Self {
        self.os = os;
        self
    }
}

impl Env for MapEnv {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn os(&self) -> Os {
        self.os
    }

    fn runtime_dir_is_private(&self, _path: &Path) -> bool {
        self.private_runtime
    }
}
