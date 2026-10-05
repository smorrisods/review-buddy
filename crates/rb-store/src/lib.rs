//! Review Buddy's disposable SQLite cache and ETag store.
//!
//! Everything here can be deleted at any time: a missing, corrupt, or newer-than-supported database
//! is replaced with an empty one on open, and the app simply refetches. The API is synchronous;
//! [`Store`] is `Send` but not `Sync`, so give each thread its own handle or wrap it in a mutex.

use std::fs;
use std::path::{Path, PathBuf};

use rb_core::{ChangeDetail, ChangeId, ChangeSummary, Etag, SourceId, Timestamp};
use rusqlite::{params, Connection, OptionalExtension};

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cache database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("couldn't read or write the cache at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cached data couldn't be decoded: {0}")]
    Json(#[from] serde_json::Error),
}

/// Ordered migrations; entry `n` upgrades schema version `n` to `n + 1`.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE changes (
        source_id    TEXT NOT NULL,
        repo         TEXT NOT NULL,
        number       INTEGER NOT NULL,
        state        TEXT NOT NULL,
        updated_at   INTEGER NOT NULL,
        fetched_at   INTEGER NOT NULL,
        summary_json TEXT NOT NULL,
        detail_json  TEXT,
        PRIMARY KEY (source_id, repo, number)
    ) WITHOUT ROWID;
    CREATE INDEX changes_by_updated ON changes (source_id, updated_at DESC);
    CREATE TABLE etags (
        source_id  TEXT NOT NULL,
        request    TEXT NOT NULL,
        etag       TEXT NOT NULL,
        fetched_at INTEGER NOT NULL,
        PRIMARY KEY (source_id, request)
    ) WITHOUT ROWID;
"];

/// The schema version this build writes.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

