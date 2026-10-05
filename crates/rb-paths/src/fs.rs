use std::io;
use std::path::Path;

/// Creates `path` and any missing parents with mode `0700` (default modes on Windows).
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Writes `contents` to `path` with mode `0600`, creating the parent directory
/// privately. An existing file is truncated and tightened to `0600`.
pub fn write_private_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn dirs_are_0700_including_parents() {
        let t = tempfile::tempdir().unwrap();
        let leaf = t.path().join("a/b/c");
        ensure_private_dir(&leaf).unwrap();
        assert_eq!(mode(&leaf), 0o700);
        assert_eq!(mode(&t.path().join("a")), 0o700);
        ensure_private_dir(&leaf).unwrap();
    }

    #[test]
    fn files_are_0600_and_existing_files_are_tightened() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("x/session.toml");
        write_private_file(&f, b"one").unwrap();
        assert_eq!(mode(&f), 0o600);
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private_file(&f, b"two").unwrap();
        assert_eq!(mode(&f), 0o600);
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
    }
}
