use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use std::sync::Once;

use crate::error::{Result, ShabkaError};
use crate::model::*;
use crate::storage::StorageBackend;

/// Report from a database integrity check (SQLite only).
#[derive(Debug, Default)]
pub struct IntegrityReport {
    pub total_memories: usize,
    pub total_embeddings: usize,
    pub total_relations: usize,
    pub total_sessions: usize,
    pub orphaned_embeddings: Vec<String>,
    pub broken_relations: Vec<(String, String)>,
    pub missing_embeddings: usize,
    pub sqlite_integrity_ok: bool,
}

/// Current schema version. Bump this when adding migrations.
/// Existing DBs at version 0 get stamped to this on first open.
const SCHEMA_VERSION: i32 = 1;

static EXTENSIONS_REGISTERED: Once = Once::new();

extern "C" {
    fn sqlite3_fuzzy_init(
        db: *mut rusqlite::ffi::sqlite3,
        pz_err_msg: *mut *mut std::ffi::c_char,
        p_api: *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::ffi::c_int;

    fn sqlite3_stats_init(
        db: *mut rusqlite::ffi::sqlite3,
        pz_err_msg: *mut *mut std::ffi::c_char,
        p_api: *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::ffi::c_int;

    fn sqlite3_crypto_init(
        db: *mut rusqlite::ffi::sqlite3,
        pz_err_msg: *mut *mut std::ffi::c_char,
        p_api: *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::ffi::c_int;
}

fn register_extensions() {
    EXTENSIONS_REGISTERED.call_once(|| unsafe {
        // sqlite-vec: vector search
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::ffi::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::ffi::c_int,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
        // sqlean: fuzzy string matching
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::ffi::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::ffi::c_int,
        >(sqlite3_fuzzy_init as *const ())));
        // sqlean: statistical aggregations
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::ffi::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::ffi::c_int,
        >(sqlite3_stats_init as *const ())));
        // sqlean: cryptographic hashing
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::ffi::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::ffi::c_int,
        >(sqlite3_crypto_init as *const ())));
    });
}

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
        register_extensions();
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)
            .map_err(|e| ShabkaError::Storage(format!("failed to open SQLite database: {e}")))?;

        Self::configure_and_init(conn, path)
    }

    /// Open an in-memory SQLite database (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        register_extensions();
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

        // Create vec_memories with dimensions matching existing embeddings.
        // vec0 requires a fixed dimension at creation time, so we detect it
        // from stored embeddings or default to 128 (hash provider).
        Self::ensure_vec_table(&conn)?;

        // Schema versioning: stamp version + metadata table
        Self::check_schema_version(&conn)?;

        Ok(())
    }

    /// Ensure the `vec_memories` virtual table exists with the correct
    /// dimensions.  Detects the most common dimension from existing
    /// embeddings (handles mixed providers), or defaults to 128 (hash).
    /// Only migrates embeddings that match the chosen dimension — stale
    /// embeddings from a previous provider are skipped (fixed by `reembed`).
    fn ensure_vec_table(conn: &Connection) -> Result<()> {
        // Pick the most common dimension across all stored embeddings.
        // A real DB may contain mixed dims (e.g. 128 from hash + 768 from
        // ollama) after switching providers.  We use the mode so the
        // vec_memories table matches the majority of data.
        let dims: i64 = conn
            .query_row(
                "SELECT dimensions FROM embeddings
                 GROUP BY dimensions ORDER BY COUNT(*) DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(128);

        // Always drop and recreate — vec_memories is a derived index,
        // the embeddings table is the source of truth.
        conn.execute_batch("DROP TABLE IF EXISTS vec_memories;")
            .map_err(|e| ShabkaError::Storage(format!("failed to drop vec_memories: {e}")))?;

        conn.execute_batch(&format!(
            "CREATE VIRTUAL TABLE vec_memories USING vec0(
                memory_id TEXT PRIMARY KEY,
                embedding float[{dims}]
            );"
        ))
        .map_err(|e| ShabkaError::Storage(format!("failed to create vec_memories: {e}")))?;

        // Only migrate embeddings that match the chosen dimension.
        // Mismatched ones are stale from a previous provider switch
        // and will be re-embedded via `shabka reembed`.
        let migrated: usize = conn
            .execute(
                "INSERT INTO vec_memories (memory_id, embedding)
                 SELECT memory_id, vector FROM embeddings
                 WHERE dimensions = ?1",
                [dims],
            )
            .map_err(|e| ShabkaError::Storage(format!("failed to populate vec_memories: {e}")))?;

        if migrated > 0 {
            // Check if any embeddings were skipped due to dimension mismatch.
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))
                .unwrap_or(0);
            let skipped = total - migrated as i64;

            if skipped > 0 {
                tracing::warn!(
                    migrated,
                    skipped,
                    dims,
                    "skipped embeddings with mismatched dimensions — run `shabka reembed` to fix"
                );
            } else {
                tracing::info!(migrated, dims, "populated vec_memories from embeddings");
            }
        }

        Ok(())
    }

    // ── schema versioning ──────────────────────────────────────────────

    /// Check and update the schema version using `PRAGMA user_version`.
    ///
    /// - `user_version == 0` → fresh DB or pre-versioning; stamp to `SCHEMA_VERSION`
    /// - `user_version < SCHEMA_VERSION` → run sequential migrations, then update
    /// - `user_version > SCHEMA_VERSION` → log warning (newer binary wrote this DB)
    ///
    /// Also creates the `metadata` table and records `last_writer_version`.
    fn check_schema_version(conn: &Connection) -> Result<()> {
        // Create metadata table (idempotent)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .map_err(|e| ShabkaError::Storage(format!("failed to create metadata table: {e}")))?;

        let current: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| ShabkaError::Storage(format!("failed to read user_version: {e}")))?;

        if current < SCHEMA_VERSION {
            if current > 0 {
                Self::run_migrations(conn, current)?;
            }
            conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
                .map_err(|e| ShabkaError::Storage(format!("failed to set user_version: {e}")))?;
        } else if current > SCHEMA_VERSION {
            tracing::warn!(
                db_version = current,
                binary_version = SCHEMA_VERSION,
                "database was written by a newer version of Shabka — consider upgrading"
            );
        }

        // Always record which binary version last wrote to this DB
        let writer_version = env!("CARGO_PKG_VERSION");
        conn.execute(
            "INSERT INTO metadata (key, value) VALUES ('last_writer_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![writer_version],
        )
        .map_err(|e| ShabkaError::Storage(format!("failed to update metadata: {e}")))?;

        Ok(())
    }

    /// Run sequential migrations from `from_version` up to `SCHEMA_VERSION`.
    /// Each version bump gets its own match arm.
    #[allow(clippy::needless_range_loop)]
    fn run_migrations(_conn: &Connection, from_version: i32) -> Result<()> {
        let mut version = from_version;
        while version < SCHEMA_VERSION {
            // Future migrations go here:
            // if version == 1 { conn.execute_batch("ALTER TABLE ...")?; }
            version += 1;
        }
        Ok(())
    }

    /// Return `(schema_version, last_writer_version)` for status display.
    pub fn schema_info(&self) -> Result<(i32, Option<String>)> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ShabkaError::Storage(format!("failed to acquire database lock: {e}")))?;

        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| ShabkaError::Storage(format!("failed to read user_version: {e}")))?;

        let writer: Option<String> = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'last_writer_version'",
                [],
                |row| row.get(0),
            )
            .ok();

        Ok((version, writer))
    }

    /// Run a full integrity check on the SQLite database.
    ///
    /// Returns an [`IntegrityReport`] with counts, orphaned embeddings,
    /// broken relations, memories missing embeddings, and the result of
    /// `PRAGMA integrity_check`.
    pub fn integrity_check(&self) -> Result<IntegrityReport> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ShabkaError::Storage(format!("failed to acquire database lock: {e}")))?;

        // Counts
        let total_memories = conn
            .query_row("SELECT COUNT(*) FROM memories", [], |r| r.get::<_, i64>(0))
            .map_err(|e| ShabkaError::Storage(format!("count memories: {e}")))?
            as usize;
        let total_embeddings = conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(|e| ShabkaError::Storage(format!("count embeddings: {e}")))?
            as usize;
        let total_relations = conn
            .query_row("SELECT COUNT(*) FROM relations", [], |r| r.get::<_, i64>(0))
            .map_err(|e| ShabkaError::Storage(format!("count relations: {e}")))?
            as usize;
        let total_sessions = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get::<_, i64>(0))
            .map_err(|e| ShabkaError::Storage(format!("count sessions: {e}")))?
            as usize;

        // Orphaned embeddings: embedding rows whose memory_id has no matching memory
        let mut stmt = conn
            .prepare(
                "SELECT e.memory_id FROM embeddings e \
                 LEFT JOIN memories m ON m.id = e.memory_id \
                 WHERE m.id IS NULL",
            )
            .map_err(|e| ShabkaError::Storage(format!("prepare orphan query: {e}")))?;
        let orphaned_embeddings: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| ShabkaError::Storage(format!("orphan query: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        // Broken relations: relation rows where source or target memory is missing
        let mut stmt = conn
            .prepare(
                "SELECT r.source_id, r.target_id FROM relations r \
                 LEFT JOIN memories m1 ON m1.id = r.source_id \
                 LEFT JOIN memories m2 ON m2.id = r.target_id \
                 WHERE m1.id IS NULL OR m2.id IS NULL",
            )
            .map_err(|e| ShabkaError::Storage(format!("prepare broken-rel query: {e}")))?;
        let broken_relations: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| ShabkaError::Storage(format!("broken-rel query: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        // Memories that have no embedding row
        let missing_embeddings = conn
            .query_row(
                "SELECT COUNT(*) FROM memories m \
                 LEFT JOIN embeddings e ON e.memory_id = m.id \
                 WHERE e.memory_id IS NULL",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| ShabkaError::Storage(format!("missing embeddings query: {e}")))?
            as usize;

        // SQLite built-in integrity check
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|e| ShabkaError::Storage(format!("integrity_check pragma: {e}")))?;

        Ok(IntegrityReport {
            total_memories,
            total_embeddings,
            total_relations,
            total_sessions,
            orphaned_embeddings,
            broken_relations,
            missing_embeddings,
            sqlite_integrity_ok: integrity == "ok",
        })
    }

    /// Remove orphaned embeddings and broken relations identified by a
    /// previous [`integrity_check`](Self::integrity_check) run.
    ///
    /// Returns `(orphaned_embeddings_removed, broken_relations_removed)`.
    pub fn repair(&self, report: &IntegrityReport) -> Result<(usize, usize)> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ShabkaError::Storage(format!("failed to acquire database lock: {e}")))?;

        let mut orphans_removed = 0;
        for memory_id in &report.orphaned_embeddings {
            orphans_removed += conn
                .execute(
                    "DELETE FROM embeddings WHERE memory_id = ?1",
                    params![memory_id],
                )
                .map_err(|e| ShabkaError::Storage(format!("delete orphan embedding: {e}")))?;
        }

        let mut relations_removed = 0;
        for (source_id, target_id) in &report.broken_relations {
            relations_removed += conn
                .execute(
                    "DELETE FROM relations WHERE source_id = ?1 AND target_id = ?2",
                    params![source_id, target_id],
                )
                .map_err(|e| ShabkaError::Storage(format!("delete broken relation: {e}")))?;
        }

        Ok((orphans_removed, relations_removed))
    }

    /// Run a blocking closure against the SQLite connection on the Tokio
    /// blocking thread-pool.  This is the primary way trait methods will
    /// interact with the database.
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

