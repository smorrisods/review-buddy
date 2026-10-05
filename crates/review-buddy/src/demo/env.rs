//! The throwaway directories demo mode uses instead of the real XDG ones.

use std::io;
use std::path::{Path, PathBuf};

use rb_paths::ensure_private_dir;

/// Config, data, cache and state directories inside a temporary directory that is removed on drop.
#[derive(Debug)]
pub struct DemoEnv {
    root: tempfile::TempDir,
}

impl DemoEnv {
    pub fn new() -> io::Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("review-buddy-demo-")
            .tempdir()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        let env = Self { root };
        for dir in [
            env.config_dir(),
            env.data_dir(),
            env.cache_dir(),
            env.state_dir(),
        ] {
            ensure_private_dir(&dir)?;
        }
        Ok(env)
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn config_dir(&self) -> PathBuf {
        self.root.path().join("config")
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.path().join("data")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.path().join("cache")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.path().join("state")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_four_empty_dirs_and_removes_them_on_drop() {
        let env = DemoEnv::new().unwrap();
        let root = env.root().to_path_buf();
        for dir in [
            env.config_dir(),
            env.data_dir(),
            env.cache_dir(),
            env.state_dir(),
        ] {
            assert!(dir.is_dir());
            assert!(dir.starts_with(&root));
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        }
        drop(env);
        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn directories_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let env = DemoEnv::new().unwrap();
        for dir in [env.root().to_path_buf(), env.config_dir(), env.state_dir()] {
            let mode = std::fs::metadata(dir).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
    }
}
