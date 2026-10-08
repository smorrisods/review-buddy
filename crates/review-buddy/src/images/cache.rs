//! The on-disk cache of fetched image bytes, so reopening a change doesn't fetch again and
//! cached images still show offline.
//!
//! Files are named by a hash of the address, so nothing about a private repository leaks through
//! a file name, and live in a private directory. Reads refresh the file's time and a write that
//! takes the cache past its cap removes the least recently used files first.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha1::{Digest, Sha1};
use url::Url;

pub const DEFAULT_CAP: u64 = 100 * 1024 * 1024;
/// How long a cached copy is used without asking the server again.
pub const FRESH_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone)]
pub struct DiskCache {
    dir: PathBuf,
    cap: u64,
}

/// What the cache holds for an address.
#[derive(Debug, PartialEq, Eq)]
pub struct Hit {
    pub bytes: Vec<u8>,
    pub fresh: bool,
}

impl DiskCache {
    pub fn new(dir: PathBuf, cap: u64) -> Self {
        Self { dir, cap }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file name for an address: its SHA-1 in hex. The fragment is ignored.
    pub fn key(url: &Url) -> String {
        let mut url = url.clone();
        url.set_fragment(None);
        let digest = Sha1::digest(url.as_str().as_bytes());
        let mut hex = String::with_capacity(40);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }

    fn path(&self, url: &Url) -> PathBuf {
        self.dir.join(format!("{}.img", Self::key(url)))
    }

    pub fn get(&self, url: &Url) -> Option<Hit> {
        let path = self.path(url);
        let bytes = fs::read(&path).ok()?;
        let age = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok());
        touch(&path);
        Some(Hit {
            bytes,
            fresh: age.is_some_and(|a| a < FRESH_FOR),
        })
    }

    pub fn put(&self, url: &Url, bytes: &[u8]) -> io::Result<()> {
        rb_paths::ensure_private_dir(&self.dir)?;
        rb_paths::write_private_file(&self.path(url), bytes)?;
        self.prune();
        Ok(())
    }

    /// Drops a copy that turned out not to decode.
    pub fn forget(&self, url: &Url) {
        let _ = fs::remove_file(self.path(url));
    }

    fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "img"))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((meta.modified().ok()?, meta.len(), e.path()))
            })
            .collect();
        let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
        if total <= self.cap {
            return;
        }
        files.sort();
        for (_, len, path) in files {
            if total <= self.cap * 4 / 5 {
                break;
            }
            if fs::remove_file(path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

fn touch(path: &Path) {
    if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(tail: &str) -> Url {
        Url::parse(&format!("https://x.test/{tail}")).unwrap()
    }

    fn cache(cap: u64) -> (tempfile::TempDir, DiskCache) {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().join("images"), cap);
        (dir, cache)
    }

    #[test]
    fn keys_are_stable_hashes_that_ignore_the_fragment() {
        let a = DiskCache::key(&url("a.png?x=1#top"));
        assert_eq!(a, DiskCache::key(&url("a.png?x=1#other")));
        assert_ne!(a, DiskCache::key(&url("a.png?x=2")));
        assert_eq!(a.len(), 40);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!a.contains("x.test"));
    }

    #[test]
    fn a_stored_image_comes_back_fresh() {
        let (_guard, cache) = cache(DEFAULT_CAP);
        assert_eq!(cache.get(&url("a.png")), None);
        cache.put(&url("a.png"), b"bytes").unwrap();
        let hit = cache.get(&url("a.png")).unwrap();
        assert_eq!(hit.bytes, b"bytes");
        assert!(hit.fresh);
        cache.forget(&url("a.png"));
        assert_eq!(cache.get(&url("a.png")), None);
    }

    #[test]
    fn an_old_copy_is_stale_but_still_returned() {
        let (_guard, cache) = cache(DEFAULT_CAP);
        cache.put(&url("a.png"), b"old").unwrap();
        let file = fs::File::options()
            .write(true)
            .open(cache.path(&url("a.png")))
            .unwrap();
        file.set_modified(SystemTime::now() - FRESH_FOR - Duration::from_secs(60))
            .unwrap();
        let hit = cache.get(&url("a.png")).unwrap();
        assert!(!hit.fresh);
        assert_eq!(hit.bytes, b"old");
    }

    #[test]
    fn the_least_recently_used_files_go_first_when_over_the_cap() {
        let (_guard, cache) = cache(100);
        let old = SystemTime::now() - Duration::from_secs(3600);
        for (n, name) in ["one.png", "two.png", "three.png"].into_iter().enumerate() {
            cache.put(&url(name), &[0u8; 30]).unwrap();
            let file = fs::File::options()
                .write(true)
                .open(cache.path(&url(name)))
                .unwrap();
            file.set_modified(old + Duration::from_secs(n as u64 * 60))
                .unwrap();
        }
        assert!(cache.get(&url("one.png")).is_some(), "reading refreshes it");
        cache.put(&url("four.png"), &[0u8; 30]).unwrap();
        assert!(
            cache.get(&url("two.png")).is_none(),
            "the oldest untouched file is dropped"
        );
        assert!(cache.get(&url("one.png")).is_some());
        assert!(cache.get(&url("four.png")).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn the_directory_and_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_guard, cache) = cache(DEFAULT_CAP);
        cache.put(&url("a.png"), b"x").unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(cache.dir()), 0o700);
        assert_eq!(mode(&cache.path(&url("a.png"))), 0o600);
    }
}