pub struct Store {
    conn: Connection,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Opens (creating if needed) the cache at `path`. An unreadable, corrupt, or newer-schema
    /// database is deleted and rebuilt empty.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|source| io_err(dir, source))?;
            set_mode(dir, 0o700);
        }
        match Self::try_open(path) {
            Ok(store) => Ok(store),
            Err(_) => {
                remove_db_files(path)?;
                Self::try_open(path)
            }
        }
    }

    /// An in-memory store, for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    fn try_open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(StoreError::Sqlite(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
                Some(check),
            )));
        }
        migrate(&conn)?;
        set_mode(path, 0o600);
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<u32> {
        user_version(&self.conn)
    }

    /// Upserts summaries for a source, keeping any cached detail whose head SHA is unchanged.
    pub fn put_summaries(
        &mut self,
        source: &SourceId,
        summaries: &[ChangeSummary],
        now: Timestamp,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        for s in summaries {
            upsert_summary(&tx, source, s, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Like [`put_summaries`](Self::put_summaries), but drops the source's other cached changes
    /// (merged, closed, or out of scope since the last full refresh).
    pub fn replace_summaries(
        &mut self,
        source: &SourceId,
        summaries: &[ChangeSummary],
        now: Timestamp,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM changes WHERE source_id = ?1",
            params![source.as_str()],
        )?;
        for s in summaries {
            upsert_summary(&tx, source, s, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Cached summaries for a source, most recently updated first.
    pub fn list_summaries(&self, source: &SourceId) -> Result<Vec<ChangeSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT summary_json FROM changes WHERE source_id = ?1
             ORDER BY updated_at DESC, repo, number DESC",
        )?;
        let rows = stmt.query_map(params![source.as_str()], |r| r.get::<_, String>(0))?;
        rows.map(|json| Ok(serde_json::from_str(&json?)?)).collect()
    }

    pub fn get_summary(&self, id: &ChangeId) -> Result<Option<ChangeSummary>> {
        self.json_column(id, "summary_json")
    }

    /// Stores a detail, also refreshing its summary row.
    pub fn put_detail(&mut self, detail: &ChangeDetail, now: Timestamp) -> Result<()> {
        let tx = self.conn.transaction()?;
        let source = detail.summary.id.source_id.clone();
        upsert_summary(&tx, &source, &detail.summary, now)?;
        tx.execute(
            "UPDATE changes SET detail_json = ?4
             WHERE source_id = ?1 AND repo = ?2 AND number = ?3",
            params![
                source.as_str(),
                detail.summary.id.repo,
                detail.summary.id.number,
                serde_json::to_string(detail)?
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_detail(&self, id: &ChangeId) -> Result<Option<ChangeDetail>> {
        self.json_column(id, "detail_json")
    }

    /// When the change was last stored, for "cached 10:42" labels.
    pub fn fetched_at(&self, id: &ChangeId) -> Result<Option<Timestamp>> {
        Ok(self
            .conn
            .query_row(
                "SELECT fetched_at FROM changes WHERE source_id = ?1 AND repo = ?2 AND number = ?3",
                params![id.source_id.as_str(), id.repo, id.number],
                |r| r.get(0),
            )
            .optional()?
            .map(Timestamp))
    }

    /// The newest fetch time across a source's cached changes.
    pub fn last_fetched(&self, source: &SourceId) -> Result<Option<Timestamp>> {
        let ts: Option<i64> = self.conn.query_row(
            "SELECT MAX(fetched_at) FROM changes WHERE source_id = ?1",
            params![source.as_str()],
            |r| r.get(0),
        )?;
        Ok(ts.map(Timestamp))
    }

    pub fn remove_change(&mut self, id: &ChangeId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM changes WHERE source_id = ?1 AND repo = ?2 AND number = ?3",
            params![id.source_id.as_str(), id.repo, id.number],
        )?;
        Ok(())
    }

    pub fn etag(&self, source: &SourceId, request: &str) -> Result<Option<Etag>> {
        Ok(self
            .conn
            .query_row(
                "SELECT etag FROM etags WHERE source_id = ?1 AND request = ?2",
                params![source.as_str(), request],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(Etag))
    }

    pub fn set_etag(
        &mut self,
        source: &SourceId,
        request: &str,
        etag: &Etag,
        now: Timestamp,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO etags (source_id, request, etag, fetched_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (source_id, request)
             DO UPDATE SET etag = excluded.etag, fetched_at = excluded.fetched_at",
            params![source.as_str(), request, etag.as_str(), now.0],
        )?;
        Ok(())
    }

    pub fn clear_etag(&mut self, source: &SourceId, request: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM etags WHERE source_id = ?1 AND request = ?2",
            params![source.as_str(), request],
        )?;
        Ok(())
    }

    /// Forgets everything cached for a source, including its ETags.
    pub fn clear_source(&mut self, source: &SourceId) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM changes WHERE source_id = ?1",
            params![source.as_str()],
        )?;
        tx.execute(
            "DELETE FROM etags WHERE source_id = ?1",
            params![source.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Empties every table, keeping the schema.
    pub fn clear_all(&mut self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM changes; DELETE FROM etags;")?;
        Ok(())
    }

    /// Deletes the database files at `path` (the cache is rebuilt on the next [`open`](Self::open)).
    pub fn delete(path: &Path) -> Result<()> {
        remove_db_files(path)
    }

    fn json_column<T: serde::de::DeserializeOwned>(
        &self,
        id: &ChangeId,
        column: &str,
    ) -> Result<Option<T>> {
        let sql = format!(
            "SELECT {column} FROM changes WHERE source_id = ?1 AND repo = ?2 AND number = ?3"
        );
        let json: Option<Option<String>> = self
            .conn
            .query_row(
                &sql,
                params![id.source_id.as_str(), id.repo, id.number],
                |r| r.get(0),
            )
            .optional()?;
        match json.flatten() {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }
}

fn upsert_summary(
    conn: &Connection,
    source: &SourceId,
    s: &ChangeSummary,
    now: Timestamp,
) -> Result<()> {
    let state = serde_json::to_value(s.state)?;
    conn.execute(
        "INSERT INTO changes (source_id, repo, number, state, updated_at, fetched_at, summary_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (source_id, repo, number) DO UPDATE SET
            state = excluded.state,
            updated_at = excluded.updated_at,
            fetched_at = excluded.fetched_at,
            summary_json = excluded.summary_json,
            detail_json = CASE
                WHEN json_extract(changes.summary_json, '$.head_sha')
                   = json_extract(excluded.summary_json, '$.head_sha')
                THEN changes.detail_json END",
        params![
            source.as_str(),
            s.id.repo,
            s.id.number,
            state.as_str().unwrap_or_default(),
            s.updated_at.0,
            now.0,
            serde_json::to_string(s)?
        ],
    )?;
    Ok(())
}

fn user_version(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

fn migrate(conn: &Connection) -> Result<()> {
    let current = user_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(StoreError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_SCHEMA),
            Some(format!("schema version {current} is newer than supported")),
        )));
    }
    for (version, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        conn.execute_batch(&format!(
            "BEGIN; {sql} PRAGMA user_version = {}; COMMIT;",
            version + 1
        ))?;
    }
    Ok(())
}

fn remove_db_files(path: &Path) -> Result<()> {
    let base = path.as_os_str().to_owned();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = base.clone();
        name.push(suffix);
        let file = PathBuf::from(name);
        match fs::remove_file(&file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(io_err(&file, source)),
        }
    }
    Ok(())
}

fn io_err(path: &Path, source: std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use rb_core::*;

    fn sid(s: &str) -> SourceId {
        SourceId::new(s)
    }

    fn summary(source: &str, repo: &str, number: u64, updated: i64, sha: &str) -> ChangeSummary {
        ChangeSummary {
            id: ChangeId {
                source_id: sid(source),
                kind: ForgeKind::GitHub,
                repo: repo.into(),
                number,
            },
            title: format!("Change {number}"),
            author: "ada".into(),
            author_is_bot: false,
            state: ChangeState::Open,
            draft: false,
            created_at: Timestamp(1),
            updated_at: Timestamp(updated),
            branch: "feat".into(),
            base: "main".into(),
            head_sha: sha.into(),
            base_sha: "base".into(),
            adds: 1,
            dels: 2,
            files: 3,
            ci: CiState::Pass,
            labels: vec!["x".into()],
            reviewers: vec![],
            my_role: MyRole::Reviewing,
            my_review: MyReview::None,
            my_reviewed_sha: None,
            i_commented: false,
            has_new_activity: false,
        }
    }

    fn detail(s: ChangeSummary) -> ChangeDetail {
        ChangeDetail {
            summary: s,
            body: "body".into(),
            web_url: "https://example.com/pr/1".parse().unwrap(),
            mergeable: Some(true),
            mergeability: Mergeability::Clean,
            commit_count: 3,
        }
    }

    #[test]
    fn detail_cached_before_model_gaps_still_loads() {
        let mut store = Store::open_in_memory().unwrap();
        let d = detail(summary("s", "o/r", 1, 10, "sha"));
        let mut json = serde_json::to_value(&d).unwrap();
        let obj = json.as_object_mut().unwrap();
        obj.remove("mergeability");
        obj.remove("commit_count");
        let raw = serde_json::to_string(&json).unwrap();
        assert!(!raw.contains("commit_count"));
        store.put_detail(&d, Timestamp(1)).unwrap();
        store
            .conn
            .execute("UPDATE changes SET detail_json = ?1", [raw])
            .unwrap();
        let old = store.get_detail(&d.summary.id).unwrap().unwrap();
        assert_eq!(old.commit_count, 0);
        assert_eq!(old.mergeability, Mergeability::Unknown);
        assert_eq!(old.summary, d.summary);
    }

    #[test]
    fn migrates_to_latest_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn uses_wal_mode() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("a/b/cache.sqlite")).unwrap();
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn summary_round_trip_ordered_by_recency() {
        let mut store = Store::open_in_memory().unwrap();
        let a = summary("s1", "o/r", 1, 10, "a");
        let b = summary("s1", "o/r", 2, 20, "b");
        let other = summary("s2", "o/r", 1, 30, "c");
        store
            .put_summaries(&sid("s1"), &[a.clone(), b.clone()], Timestamp(100))
            .unwrap();
        store
            .put_summaries(&sid("s2"), std::slice::from_ref(&other), Timestamp(100))
            .unwrap();
        assert_eq!(
            store.list_summaries(&sid("s1")).unwrap(),
            vec![b, a.clone()]
        );
        assert_eq!(store.get_summary(&a.id).unwrap(), Some(a.clone()));
        assert_eq!(store.fetched_at(&a.id).unwrap(), Some(Timestamp(100)));
        assert_eq!(
            store.last_fetched(&sid("s2")).unwrap(),
            Some(Timestamp(100))
        );
        assert_eq!(store.last_fetched(&sid("none")).unwrap(), None);
    }

    #[test]
    fn replace_drops_missing_changes() {
        let mut store = Store::open_in_memory().unwrap();
        let a = summary("s1", "o/r", 1, 10, "a");
        let b = summary("s1", "o/r", 2, 20, "b");
        store
            .put_summaries(&sid("s1"), &[a.clone(), b.clone()], Timestamp(1))
            .unwrap();
        store
            .replace_summaries(&sid("s1"), std::slice::from_ref(&b), Timestamp(2))
            .unwrap();
        assert_eq!(store.list_summaries(&sid("s1")).unwrap(), vec![b]);
        assert_eq!(store.get_summary(&a.id).unwrap(), None);
    }

    #[test]
    fn detail_round_trip_and_invalidation_on_new_head() {
        let mut store = Store::open_in_memory().unwrap();
        let s = summary("s1", "o/r", 1, 10, "a");
        assert_eq!(store.get_detail(&s.id).unwrap(), None);
        let d = detail(s.clone());
        store.put_detail(&d, Timestamp(5)).unwrap();
        assert_eq!(store.get_detail(&s.id).unwrap(), Some(d.clone()));

        let mut same = s.clone();
        same.title = "Renamed".into();
        store
            .put_summaries(&sid("s1"), &[same], Timestamp(6))
            .unwrap();
        assert_eq!(store.get_detail(&s.id).unwrap(), Some(d));

        let moved = summary("s1", "o/r", 1, 11, "b");
        store
            .put_summaries(&sid("s1"), std::slice::from_ref(&moved), Timestamp(7))
            .unwrap();
        assert_eq!(store.get_detail(&s.id).unwrap(), None);
        assert_eq!(store.get_summary(&s.id).unwrap(), Some(moved));
    }

    #[test]
    fn etag_set_get_overwrite_clear_and_scoping() {
        let mut store = Store::open_in_memory().unwrap();
        let (s1, s2) = (sid("s1"), sid("s2"));
        assert_eq!(store.etag(&s1, "list").unwrap(), None);
        store
            .set_etag(&s1, "list", &Etag::new("\"v1\""), Timestamp(1))
            .unwrap();
        store
            .set_etag(&s2, "list", &Etag::new("\"other\""), Timestamp(1))
            .unwrap();
        store
            .set_etag(&s1, "list", &Etag::new("\"v2\""), Timestamp(2))
            .unwrap();
        assert_eq!(store.etag(&s1, "list").unwrap(), Some(Etag::new("\"v2\"")));
        assert_eq!(
            store.etag(&s2, "list").unwrap(),
            Some(Etag::new("\"other\""))
        );
        assert_eq!(store.etag(&s1, "detail").unwrap(), None);
        store.clear_etag(&s1, "list").unwrap();
        assert_eq!(store.etag(&s1, "list").unwrap(), None);
        assert!(store.etag(&s2, "list").unwrap().is_some());
    }

    #[test]
    fn clear_source_and_all() {
        let mut store = Store::open_in_memory().unwrap();
        for s in ["s1", "s2"] {
            store
                .put_summaries(&sid(s), &[summary(s, "o/r", 1, 1, "a")], Timestamp(1))
                .unwrap();
            store
                .set_etag(&sid(s), "list", &Etag::new("e"), Timestamp(1))
                .unwrap();
        }
        store.clear_source(&sid("s1")).unwrap();
        assert!(store.list_summaries(&sid("s1")).unwrap().is_empty());
        assert_eq!(store.etag(&sid("s1"), "list").unwrap(), None);
        assert_eq!(store.list_summaries(&sid("s2")).unwrap().len(), 1);
        store.clear_all().unwrap();
        assert!(store.list_summaries(&sid("s2")).unwrap().is_empty());
        assert_eq!(store.etag(&sid("s2"), "list").unwrap(), None);
    }

    #[test]
    fn remove_change_removes_one() {
        let mut store = Store::open_in_memory().unwrap();
        let a = summary("s1", "o/r", 1, 1, "a");
        let b = summary("s1", "o/r", 2, 2, "b");
        store
            .put_summaries(&sid("s1"), &[a.clone(), b.clone()], Timestamp(1))
            .unwrap();
        store.remove_change(&a.id).unwrap();
        assert_eq!(store.list_summaries(&sid("s1")).unwrap(), vec![b]);
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        let a = summary("s1", "o/r", 1, 1, "a");
        {
            let mut store = Store::open(&path).unwrap();
            store
                .put_summaries(&sid("s1"), std::slice::from_ref(&a), Timestamp(1))
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.list_summaries(&sid("s1")).unwrap(), vec![a]);
    }

    #[test]
    fn corrupt_file_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        fs::write(&path, vec![0x42u8; 8192]).unwrap();
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(store.list_summaries(&sid("s1")).unwrap().is_empty());
    }

    #[test]
    fn newer_schema_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5)
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn delete_then_open_rebuilds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        let mut store = Store::open(&path).unwrap();
        store
            .put_summaries(&sid("s1"), &[summary("s1", "o/r", 1, 1, "a")], Timestamp(1))
            .unwrap();
        drop(store);
        Store::delete(&path).unwrap();
        assert!(!path.exists());
        Store::delete(&path).unwrap();
        let store = Store::open(&path).unwrap();
        assert!(store.list_summaries(&sid("s1")).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn creates_private_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/cache.sqlite");
        Store::open(&path).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn store_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Store>();
    }
}
