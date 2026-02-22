use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use std::sync::Once;

use crate::error::{Result, ShabkaError};
use crate::model::*;
use crate::storage::StorageBackend;

static EXTENSIONS_REGISTERED: Once = Once::new();

fn register_extensions() {
    EXTENSIONS_REGISTERED.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut i8,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
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

            CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories USING vec0(
                memory_id TEXT PRIMARY KEY,
                embedding float[128]
            );
            ",
        )
        .map_err(|e| ShabkaError::Storage(format!("failed to create tables: {e}")))?;

        Ok(())
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

/// Cosine similarity between two vectors. Returns 0.0 for zero-length vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }
    dot / (mag_a * mag_b)
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
                "INSERT INTO memories (id, kind, title, content, summary, tags, source, scope,
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
                    "INSERT INTO embeddings (memory_id, vector, dimensions) VALUES (?1, ?2, ?3)",
                    params![memory.id.to_string(), blob, dimensions],
                )
                .map_err(|e| ShabkaError::Storage(format!("failed to insert embedding: {e}")))?;
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
            // 1. Load all embeddings
            let mut stmt = conn
                .prepare("SELECT memory_id, vector, dimensions FROM embeddings")
                .map_err(|e| ShabkaError::Storage(format!("failed to prepare query: {e}")))?;

            let mut scored: Vec<(String, f32)> = Vec::new();

            let rows = stmt
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    let dims: i64 = row.get(2)?;
                    Ok((id, blob, dims))
                })
                .map_err(|e| ShabkaError::Storage(format!("failed to query embeddings: {e}")))?;

            for row in rows {
                let (id, blob, dims) = row.map_err(|e| {
                    ShabkaError::Storage(format!("failed to read embedding row: {e}"))
                })?;

                // 2. Deserialize BLOB to Vec<f32>
                if blob.len() != (dims as usize) * 4 {
                    continue; // corrupted embedding
                }
                let stored: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect();

                if stored.len() != query_vec.len() {
                    continue; // dimension mismatch
                }

                // 3. Score
                let score = cosine_similarity(&query_vec, &stored);
                scored.push((id, score));
            }

            // 4. Sort and truncate
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(limit);

            if scored.is_empty() {
                return Ok(Vec::new());
            }

            // 5. Fetch full Memory records
            let ids: Vec<String> = scored.iter().map(|(id, _)| id.clone()).collect();
            let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT * FROM memories WHERE id IN ({})",
                placeholders.join(", ")
            );
            let params: Vec<&dyn rusqlite::types::ToSql> = ids
                .iter()
                .map(|s| s as &dyn rusqlite::types::ToSql)
                .collect();

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| ShabkaError::Storage(format!("failed to prepare query: {e}")))?;
            let mem_rows = stmt
                .query_map(params.as_slice(), row_to_memory)
                .map_err(|e| ShabkaError::Storage(format!("failed to query memories: {e}")))?;

            let mut memory_map: std::collections::HashMap<String, Memory> =
                std::collections::HashMap::new();
            for row in mem_rows {
                let mem = row
                    .map_err(|e| ShabkaError::Storage(format!("failed to read memory row: {e}")))?;
                memory_map.insert(mem.id.to_string(), mem);
            }

            // 6. Reassemble in score order
            let results: Vec<(Memory, f32)> = scored
                .into_iter()
                .filter_map(|(id, score)| memory_map.remove(&id).map(|mem| (mem, score)))
                .collect();

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
                 LIMIT ?{idx}"
            );
            params.push(Box::new(query.limit as i64));

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

        let mut m1 = test_memory();
        m1.title = "Rust patterns".to_string();
        let emb1 = vec![1.0_f32, 0.0, 0.0];

        let mut m2 = test_memory();
        m2.title = "Rust lifetimes".to_string();
        let emb2 = vec![0.9, 0.1, 0.0];

        let mut m3 = test_memory();
        m3.title = "Python basics".to_string();
        let emb3 = vec![0.0, 0.0, 1.0];

        storage.save_memory(&m1, Some(&emb1)).await.unwrap();
        storage.save_memory(&m2, Some(&emb2)).await.unwrap();
        storage.save_memory(&m3, Some(&emb3)).await.unwrap();

        let query = vec![1.0_f32, 0.0, 0.0];
        let results = storage.vector_search(&query, 2).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0.title, "Rust patterns");
        assert_eq!(results[1].0.title, "Rust lifetimes");
        assert!(results[0].1 > 0.99);
        assert!(results[1].1 > 0.9);
    }

    #[tokio::test]
    async fn test_vector_search_no_embeddings() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        storage.save_memory(&mem, None).await.unwrap();

        let query = vec![1.0_f32, 0.0, 0.0];
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
}