// ── Helper functions ────────────────────────────────────────────────────

/// Convert a SQLite row (from SELECT * on memories) into a `Memory` struct.
fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
    let id_str: String = row.get("id")?;
    let kind_str: String = row.get("kind")?;
    let status_str: String = row.get("status")?;
    let privacy_str: String = row.get("privacy")?;
    let verification_str: String = row.get("verification")?;
    let source_json: String = row.get("source")?;
    let scope_json: String = row.get("scope")?;
    let tags_json: String = row.get("tags")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;
    let accessed_at_str: String = row.get("accessed_at")?;
    let project_id: Option<String> = row.get("project_id")?;
    let session_id_str: Option<String> = row.get("session_id")?;

    // Simple enums: stored as plain strings like "observation" — wrap in quotes for serde
    let kind: MemoryKind = serde_json::from_str(&format!("\"{kind_str}\"")).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let status: MemoryStatus = serde_json::from_str(&format!("\"{status_str}\"")).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let privacy: MemoryPrivacy =
        serde_json::from_str(&format!("\"{privacy_str}\"")).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let verification: VerificationStatus = serde_json::from_str(&format!("\"{verification_str}\""))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, Box::new(e))
        })?;

    // Tagged enums: stored as JSON objects
    let source: MemorySource = serde_json::from_str(&source_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let scope: MemoryScope = serde_json::from_str(&scope_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
    })?;

    // Tags: JSON array
    let tags: Vec<String> = serde_json::from_str(&tags_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;

    // UUID fields
    let id = Uuid::parse_str(&id_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let session_id = session_id_str
        .map(|s| Uuid::parse_str(&s))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(13, rusqlite::types::Type::Text, Box::new(e))
        })?;

    // Dates: RFC 3339 strings
    let created_at: DateTime<Utc> = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(15, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let updated_at: DateTime<Utc> = DateTime::parse_from_rfc3339(&updated_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(16, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let accessed_at: DateTime<Utc> = DateTime::parse_from_rfc3339(&accessed_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(17, rusqlite::types::Type::Text, Box::new(e))
        })?;

    // importance stored as f64 in SQLite, coerce to f32
    let importance: f64 = row.get("importance")?;

    Ok(Memory {
        id,
        kind,
        title: row.get("title")?,
        content: row.get("content")?,
        summary: row.get("summary")?,
        tags,
        source,
        scope,
        importance: importance as f32,
        status,
        privacy,
        verification,
        project_id,
        session_id,
        created_by: row.get("created_by")?,
        created_at,
        updated_at,
        accessed_at,
    })
}

/// Serialize a simple serde enum to its snake_case string value (no quotes).
fn kind_to_str(kind: &MemoryKind) -> String {
    serde_json::to_string(kind)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

fn status_to_str(status: &MemoryStatus) -> String {
    serde_json::to_string(status)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

fn privacy_to_str(privacy: &MemoryPrivacy) -> String {
    serde_json::to_string(privacy)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

fn verification_to_str(verification: &VerificationStatus) -> String {
    serde_json::to_string(verification)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

// ── StorageBackend impl ─────────────────────────────────────────────────

impl StorageBackend for SqliteStorage {
    // -- Memory CRUD --

    async fn save_memory(&self, memory: &Memory, embedding: Option<&[f32]>) -> Result<()> {
        let memory = memory.clone();
        let embedding = embedding.map(|e| e.to_vec());

        self.with_conn(move |conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| ShabkaError::Storage(format!("failed to begin transaction: {e}")))?;

            tx.execute(
                "INSERT OR REPLACE INTO memories (id, kind, title, content, summary, tags, source, scope,
                    importance, status, privacy, verification, project_id, session_id,
                    created_by, created_at, updated_at, accessed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                params![
                    memory.id.to_string(),
                    kind_to_str(&memory.kind),
                    memory.title,
                    memory.content,
                    memory.summary,
                    serde_json::to_string(&memory.tags).unwrap_or_else(|_| "[]".to_string()),
                    serde_json::to_string(&memory.source).unwrap_or_else(|_| r#"{"type":"manual"}"#.to_string()),
                    serde_json::to_string(&memory.scope).unwrap_or_else(|_| r#"{"type":"global"}"#.to_string()),
                    memory.importance as f64,
                    status_to_str(&memory.status),
                    privacy_to_str(&memory.privacy),
                    verification_to_str(&memory.verification),
                    memory.project_id,
                    memory.session_id.map(|id| id.to_string()),
                    memory.created_by,
                    memory.created_at.to_rfc3339(),
                    memory.updated_at.to_rfc3339(),
                    memory.accessed_at.to_rfc3339(),
                ],
            )
            .map_err(|e| ShabkaError::Storage(format!("failed to insert memory: {e}")))?;

            if let Some(emb) = embedding {
                let dimensions = emb.len() as i64;
                // Serialize f32 vec to little-endian bytes
                let blob: Vec<u8> = emb.iter().flat_map(|f| f.to_le_bytes()).collect();
                tx.execute(
                    "INSERT OR REPLACE INTO embeddings (memory_id, vector, dimensions) VALUES (?1, ?2, ?3)",
                    params![memory.id.to_string(), blob, dimensions],
                )
                .map_err(|e| ShabkaError::Storage(format!("failed to insert embedding: {e}")))?;

                // Best-effort upsert into vec_memories for sqlite-vec search.
                // vec0 doesn't support OR REPLACE, so delete-then-insert.
                // This may fail if dimensions changed (e.g. during reembed) —
                // that's OK, vec_memories is rebuilt on next startup.
                let _ = tx.execute(
                    "DELETE FROM vec_memories WHERE memory_id = ?1",
                    params![memory.id.to_string()],
                );
                if let Err(e) = tx.execute(
                    "INSERT INTO vec_memories (memory_id, embedding) VALUES (?1, ?2)",
                    params![memory.id.to_string(), blob],
                ) {
                    tracing::debug!("vec_memories insert skipped (will rebuild on restart): {e}");
                }
            }

            tx.commit()
                .map_err(|e| ShabkaError::Storage(format!("failed to commit transaction: {e}")))?;

            Ok(())
        })
        .await
    }

    async fn get_memory(&self, id: Uuid) -> Result<Memory> {
        let id_str = id.to_string();
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT * FROM memories WHERE id = ?1",
                params![id_str],
                row_to_memory,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    ShabkaError::NotFound(format!("memory {id} not found"))
                }
                _ => ShabkaError::Storage(format!("failed to get memory: {e}")),
            })
        })
        .await
    }

    async fn get_memories(&self, ids: &[Uuid]) -> Result<Vec<Memory>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let id_strings: Vec<String> = ids.iter().map(|id| id.to_string()).collect();

        self.with_conn(move |conn| {
            // Build dynamic IN clause: WHERE id IN (?1, ?2, ...)
            let placeholders: Vec<String> =
                (1..=id_strings.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT * FROM memories WHERE id IN ({})",
                placeholders.join(", ")
            );

            let params: Vec<Box<dyn rusqlite::types::ToSql>> = id_strings
                .iter()
                .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::types::ToSql>)
                .collect();

            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| ShabkaError::Storage(format!("failed to prepare query: {e}")))?;

            let rows = stmt
                .query_map(param_refs.as_slice(), row_to_memory)
                .map_err(|e| ShabkaError::Storage(format!("failed to query memories: {e}")))?;

            let mut memories = Vec::new();
            for row in rows {
                memories.push(
                    row.map_err(|e| ShabkaError::Storage(format!("failed to read row: {e}")))?,
                );
            }

            Ok(memories)
        })
        .await
    }

    async fn update_memory(&self, id: Uuid, input: &UpdateMemoryInput) -> Result<Memory> {
        let id_str = id.to_string();
        let input = input.clone();

        self.with_conn(move |conn| {
            // Build dynamic SET clause from non-None fields
            let mut set_clauses: Vec<String> = Vec::new();
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1usize;

            if let Some(ref title) = input.title {
                set_clauses.push(format!("title = ?{idx}"));
                param_values.push(Box::new(title.clone()));
                idx += 1;
            }
            if let Some(ref content) = input.content {
                set_clauses.push(format!("content = ?{idx}"));
                param_values.push(Box::new(content.clone()));
                idx += 1;
            }
            if let Some(ref tags) = input.tags {
                set_clauses.push(format!("tags = ?{idx}"));
                param_values.push(Box::new(
                    serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string()),
                ));
                idx += 1;
            }
            if let Some(importance) = input.importance {
                set_clauses.push(format!("importance = ?{idx}"));
                param_values.push(Box::new(importance as f64));
                idx += 1;
            }
            if let Some(ref status) = input.status {
                set_clauses.push(format!("status = ?{idx}"));
                param_values.push(Box::new(status_to_str(status)));
                idx += 1;
            }
            if let Some(ref kind) = input.kind {
                set_clauses.push(format!("kind = ?{idx}"));
                param_values.push(Box::new(kind_to_str(kind)));
                idx += 1;
            }
            if let Some(ref privacy) = input.privacy {
                set_clauses.push(format!("privacy = ?{idx}"));
                param_values.push(Box::new(privacy_to_str(privacy)));
                idx += 1;
            }
            if let Some(ref verification) = input.verification {
                set_clauses.push(format!("verification = ?{idx}"));
                param_values.push(Box::new(verification_to_str(verification)));
                idx += 1;
            }

            // Always update updated_at
            let now = Utc::now().to_rfc3339();
            set_clauses.push(format!("updated_at = ?{idx}"));
            param_values.push(Box::new(now));
            idx += 1;

            // WHERE id = ?N
            let sql = format!(
                "UPDATE memories SET {} WHERE id = ?{idx}",
                set_clauses.join(", ")
            );
            param_values.push(Box::new(id_str.clone()));

            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p.as_ref()).collect();

            let rows_affected = conn
                .execute(&sql, param_refs.as_slice())
                .map_err(|e| ShabkaError::Storage(format!("failed to update memory: {e}")))?;

            if rows_affected == 0 {
                return Err(ShabkaError::NotFound(format!("memory {id} not found")));
            }

            // Return the updated row
            conn.query_row(
                "SELECT * FROM memories WHERE id = ?1",
                params![id_str],
                row_to_memory,
            )
            .map_err(|e| ShabkaError::Storage(format!("failed to read updated memory: {e}")))
        })
        .await
    }

    async fn delete_memory(&self, id: Uuid) -> Result<()> {
        let id_str = id.to_string();
        self.with_conn(move |conn| {
            // Delete from vec_memories first — vec0 virtual tables don't support
            // ON DELETE CASCADE, so we must clean up explicitly.
            conn.execute(
                "DELETE FROM vec_memories WHERE memory_id = ?1",
                params![id_str],
            )
            .map_err(|e| ShabkaError::Storage(format!("failed to delete vec embedding: {e}")))?;

            let rows_affected = conn
                .execute("DELETE FROM memories WHERE id = ?1", params![id_str])
                .map_err(|e| ShabkaError::Storage(format!("failed to delete memory: {e}")))?;

            if rows_affected == 0 {
                return Err(ShabkaError::NotFound(format!("memory {id} not found")));
            }

            Ok(())
        })
        .await
    }

    // -- Search --

    async fn vector_search(&self, embedding: &[f32], limit: usize) -> Result<Vec<(Memory, f32)>> {
        let query_vec = embedding.to_vec();

        self.with_conn(move |conn| {
            // Guard: detect the dimension vec_memories was built with by
            // probing the first stored embedding's byte-length (4 bytes per f32).
            // If vec_memories is empty or dimensions don't match the query,
            // return empty — the user needs to `shabka reembed`.
            let stored_dims: i64 = conn
                .query_row(
                    "SELECT length(embedding) / 4 FROM vec_memories LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            if stored_dims == 0 {
                return Ok(Vec::new());
            }

            if stored_dims != query_vec.len() as i64 {
                tracing::warn!(
                    stored = stored_dims,
                    query = query_vec.len(),
                    "vector dimension mismatch — run `shabka reembed` to fix"
                );
                return Ok(Vec::new());
            }

            // Serialize query vector to little-endian bytes for sqlite-vec
            let query_blob: Vec<u8> = query_vec.iter().flat_map(|f| f.to_le_bytes()).collect();

            // KNN search via vec_memories, JOIN with memories for full records.
            // Exclude Pending memories — they require explicit approval first.
            let sql = "
                SELECT m.*, v.distance
                FROM vec_memories AS v
                JOIN memories AS m ON m.id = v.memory_id
                WHERE v.embedding MATCH ?1
                  AND v.k = ?2
                  AND m.status != 'pending'
                ORDER BY v.distance
            ";

            let mut stmt = conn
                .prepare(sql)
                .map_err(|e| ShabkaError::Storage(format!("failed to prepare vec search: {e}")))?;

            let rows = stmt
                .query_map(params![query_blob, limit as i64], |row| {
                    let mem = row_to_memory(row)?;
                    let distance: f64 = row.get("distance")?;
                    Ok((mem, distance))
                })
                .map_err(|e| ShabkaError::Storage(format!("failed to execute vec search: {e}")))?;

            let mut results = Vec::new();
            for row in rows {
                let (mem, distance) = row.map_err(|e| {
                    ShabkaError::Storage(format!("failed to read vec search row: {e}"))
                })?;
                // Convert L2 distance to similarity score: 1.0 for identical, ~0 for distant
                let score = 1.0 / (1.0 + distance as f32);
                results.push((mem, score));
            }

            Ok(results)
        })
        .await
    }

    // -- Timeline --

    async fn timeline(&self, query: &TimelineQuery) -> Result<Vec<TimelineEntry>> {
        let query = query.clone();

        self.with_conn(move |conn| {
            // Build WHERE clause dynamically from non-None fields
            let mut conditions: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(ref id) = query.memory_id {
                conditions.push(format!("m.id = ?{idx}"));
                params.push(Box::new(id.to_string()));
                idx += 1;
            }
            if let Some(ref start) = query.start {
                conditions.push(format!("m.created_at >= ?{idx}"));
                params.push(Box::new(start.to_rfc3339()));
                idx += 1;
            }
            if let Some(ref end) = query.end {
                conditions.push(format!("m.created_at <= ?{idx}"));
                params.push(Box::new(end.to_rfc3339()));
                idx += 1;
            }
            if let Some(ref session_id) = query.session_id {
                conditions.push(format!("m.session_id = ?{idx}"));
                params.push(Box::new(session_id.to_string()));
                idx += 1;
            }
            if let Some(ref project_id) = query.project_id {
                conditions.push(format!("m.project_id = ?{idx}"));
                params.push(Box::new(project_id.clone()));
                idx += 1;
            }
            if let Some(ref kind) = query.kind {
                conditions.push(format!("m.kind = ?{idx}"));
                params.push(Box::new(kind_to_str(kind)));
                idx += 1;
            }
            if let Some(ref status) = query.status {
                conditions.push(format!("m.status = ?{idx}"));
                params.push(Box::new(status_to_str(status)));
                idx += 1;
            } else {
                // Exclude Pending memories by default
                conditions.push("m.status != 'pending'".to_string());
            }
            if let Some(ref privacy) = query.privacy {
                conditions.push(format!("m.privacy = ?{idx}"));
                params.push(Box::new(privacy_to_str(privacy)));
                idx += 1;
            }
            if let Some(ref created_by) = query.created_by {
                conditions.push(format!("m.created_by = ?{idx}"));
                params.push(Box::new(created_by.clone()));
                idx += 1;
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let sql = format!(
                "SELECT m.*,
                    (SELECT COUNT(*) FROM relations r WHERE r.source_id = m.id) as related_count
                 FROM memories m
                 {where_clause}
                 ORDER BY m.created_at DESC
                 LIMIT ?{idx} OFFSET ?{}",
                idx + 1
            );
            params.push(Box::new(query.limit as i64));
            params.push(Box::new(query.offset as i64));

            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| {
                ShabkaError::Storage(format!("failed to prepare timeline query: {e}"))
            })?;

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    let memory = row_to_memory(row)?;
                    let related_count: i64 = row.get("related_count")?;
                    Ok(TimelineEntry::from((&memory, related_count as usize)))
                })
                .map_err(|e| ShabkaError::Storage(format!("failed to query timeline: {e}")))?;

            let mut entries = Vec::new();
            for row in rows {
                entries.push(row.map_err(|e| {
                    ShabkaError::Storage(format!("failed to read timeline row: {e}"))
                })?);
            }

            Ok(entries)
        })
        .await
    }

    // -- Graph --

    async fn add_relation(&self, relation: &MemoryRelation) -> Result<()> {
        let relation = relation.clone();
        self.with_conn(move |conn| {
            let rel_type = serde_json::to_string(&relation.relation_type)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();

            conn.execute(
                "INSERT OR REPLACE INTO relations (source_id, target_id, relation_type, strength)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    relation.source_id.to_string(),
                    relation.target_id.to_string(),
                    rel_type,
                    relation.strength,
                ],
            )
            .map_err(|e| ShabkaError::Storage(format!("failed to add relation: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn get_relations(&self, memory_id: Uuid) -> Result<Vec<MemoryRelation>> {
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT source_id, target_id, relation_type, strength
                     FROM relations
                     WHERE source_id = ?1 OR target_id = ?1",
                )
                .map_err(|e| ShabkaError::Storage(format!("failed to prepare query: {e}")))?;

            let rows = stmt
                .query_map(rusqlite::params![memory_id.to_string()], |row| {
                    let source_str: String = row.get(0)?;
                    let target_str: String = row.get(1)?;
                    let rel_type_str: String = row.get(2)?;
                    let strength: f32 = row.get(3)?;
                    Ok((source_str, target_str, rel_type_str, strength))
                })
                .map_err(|e| ShabkaError::Storage(format!("failed to query relations: {e}")))?;

            let mut relations = Vec::new();
            for row in rows {
                let (source_str, target_str, rel_type_str, strength) = row.map_err(|e| {
                    ShabkaError::Storage(format!("failed to read relation row: {e}"))
                })?;
                relations.push(MemoryRelation {
                    source_id: Uuid::parse_str(&source_str).unwrap_or_default(),
                    target_id: Uuid::parse_str(&target_str).unwrap_or_default(),
                    relation_type: serde_json::from_str(&format!("\"{rel_type_str}\""))
                        .unwrap_or(RelationType::Related),
                    strength,
                });
            }
            Ok(relations)
        })
        .await
    }

    async fn count_relations(&self, memory_ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
        if memory_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = memory_ids.iter().map(|id| id.to_string()).collect();
        self.with_conn(move |conn| {
            let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT source_id, COUNT(*) as cnt FROM relations
                 WHERE source_id IN ({})
                 GROUP BY source_id",
                placeholders.join(", ")
            );
            let params: Vec<&dyn rusqlite::types::ToSql> = ids
                .iter()
                .map(|s| s as &dyn rusqlite::types::ToSql)
                .collect();

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| ShabkaError::Storage(format!("failed to prepare query: {e}")))?;
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    let id_str: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((id_str, count as usize))
                })
                .map_err(|e| ShabkaError::Storage(format!("failed to count relations: {e}")))?;

            let mut counts = Vec::new();
            for row in rows {
                let (id_str, count) = row
                    .map_err(|e| ShabkaError::Storage(format!("failed to read count row: {e}")))?;
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    counts.push((id, count));
                }
            }
            Ok(counts)
        })
        .await
    }

    async fn count_contradictions(&self, memory_ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
        if memory_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = memory_ids.iter().map(|id| id.to_string()).collect();
        self.with_conn(move |conn| {
            let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT source_id, COUNT(*) as cnt FROM relations
                 WHERE source_id IN ({}) AND relation_type = 'contradicts'
                 GROUP BY source_id",
                placeholders.join(", ")
            );
            let params: Vec<&dyn rusqlite::types::ToSql> = ids
                .iter()
                .map(|s| s as &dyn rusqlite::types::ToSql)
                .collect();

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| ShabkaError::Storage(format!("failed to prepare query: {e}")))?;
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    let id_str: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((id_str, count as usize))
                })
                .map_err(|e| {
                    ShabkaError::Storage(format!("failed to count contradictions: {e}"))
                })?;

            let mut counts = Vec::new();
            for row in rows {
                let (id_str, count) = row
                    .map_err(|e| ShabkaError::Storage(format!("failed to read count row: {e}")))?;
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    counts.push((id, count));
                }
            }
            Ok(counts)
        })
        .await
    }

    // -- Session --

    async fn save_session(&self, session: &Session) -> Result<()> {
        let session = session.clone();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO sessions (id, project_id, started_at, ended_at, summary, memory_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    session.id.to_string(),
                    session.project_id,
                    session.started_at.to_rfc3339(),
                    session.ended_at.map(|dt| dt.to_rfc3339()),
                    session.summary,
                    session.memory_count as i64,
                ],
            )
            .map_err(|e| ShabkaError::Storage(format!("failed to save session: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn get_session(&self, id: Uuid) -> Result<Session> {
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT * FROM sessions WHERE id = ?1",
                rusqlite::params![id.to_string()],
                |row| {
                    let id_str: String = row.get("id")?;
                    let started_at_str: String = row.get("started_at")?;
                    let ended_at_str: Option<String> = row.get("ended_at")?;
                    let memory_count: i64 = row.get("memory_count")?;
                    Ok(Session {
                        id: uuid::Uuid::parse_str(&id_str).unwrap_or_default(),
                        project_id: row.get("project_id")?,
                        started_at: chrono::DateTime::parse_from_rfc3339(&started_at_str)
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                            .unwrap_or_default(),
                        ended_at: ended_at_str.and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s)
                                .map(|dt| dt.with_timezone(&chrono::Utc))
                                .ok()
                        }),
                        summary: row.get("summary")?,
                        memory_count: memory_count as usize,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    ShabkaError::NotFound(format!("session {id}"))
                }
                _ => ShabkaError::Storage(format!("failed to get session: {e}")),
            })
        })
        .await
    }
}

