use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{Result, ShabkaError};

/// SQLite-backed storage for Shabka memories.
///
/// Uses a single `Connection` behind `Arc<Mutex<>>` so it can be shared
/// across async tasks.  All blocking SQLite calls go through
/// [`with_conn`](Self::with_conn) which runs them on the Tokio blocking
/// thread-pool.
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl SqliteStorage {
    /// Open (or create) a file-backed SQLite database at `path`.
    ///
    /// Sets WAL journal mode and enables foreign keys, then creates all
    /// tables and indexes if they don't already exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)
            .map_err(|e| ShabkaError::Storage(format!("failed to open SQLite database: {e}")))?;

        Self::configure_and_init(conn, path)
    }

    /// Open an in-memory SQLite database (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| {
            ShabkaError::Storage(format!("failed to open in-memory SQLite database: {e}"))
        })?;

        Self::configure_and_init(conn, PathBuf::from(":memory:"))
    }

    /// Return the path this database was opened with (`:memory:` for in-memory).
    pub fn path(&self) -> &Path {
        &self.path
    }

    // ── helpers ────────────────────────────────────────────────────────

    /// Shared initialisation: pragmas + table creation.
    fn configure_and_init(conn: Connection, path: PathBuf) -> Result<Self> {
        // WAL mode for better concurrent-read performance.
        conn.execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(|e| ShabkaError::Storage(format!("failed to set WAL mode: {e}")))?;

        // Enforce foreign-key constraints.
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| ShabkaError::Storage(format!("failed to enable foreign keys: {e}")))?;

        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
        };

        storage.create_tables()?;
        Ok(storage)
    }

    /// Create all tables and indexes (idempotent).
    fn create_tables(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ShabkaError::Storage(format!("failed to acquire database lock: {e}")))?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                summary TEXT NOT NULL DEFAULT '',
                tags TEXT NOT NULL DEFAULT '[]',
                source TEXT NOT NULL DEFAULT '\"manual\"',
                scope TEXT NOT NULL DEFAULT '\"global\"',
                importance REAL NOT NULL DEFAULT 0.5,
                status TEXT NOT NULL DEFAULT 'active',
                privacy TEXT NOT NULL DEFAULT 'private',
                verification TEXT NOT NULL DEFAULT 'unverified',
                project_id TEXT,
                session_id TEXT,
                created_by TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                accessed_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS embeddings (
                memory_id TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
                vector BLOB NOT NULL,
                dimensions INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS relations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                target_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                relation_type TEXT NOT NULL DEFAULT 'related',
                strength REAL NOT NULL DEFAULT 0.5,
                UNIQUE(source_id, target_id, relation_type)
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                summary TEXT,
                memory_count INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_memories_project_id ON memories(project_id);
            CREATE INDEX IF NOT EXISTS idx_memories_status ON memories(status);
            CREATE INDEX IF NOT EXISTS idx_memories_session_id ON memories(session_id);
            CREATE INDEX IF NOT EXISTS idx_relations_source ON relations(source_id);
            CREATE INDEX IF NOT EXISTS idx_relations_target ON relations(target_id);
            ",
        )
        .map_err(|e| ShabkaError::Storage(format!("failed to create tables: {e}")))?;

        Ok(())
    }

    /// Run a blocking closure against the SQLite connection on the Tokio
    /// blocking thread-pool.  This is the primary way trait methods will
    /// interact with the database.
    #[allow(dead_code)] // used by StorageBackend impl in a subsequent task
    pub(crate) async fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| {
                ShabkaError::Storage(format!("failed to acquire database lock: {e}"))
            })?;
            f(&conn)
        })
        .await
        .map_err(|e| ShabkaError::Storage(format!("task join error: {e}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_creates_tables() {
        let storage = SqliteStorage::open_in_memory().expect("should open in-memory DB");
        assert_eq!(storage.path().to_str().unwrap(), ":memory:");

        // Verify tables exist by querying sqlite_master.
        let conn = storage.conn.lock().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"memories".to_string()));
        assert!(tables.contains(&"embeddings".to_string()));
        assert!(tables.contains(&"relations".to_string()));
        assert!(tables.contains(&"sessions".to_string()));
    }

    #[test]
    fn create_tables_is_idempotent() {
        let storage = SqliteStorage::open_in_memory().expect("should open in-memory DB");
        // Calling create_tables again should not error.
        storage.create_tables().expect("idempotent create_tables");
    }

    #[tokio::test]
    async fn with_conn_runs_on_blocking_pool() {
        let storage = SqliteStorage::open_in_memory().expect("should open in-memory DB");
        let count: i64 = storage
            .with_conn(|conn| {
                let n: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| ShabkaError::Storage(e.to_string()))?;
                Ok(n)
            })
            .await
            .expect("with_conn should succeed");

        // At least the 4 tables we created.
        assert!(count >= 4, "expected at least 4 tables, got {count}");
    }

    #[test]
    fn open_file_based_db() {
        let dir = std::env::temp_dir().join(format!("shabka-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");

        let storage = SqliteStorage::open(&db_path).expect("should open file DB");
        assert_eq!(storage.path(), db_path);

        // Cleanup.
        drop(storage);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