// ── Additional query methods (not on the trait) ─────────────────────────

impl SqliteStorage {
    /// Return the total count of timeline entries matching the given filters,
    /// ignoring `limit` and `offset`. Used for pagination metadata.
    pub async fn timeline_count(&self, query: &TimelineQuery) -> Result<usize> {
        let query = query.clone();
        self.with_conn(move |conn| {
            let mut conditions: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(ref id) = query.memory_id {
                conditions.push(format!("m.id = ?{idx}"));
                params.push(Box::new(id.to_string()));
                idx += 1;
            }
            if let Some(ref start) = query.start {
                conditions.push(format!("m.created_at >= ?{idx}"));
                params.push(Box::new(start.to_rfc3339()));
                idx += 1;
            }
            if let Some(ref end) = query.end {
                conditions.push(format!("m.created_at <= ?{idx}"));
                params.push(Box::new(end.to_rfc3339()));
                idx += 1;
            }
            if let Some(ref session_id) = query.session_id {
                conditions.push(format!("m.session_id = ?{idx}"));
                params.push(Box::new(session_id.to_string()));
                idx += 1;
            }
            if let Some(ref project_id) = query.project_id {
                conditions.push(format!("m.project_id = ?{idx}"));
                params.push(Box::new(project_id.clone()));
                idx += 1;
            }
            if let Some(ref kind) = query.kind {
                conditions.push(format!("m.kind = ?{idx}"));
                params.push(Box::new(kind_to_str(kind)));
                idx += 1;
            }
            if let Some(ref status) = query.status {
                conditions.push(format!("m.status = ?{idx}"));
                params.push(Box::new(status_to_str(status)));
                idx += 1;
            } else {
                // Exclude Pending memories by default
                conditions.push("m.status != 'pending'".to_string());
            }
            if let Some(ref privacy) = query.privacy {
                conditions.push(format!("m.privacy = ?{idx}"));
                params.push(Box::new(privacy_to_str(privacy)));
                idx += 1;
            }
            if let Some(ref created_by) = query.created_by {
                conditions.push(format!("m.created_by = ?{idx}"));
                params.push(Box::new(created_by.clone()));
                let _ = idx; // suppress unused warning
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let sql = format!("SELECT COUNT(*) FROM memories m {where_clause}");
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let count: i64 = conn
                .query_row(&sql, param_refs.as_slice(), |row| row.get(0))
                .map_err(|e| {
                    ShabkaError::Storage(format!("failed to count timeline entries: {e}"))
                })?;

            Ok(count as usize)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        MemoryKind, MemoryPrivacy, MemoryRelation, MemoryScope, MemorySource, MemoryStatus,
        RelationType, UpdateMemoryInput, VerificationStatus,
    };
    use crate::storage::StorageBackend;

    fn test_memory() -> Memory {
        Memory {
            id: Uuid::now_v7(),
            kind: MemoryKind::Observation,
            title: "Test memory".to_string(),
            content: "Some content".to_string(),
            summary: "A summary".to_string(),
            tags: vec!["test".to_string()],
            source: MemorySource::Manual,
            scope: MemoryScope::Global,
            importance: 0.7,
            status: MemoryStatus::Active,
            privacy: MemoryPrivacy::Private,
            verification: VerificationStatus::Unverified,
            project_id: None,
            session_id: None,
            created_by: "tester".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            accessed_at: Utc::now(),
        }
    }

    #[test]
    fn sqlite_vec_extension_loaded() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let conn = storage.conn.lock().unwrap();
        let version: String = conn
            .query_row("SELECT vec_version()", [], |row| row.get(0))
            .unwrap();
        assert!(!version.is_empty(), "sqlite-vec should report a version");
    }

    #[test]
    fn sqlean_fuzzy_loaded() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let conn = storage.conn.lock().unwrap();
        let score: f64 = conn
            .query_row("SELECT fuzzy_damlev('kitten', 'sitting')", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(score > 0.0, "fuzzy_damlev should return edit distance");
    }

    #[test]
    fn sqlean_stats_loaded() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let conn = storage.conn.lock().unwrap();
        let median: f64 = conn
            .query_row(
                "SELECT stats_median(value) FROM (SELECT 1 AS value UNION SELECT 2 UNION SELECT 3)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!((median - 2.0).abs() < 0.01, "median of 1,2,3 should be 2");
    }

    #[test]
    fn sqlean_crypto_loaded() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let conn = storage.conn.lock().unwrap();
        let hash: String = conn
            .query_row("SELECT hex(crypto_sha256('hello'))", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            hash.len(),
            64,
            "SHA-256 should produce 32 bytes (64 hex chars)"
        );
    }

    #[test]
    fn vec_memories_table_created() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let conn = storage.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='vec_memories'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "vec_memories virtual table should exist");
    }

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
        assert!(tables.contains(&"vec_memories".to_string()));
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

    // ── CRUD tests ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_save_and_get_memory() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        let id = mem.id;

        storage.save_memory(&mem, None).await.unwrap();
        let got = storage.get_memory(id).await.unwrap();

        assert_eq!(got.id, mem.id);
        assert_eq!(got.title, "Test memory");
        assert_eq!(got.content, "Some content");
        assert_eq!(got.summary, "A summary");
        assert_eq!(got.tags, vec!["test".to_string()]);
        assert_eq!(got.importance, 0.7_f32);
        assert_eq!(got.created_by, "tester");
        assert!(matches!(got.kind, MemoryKind::Observation));
        assert!(matches!(got.status, MemoryStatus::Active));
        assert!(matches!(got.privacy, MemoryPrivacy::Private));
        assert!(matches!(got.verification, VerificationStatus::Unverified));
        assert!(matches!(got.source, MemorySource::Manual));
        assert!(matches!(got.scope, MemoryScope::Global));
        assert!(got.project_id.is_none());
        assert!(got.session_id.is_none());
    }

    #[tokio::test]
    async fn test_get_memory_not_found() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let result = storage.get_memory(Uuid::now_v7()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ShabkaError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_get_memories_batch() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem1 = test_memory();
        let mem2 = {
            let mut m = test_memory();
            m.title = "Second memory".to_string();
            m
        };
        let id1 = mem1.id;
        let id2 = mem2.id;

        storage.save_memory(&mem1, None).await.unwrap();
        storage.save_memory(&mem2, None).await.unwrap();

        let batch = storage.get_memories(&[id1, id2]).await.unwrap();
        assert_eq!(batch.len(), 2);

        let titles: Vec<&str> = batch.iter().map(|m| m.title.as_str()).collect();
        assert!(titles.contains(&"Test memory"));
        assert!(titles.contains(&"Second memory"));

        // Empty input returns empty vec
        let empty = storage.get_memories(&[]).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn test_update_memory() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        let id = mem.id;

        storage.save_memory(&mem, None).await.unwrap();

        let input = UpdateMemoryInput {
            title: Some("Updated title".to_string()),
            importance: Some(0.9),
            ..Default::default()
        };

        let updated = storage.update_memory(id, &input).await.unwrap();
        assert_eq!(updated.title, "Updated title");
        assert!((updated.importance - 0.9).abs() < f32::EPSILON);

        // Unchanged fields should be preserved
        assert_eq!(updated.content, "Some content");
        assert_eq!(updated.tags, vec!["test".to_string()]);
        assert_eq!(updated.created_by, "tester");
        assert!(matches!(updated.kind, MemoryKind::Observation));
        assert!(matches!(updated.status, MemoryStatus::Active));

        // updated_at should be newer
        assert!(updated.updated_at >= mem.updated_at);
    }

    #[tokio::test]
    async fn test_delete_memory() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        let id = mem.id;

        storage.save_memory(&mem, None).await.unwrap();

        // Verify it exists
        storage.get_memory(id).await.unwrap();

        // Delete it
        storage.delete_memory(id).await.unwrap();

        // Verify it's gone
        let result = storage.get_memory(id).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ShabkaError::NotFound(_)));

        // Double-delete should also return NotFound
        let result = storage.delete_memory(id).await;
        assert!(matches!(result.unwrap_err(), ShabkaError::NotFound(_)));
    }

    // ── Vector search tests ────────────────────────────────────────────

    #[tokio::test]
    async fn test_vector_search() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        // Use 128-dimensional vectors to match vec_memories float[128] definition
        let mut emb1 = vec![0.0_f32; 128];
        emb1[0] = 1.0;

        let mut emb2 = vec![0.0_f32; 128];
        emb2[0] = 0.9;
        emb2[1] = 0.1;

        let mut emb3 = vec![0.0_f32; 128];
        emb3[2] = 1.0;

        let mut m1 = test_memory();
        m1.title = "Rust patterns".to_string();

        let mut m2 = test_memory();
        m2.title = "Rust lifetimes".to_string();

        let mut m3 = test_memory();
        m3.title = "Python basics".to_string();

        storage.save_memory(&m1, Some(&emb1)).await.unwrap();
        storage.save_memory(&m2, Some(&emb2)).await.unwrap();
        storage.save_memory(&m3, Some(&emb3)).await.unwrap();

        let mut query = vec![0.0_f32; 128];
        query[0] = 1.0;
        let results = storage.vector_search(&query, 2).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0.title, "Rust patterns");
        assert_eq!(results[1].0.title, "Rust lifetimes");
        // L2-based similarity: 1/(1+d). Exact match → 1.0, close match → high score.
        assert!(
            results[0].1 > results[1].1,
            "first result should score higher than second"
        );
        assert!(results[0].1 > 0.9, "exact match should have high score");
        assert!(results[1].1 > 0.5, "close match should have decent score");
    }

    #[tokio::test]
    async fn test_vector_search_no_embeddings() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        storage.save_memory(&mem, None).await.unwrap();

        // Use 128-dimensional query to match vec_memories float[128] definition
        let mut query = vec![0.0_f32; 128];
        query[0] = 1.0;
        let results = storage.vector_search(&query, 10).await.unwrap();
        assert!(results.is_empty());
    }

    // ── Timeline tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_timeline_basic() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        let mut m1 = test_memory();
        m1.title = "First".to_string();
        m1.created_at = chrono::Utc::now() - chrono::Duration::hours(2);
        m1.updated_at = m1.created_at;

        let mut m2 = test_memory();
        m2.title = "Second".to_string();

        storage.save_memory(&m1, None).await.unwrap();
        storage.save_memory(&m2, None).await.unwrap();

        let query = TimelineQuery {
            limit: 10,
            ..Default::default()
        };
        let entries = storage.timeline(&query).await.unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Second"); // most recent first
        assert_eq!(entries[1].title, "First");
    }

    #[tokio::test]
    async fn test_timeline_with_filters() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        let mut m1 = test_memory();
        m1.project_id = Some("proj-a".to_string());
        let mut m2 = test_memory();
        m2.project_id = Some("proj-b".to_string());

        storage.save_memory(&m1, None).await.unwrap();
        storage.save_memory(&m2, None).await.unwrap();

        let query = TimelineQuery {
            project_id: Some("proj-a".to_string()),
            limit: 10,
            ..Default::default()
        };
        let entries = storage.timeline(&query).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, m1.id);
    }

    #[tokio::test]
    async fn test_timeline_with_kind_filter() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        let mut m1 = test_memory();
        m1.kind = MemoryKind::Error;
        m1.title = "Error memory".to_string();

        let mut m2 = test_memory();
        m2.kind = MemoryKind::Fix;
        m2.title = "Fix memory".to_string();

        storage.save_memory(&m1, None).await.unwrap();
        storage.save_memory(&m2, None).await.unwrap();

        let query = TimelineQuery {
            kind: Some(MemoryKind::Error),
            limit: 10,
            ..Default::default()
        };
        let entries = storage.timeline(&query).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Error memory");
    }

    #[tokio::test]
    async fn test_timeline_with_status_filter() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        let mut m1 = test_memory();
        m1.title = "Active memory".to_string();

        let mut m2 = test_memory();
        m2.status = MemoryStatus::Archived;
        m2.title = "Archived memory".to_string();

        storage.save_memory(&m1, None).await.unwrap();
        storage.save_memory(&m2, None).await.unwrap();

        let query = TimelineQuery {
            status: Some(MemoryStatus::Archived),
            limit: 10,
            ..Default::default()
        };
        let entries = storage.timeline(&query).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Archived memory");
    }

    // ── Graph tests ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_add_and_get_relations() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let m1 = test_memory();
        let mut m2 = test_memory();
        m2.title = "Related".to_string();

        storage.save_memory(&m1, None).await.unwrap();
        storage.save_memory(&m2, None).await.unwrap();

        let relation = MemoryRelation {
            source_id: m1.id,
            target_id: m2.id,
            relation_type: RelationType::Related,
            strength: 0.8,
        };
        storage.add_relation(&relation).await.unwrap();

        let relations = storage.get_relations(m1.id).await.unwrap();
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].target_id, m2.id);
        assert_eq!(relations[0].relation_type, RelationType::Related);
        assert!((relations[0].strength - 0.8).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn test_count_relations() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let m1 = test_memory();
        let mut m2 = test_memory();
        let mut m3 = test_memory();
        m2.title = "Two".to_string();
        m3.title = "Three".to_string();

        storage.save_memory(&m1, None).await.unwrap();
        storage.save_memory(&m2, None).await.unwrap();
        storage.save_memory(&m3, None).await.unwrap();

        storage
            .add_relation(&MemoryRelation {
                source_id: m1.id,
                target_id: m2.id,
                relation_type: RelationType::Related,
                strength: 0.5,
            })
            .await
            .unwrap();
        storage
            .add_relation(&MemoryRelation {
                source_id: m1.id,
                target_id: m3.id,
                relation_type: RelationType::Fixes,
                strength: 0.9,
            })
            .await
            .unwrap();

        let counts = storage.count_relations(&[m1.id, m2.id]).await.unwrap();
        let m1_count = counts.iter().find(|(id, _)| *id == m1.id).map(|(_, c)| *c);
        assert_eq!(m1_count, Some(2));
    }

    #[tokio::test]
    async fn test_count_contradictions() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let m1 = test_memory();
        let mut m2 = test_memory();
        let mut m3 = test_memory();
        m2.title = "Contradicted".to_string();
        m3.title = "Related only".to_string();

        storage.save_memory(&m1, None).await.unwrap();
        storage.save_memory(&m2, None).await.unwrap();
        storage.save_memory(&m3, None).await.unwrap();

        storage
            .add_relation(&MemoryRelation {
                source_id: m1.id,
                target_id: m2.id,
                relation_type: RelationType::Contradicts,
                strength: 0.9,
            })
            .await
            .unwrap();
        storage
            .add_relation(&MemoryRelation {
                source_id: m1.id,
                target_id: m3.id,
                relation_type: RelationType::Related,
                strength: 0.5,
            })
            .await
            .unwrap();

        let counts = storage.count_contradictions(&[m1.id]).await.unwrap();
        let m1_count = counts.iter().find(|(id, _)| *id == m1.id).map(|(_, c)| *c);
        assert_eq!(m1_count, Some(1)); // Only contradictions, not related
    }

    // ── Session tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_save_and_get_session() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let session = Session {
            id: Uuid::now_v7(),
            project_id: Some("my-project".to_string()),
            started_at: Utc::now(),
            ended_at: None,
            summary: Some("Test session".to_string()),
            memory_count: 5,
        };
        storage.save_session(&session).await.unwrap();
        let loaded = storage.get_session(session.id).await.unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.project_id, Some("my-project".to_string()));
        assert_eq!(loaded.summary, Some("Test session".to_string()));
        assert_eq!(loaded.memory_count, 5);
    }

    #[tokio::test]
    async fn test_save_memory_writes_to_vec_memories() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        // vec_memories is defined with float[128], so use a 128-dimensional vector
        let mut emb = vec![0.0_f32; 128];
        emb[0] = 1.0;
        storage.save_memory(&mem, Some(&emb)).await.unwrap();

        let conn = storage.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "vec_memories should have one entry");

        let stored_id: String = conn
            .query_row("SELECT memory_id FROM vec_memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored_id, mem.id.to_string());
    }

    #[tokio::test]
    async fn test_save_memory_no_embedding_skips_vec() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        storage.save_memory(&mem, None).await.unwrap();

        let conn = storage.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0, "vec_memories should be empty when no embedding");
    }

    #[tokio::test]
    async fn test_delete_memory_removes_vec_embedding() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        let emb = vec![0.5_f32; 128];
        storage.save_memory(&mem, Some(&emb)).await.unwrap();

        storage.delete_memory(mem.id).await.unwrap();

        let conn = storage.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0, "vec_memories should be empty after delete");
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let result = storage.get_session(Uuid::now_v7()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_save_session_upsert() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let id = Uuid::now_v7();
        let session = Session {
            id,
            project_id: None,
            started_at: Utc::now(),
            ended_at: None,
            summary: None,
            memory_count: 0,
        };
        storage.save_session(&session).await.unwrap();

        let updated = Session {
            id,
            project_id: None,
            started_at: session.started_at,
            ended_at: Some(Utc::now()),
            summary: Some("Done".to_string()),
            memory_count: 3,
        };
        storage.save_session(&updated).await.unwrap();

        let loaded = storage.get_session(id).await.unwrap();
        assert_eq!(loaded.summary, Some("Done".to_string()));
        assert_eq!(loaded.memory_count, 3);
    }

    // ── schema versioning tests ──────────────────────────────────────

    #[test]
    fn test_schema_version_stamped_on_fresh_db() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let (version, writer) = storage.schema_info().unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert!(writer.is_some());
        assert_eq!(writer.unwrap(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_metadata_table_exists() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let conn = storage.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM metadata", [], |row| row.get(0))
            .unwrap();
        assert!(count >= 1, "metadata table should have at least one row");
    }

    // ── timeline offset, privacy, count tests ────────────────────────

    #[tokio::test]
    async fn test_timeline_with_offset() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        // Insert 5 memories with different timestamps
        for i in 0..5 {
            let mut mem = test_memory();
            mem.title = format!("Memory {i}");
            mem.content = format!("Content {i}");
            mem.created_at = Utc::now() - chrono::Duration::milliseconds((5 - i) * 100);
            mem.updated_at = mem.created_at;
            storage.save_memory(&mem, None).await.unwrap();
        }

        let query = TimelineQuery {
            limit: 2,
            offset: 2,
            ..Default::default()
        };
        let results = storage.timeline(&query).await.unwrap();
        assert_eq!(results.len(), 2);
        // With DESC ordering, offset=2 skips the 2 newest
        // We have 5 items ordered newest-first, offset 2 gives items at index 2,3
    }

    #[tokio::test]
    async fn test_timeline_with_privacy_filter() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        // Insert memories with different privacy levels
        for (i, privacy) in [
            MemoryPrivacy::Public,
            MemoryPrivacy::Private,
            MemoryPrivacy::Public,
        ]
        .iter()
        .enumerate()
        {
            let mut mem = test_memory();
            mem.title = format!("Memory {privacy:?} {i}");
            mem.privacy = *privacy;
            storage.save_memory(&mem, None).await.unwrap();
        }

        let query = TimelineQuery {
            privacy: Some(MemoryPrivacy::Private),
            limit: 100,
            ..Default::default()
        };
        let results = storage.timeline(&query).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_timeline_count() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        for i in 0..5 {
            let mut mem = test_memory();
            mem.title = format!("Memory {i}");
            mem.content = format!("Content {i}");
            storage.save_memory(&mem, None).await.unwrap();
        }

        let query = TimelineQuery::default();
        let count = storage.timeline_count(&query).await.unwrap();
        assert_eq!(count, 5);

        // Count with filter that matches none
        let query = TimelineQuery {
            kind: Some(MemoryKind::Decision),
            ..Default::default()
        };
        let count = storage.timeline_count(&query).await.unwrap();
        assert_eq!(count, 0);
    }

    // ── integrity check tests ────────────────────────────────────────

    #[test]
    fn test_integrity_check_clean_db() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let report = storage.integrity_check().unwrap();
        assert!(report.sqlite_integrity_ok);
        assert!(report.orphaned_embeddings.is_empty());
        assert!(report.broken_relations.is_empty());
        assert_eq!(report.total_memories, 0);
        assert_eq!(report.total_embeddings, 0);
        assert_eq!(report.total_relations, 0);
        assert_eq!(report.total_sessions, 0);
        assert_eq!(report.missing_embeddings, 0);
    }

    #[tokio::test]
    async fn test_integrity_check_with_data() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        storage.save_memory(&mem, None).await.unwrap();

        let report = storage.integrity_check().unwrap();
        assert!(report.sqlite_integrity_ok);
        assert_eq!(report.total_memories, 1);
        assert_eq!(report.missing_embeddings, 1); // no embedding was saved
        assert!(report.orphaned_embeddings.is_empty());
        assert!(report.broken_relations.is_empty());
    }

    #[test]
    fn test_integrity_check_detects_orphaned_embedding() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        // Insert an embedding directly without a corresponding memory
        {
            let conn = storage.conn.lock().unwrap();
            // Must disable foreign keys temporarily to insert orphan
            conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
            conn.execute(
                "INSERT INTO embeddings (memory_id, vector, dimensions) VALUES (?1, ?2, ?3)",
                params!["nonexistent-id", vec![0u8; 128], 128],
            )
            .unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        }

        let report = storage.integrity_check().unwrap();
        assert_eq!(report.orphaned_embeddings.len(), 1);
        assert_eq!(report.orphaned_embeddings[0], "nonexistent-id");
    }

    #[test]
    fn test_integrity_repair_removes_orphaned_embeddings() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
            conn.execute(
                "INSERT INTO embeddings (memory_id, vector, dimensions) VALUES (?1, ?2, ?3)",
                params!["orphan-1", vec![0u8; 128], 128],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO embeddings (memory_id, vector, dimensions) VALUES (?1, ?2, ?3)",
                params!["orphan-2", vec![0u8; 128], 128],
            )
            .unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        }

        let report = storage.integrity_check().unwrap();
        assert_eq!(report.orphaned_embeddings.len(), 2);

        let (orphans, relations) = storage.repair(&report).unwrap();
        assert_eq!(orphans, 2);
        assert_eq!(relations, 0);

        // Verify they are gone
        let report_after = storage.integrity_check().unwrap();
        assert!(report_after.orphaned_embeddings.is_empty());
    }

    #[test]
    fn test_integrity_check_detects_broken_relations() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
            conn.execute(
                "INSERT INTO relations (source_id, target_id, relation_type, strength) \
                 VALUES (?1, ?2, ?3, ?4)",
                params!["missing-src", "missing-tgt", "related", 0.5],
            )
            .unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        }

        let report = storage.integrity_check().unwrap();
        assert_eq!(report.broken_relations.len(), 1);
        assert_eq!(report.broken_relations[0].0, "missing-src");
        assert_eq!(report.broken_relations[0].1, "missing-tgt");
    }

    #[test]
    fn test_integrity_repair_removes_broken_relations() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
            conn.execute(
                "INSERT INTO relations (source_id, target_id, relation_type, strength) \
                 VALUES (?1, ?2, ?3, ?4)",
                params!["missing-a", "missing-b", "related", 0.5],
            )
            .unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        }

        let report = storage.integrity_check().unwrap();
        assert_eq!(report.broken_relations.len(), 1);

        let (orphans, relations) = storage.repair(&report).unwrap();
        assert_eq!(orphans, 0);
        assert_eq!(relations, 1);

        // Verify relation is gone
        let report_after = storage.integrity_check().unwrap();
        assert!(report_after.broken_relations.is_empty());
    }

    // ── Pending status filtering tests ──────────────────────────────────

    #[tokio::test]
    async fn test_pending_excluded_from_timeline() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        let mut active = test_memory();
        active.title = "Active memory".to_string();

        let mut pending = test_memory();
        pending.status = MemoryStatus::Pending;
        pending.title = "Pending memory".to_string();

        storage.save_memory(&active, None).await.unwrap();
        storage.save_memory(&pending, None).await.unwrap();

        // Default query (no status filter) should exclude Pending
        let entries = storage
            .timeline(&TimelineQuery {
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Active memory");
    }

    #[tokio::test]
    async fn test_pending_included_when_explicitly_requested() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        let mut pending = test_memory();
        pending.status = MemoryStatus::Pending;
        pending.title = "Pending memory".to_string();

        storage.save_memory(&pending, None).await.unwrap();

        // Explicit Pending filter should include it
        let entries = storage
            .timeline(&TimelineQuery {
                status: Some(MemoryStatus::Pending),
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Pending memory");
    }

    #[tokio::test]
    async fn test_pending_excluded_from_search() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        // Create a memory with Pending status and an embedding
        let mut pending = test_memory();
        pending.status = MemoryStatus::Pending;
        pending.title = "Pending searchable memory".to_string();

        let embedding = vec![0.1_f32; 128];
        storage
            .save_memory(&pending, Some(&embedding))
            .await
            .unwrap();

        // Vector search should not return Pending memories
        let results = storage.vector_search(&embedding, 10).await.unwrap();
        assert!(
            results.is_empty(),
            "Pending memories should not appear in vector search"
        );
    }
}
