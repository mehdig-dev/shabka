# SQLite Storage Backend Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add SQLite as an alternative storage backend so users can run Shabka without HelixDB — just `cargo install shabka-cli && shabka init`.

**Architecture:** Implement `SqliteStorage` behind the existing `StorageBackend` trait (12 async methods). Uses `rusqlite` with `bundled` feature (statically links SQLite). Vector search is brute-force cosine similarity in Rust (sufficient for single-user scale). Config gets a new `[storage]` section with `backend = "sqlite"` (new default) or `"helix"`.

**Tech Stack:** `rusqlite` 0.34 (bundled), existing `StorageBackend` trait, `tokio::task::spawn_blocking` for sync→async bridge.

---

## Key Reference Files

| File | Purpose |
|------|---------|
| `crates/shabka-core/src/storage/backend.rs` | `StorageBackend` trait — 12 methods to implement |
| `crates/shabka-core/src/storage/helix.rs` | Reference implementation (HelixDB) — patterns to follow |
| `crates/shabka-core/src/storage/mod.rs` | Module declarations — add `sqlite` module here |
| `crates/shabka-core/src/model/memory.rs` | `Memory`, `UpdateMemoryInput`, `TimelineQuery`, `TimelineEntry`, enums |
| `crates/shabka-core/src/model/graph.rs` | `MemoryRelation`, `RelationType` |
| `crates/shabka-core/src/model/session.rs` | `Session` |
| `crates/shabka-core/src/config/mod.rs` | `ShabkaConfig` — add `StorageConfig` |
| `crates/shabka-core/src/error.rs` | `ShabkaError::Storage(String)` — SQLite errors map here |
| `crates/shabka-core/Cargo.toml` | Add `rusqlite` dependency |
| `Cargo.toml` (workspace root) | Add `rusqlite` to workspace deps |
| `docs/plans/2026-02-20-sqlite-mode-design.md` | Design doc with schema and decisions |

## Enum Serialization Reference

These enums are stored as JSON strings in the database (same as HelixDB):

```rust
// MemoryKind: "observation", "decision", "pattern", "error", "fix", "preference", "fact", "lesson", "todo"
// MemorySource: tagged enum — "manual", {"auto_capture":{"hook":"..."}} etc
// MemoryScope: tagged enum — "global", {"project":{"id":"..."}} etc
// MemoryStatus: "active", "archived", "superseded"
// MemoryPrivacy: "public", "team", "private"
// VerificationStatus: "unverified", "verified", "disputed", "outdated"
// RelationType: "caused_by", "fixes", "supersedes", "related", "contradicts"
```

All use `#[serde(rename_all = "snake_case")]` so `serde_json::to_string()` / `from_str()` works.

---

### Task 1: Add rusqlite dependency and create SqliteStorage struct with schema

**Files:**
- Modify: `Cargo.toml` (workspace root, dependencies section)
- Modify: `crates/shabka-core/Cargo.toml` (add rusqlite dep)
- Create: `crates/shabka-core/src/storage/sqlite.rs`
- Modify: `crates/shabka-core/src/storage/mod.rs` (add module)

**Step 1: Add rusqlite to workspace dependencies**

In `Cargo.toml` (workspace root), add under `[workspace.dependencies]` after the `helix-rs` line:

```toml
rusqlite = { version = "0.34", features = ["bundled"] }
```

In `crates/shabka-core/Cargo.toml`, add under `[dependencies]`:

```toml
rusqlite = { workspace = true }
```

**Step 2: Create the SqliteStorage struct with schema initialization**

Create `crates/shabka-core/src/storage/sqlite.rs`:

```rust
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{Result, ShabkaError};
use crate::model::*;

use super::StorageBackend;

/// SQLite storage backend.
///
/// Single-file database at `~/.config/shabka/shabka.db` (configurable).
/// Uses `rusqlite` (bundled SQLite) with a `Mutex<Connection>` for thread safety.
/// All trait methods use `tokio::task::spawn_blocking` to bridge sync rusqlite → async.
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl SqliteStorage {
    /// Open (or create) a SQLite database at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ShabkaError::Storage(format!("failed to create database directory: {e}"))
            })?;
        }

        let conn = Connection::open(&path)
            .map_err(|e| ShabkaError::Storage(format!("failed to open SQLite database: {e}")))?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| ShabkaError::Storage(format!("failed to set pragmas: {e}")))?;

        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
        };
        storage.create_tables()?;
        Ok(storage)
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| ShabkaError::Storage(format!("failed to open in-memory database: {e}")))?;

        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| ShabkaError::Storage(format!("failed to set pragmas: {e}")))?;

        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
            path: PathBuf::from(":memory:"),
        };
        storage.create_tables()?;
        Ok(storage)
    }

    /// Returns the database file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn create_tables(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            ShabkaError::Storage(format!("failed to acquire database lock: {e}"))
        })?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
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
            CREATE INDEX IF NOT EXISTS idx_relations_target ON relations(target_id);",
        )
        .map_err(|e| ShabkaError::Storage(format!("failed to create tables: {e}")))?;

        Ok(())
    }

    /// Helper: acquire lock and run a closure on the connection inside spawn_blocking.
    async fn with_conn<F, T>(&self, f: F) -> Result<T>
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
```

**Step 3: Register the module**

In `crates/shabka-core/src/storage/mod.rs`, add:

```rust
mod sqlite;
pub use sqlite::SqliteStorage;
```

The file should look like:

```rust
mod backend;
mod helix;
mod sqlite;

pub use backend::StorageBackend;
pub use helix::HelixStorage;
pub use sqlite::SqliteStorage;
```

**Step 4: Verify it compiles**

Run: `cargo check -p shabka-core --no-default-features`
Expected: Compiles (though with dead_code warnings since trait isn't implemented yet)

**Step 5: Commit**

```bash
git add Cargo.toml crates/shabka-core/Cargo.toml crates/shabka-core/src/storage/sqlite.rs crates/shabka-core/src/storage/mod.rs
git commit -m "feat(storage): add SqliteStorage struct with schema creation"
```

---

### Task 2: Implement Memory CRUD (save, get, get_many, update, delete)

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs`
- Test: inline `#[cfg(test)]` module in same file

**Step 1: Write failing tests for CRUD operations**

Add to the bottom of `sqlite.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

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

    #[tokio::test]
    async fn test_save_and_get_memory() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();

        storage.save_memory(&mem, None).await.unwrap();
        let loaded = storage.get_memory(mem.id).await.unwrap();

        assert_eq!(loaded.id, mem.id);
        assert_eq!(loaded.title, "Test memory");
        assert_eq!(loaded.content, "Some content");
        assert_eq!(loaded.tags, vec!["test"]);
        assert!((loaded.importance - 0.7).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn test_get_memory_not_found() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let result = storage.get_memory(Uuid::now_v7()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_memories_batch() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let m1 = test_memory();
        let mut m2 = test_memory();
        m2.title = "Second".to_string();

        storage.save_memory(&m1, None).await.unwrap();
        storage.save_memory(&m2, None).await.unwrap();

        let loaded = storage.get_memories(&[m1.id, m2.id]).await.unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[tokio::test]
    async fn test_update_memory() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        storage.save_memory(&mem, None).await.unwrap();

        let input = UpdateMemoryInput {
            title: Some("Updated title".to_string()),
            importance: Some(0.9),
            ..Default::default()
        };
        let updated = storage.update_memory(mem.id, &input).await.unwrap();
        assert_eq!(updated.title, "Updated title");
        assert!((updated.importance - 0.9).abs() < f32::EPSILON);
        // Unchanged fields preserved
        assert_eq!(updated.content, "Some content");
    }

    #[tokio::test]
    async fn test_delete_memory() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        storage.save_memory(&mem, None).await.unwrap();

        storage.delete_memory(mem.id).await.unwrap();
        assert!(storage.get_memory(mem.id).await.is_err());
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests --nocapture 2>&1 | head -30`
Expected: Compile errors — `StorageBackend` not implemented for `SqliteStorage`

**Step 3: Implement the CRUD methods**

Add to `sqlite.rs`, after the `impl SqliteStorage` block and before `#[cfg(test)]`:

```rust
/// Convert a rusqlite Row to a Memory struct.
fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
    let id_str: String = row.get("id")?;
    let kind_str: String = row.get("kind")?;
    let tags_str: String = row.get("tags")?;
    let source_str: String = row.get("source")?;
    let scope_str: String = row.get("scope")?;
    let status_str: String = row.get("status")?;
    let privacy_str: String = row.get("privacy")?;
    let verification_str: String = row.get("verification")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;
    let accessed_at_str: String = row.get("accessed_at")?;

    Ok(Memory {
        id: uuid::Uuid::parse_str(&id_str).unwrap_or_default(),
        kind: serde_json::from_str(&format!("\"{kind_str}\"")).unwrap_or(MemoryKind::Observation),
        title: row.get("title")?,
        content: row.get("content")?,
        summary: row.get("summary")?,
        tags: serde_json::from_str(&tags_str).unwrap_or_default(),
        source: serde_json::from_str(&source_str).unwrap_or(MemorySource::Manual),
        scope: serde_json::from_str(&scope_str).unwrap_or(MemoryScope::Global),
        importance: row.get("importance")?,
        status: serde_json::from_str(&format!("\"{status_str}\"")).unwrap_or(MemoryStatus::Active),
        privacy: serde_json::from_str(&format!("\"{privacy_str}\""))
            .unwrap_or(MemoryPrivacy::Private),
        verification: serde_json::from_str(&format!("\"{verification_str}\""))
            .unwrap_or(VerificationStatus::Unverified),
        project_id: row.get("project_id")?,
        session_id: row.get("session_id")?,
        created_by: row.get("created_by")?,
        created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_default(),
        updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_default(),
        accessed_at: chrono::DateTime::parse_from_rfc3339(&accessed_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_default(),
    })
}

/// Serialize a MemoryKind to its database string (e.g. "observation").
fn kind_to_str(kind: &MemoryKind) -> String {
    serde_json::to_string(kind)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

/// Serialize a MemoryStatus to its database string (e.g. "active").
fn status_to_str(status: &MemoryStatus) -> String {
    serde_json::to_string(status)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

/// Serialize a MemoryPrivacy to its database string.
fn privacy_to_str(privacy: &MemoryPrivacy) -> String {
    serde_json::to_string(privacy)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

/// Serialize a VerificationStatus to its database string.
fn verification_to_str(verification: &VerificationStatus) -> String {
    serde_json::to_string(verification)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

impl StorageBackend for SqliteStorage {
    async fn save_memory(&self, memory: &Memory, embedding: Option<&[f32]>) -> Result<()> {
        let memory = memory.clone();
        let embedding = embedding.map(|e| e.to_vec());

        self.with_conn(move |conn| {
            let tx = conn.unchecked_transaction().map_err(|e| {
                ShabkaError::Storage(format!("failed to begin transaction: {e}"))
            })?;

            tx.execute(
                "INSERT INTO memories (id, kind, title, content, summary, tags, source, scope,
                    importance, status, privacy, verification, project_id, session_id,
                    created_by, created_at, updated_at, accessed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                rusqlite::params![
                    memory.id.to_string(),
                    kind_to_str(&memory.kind),
                    memory.title,
                    memory.content,
                    memory.summary,
                    serde_json::to_string(&memory.tags).unwrap_or_default(),
                    serde_json::to_string(&memory.source).unwrap_or_default(),
                    serde_json::to_string(&memory.scope).unwrap_or_default(),
                    memory.importance,
                    status_to_str(&memory.status),
                    privacy_to_str(&memory.privacy),
                    verification_to_str(&memory.verification),
                    memory.project_id,
                    memory.session_id.map(|s| s.to_string()),
                    memory.created_by,
                    memory.created_at.to_rfc3339(),
                    memory.updated_at.to_rfc3339(),
                    memory.accessed_at.to_rfc3339(),
                ],
            )
            .map_err(|e| ShabkaError::Storage(format!("failed to insert memory: {e}")))?;

            if let Some(emb) = embedding {
                let dims = emb.len() as i64;
                let blob: Vec<u8> = emb.iter().flat_map(|f| f.to_le_bytes()).collect();
                tx.execute(
                    "INSERT INTO embeddings (memory_id, vector, dimensions) VALUES (?1, ?2, ?3)",
                    rusqlite::params![memory.id.to_string(), blob, dims],
                )
                .map_err(|e| ShabkaError::Storage(format!("failed to insert embedding: {e}")))?;
            }

            tx.commit().map_err(|e| {
                ShabkaError::Storage(format!("failed to commit transaction: {e}"))
            })?;
            Ok(())
        })
        .await
    }

    async fn get_memory(&self, id: uuid::Uuid) -> Result<Memory> {
        self.with_conn(move |conn| {
            conn.query_row(
                "SELECT * FROM memories WHERE id = ?1",
                rusqlite::params![id.to_string()],
                row_to_memory,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    ShabkaError::NotFound(format!("memory {id}"))
                }
                _ => ShabkaError::Storage(format!("failed to get memory: {e}")),
            })
        })
        .await
    }

    async fn get_memories(&self, ids: &[uuid::Uuid]) -> Result<Vec<Memory>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();

        self.with_conn(move |conn| {
            let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT * FROM memories WHERE id IN ({})",
                placeholders.join(", ")
            );
            let params: Vec<&dyn rusqlite::types::ToSql> =
                ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| {
                ShabkaError::Storage(format!("failed to prepare query: {e}"))
            })?;

            let rows = stmt
                .query_map(params.as_slice(), row_to_memory)
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

    async fn update_memory(&self, id: uuid::Uuid, input: &UpdateMemoryInput) -> Result<Memory> {
        let input = input.clone();

        self.with_conn(move |conn| {
            // Build dynamic SET clause from non-None fields
            let mut sets: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(ref title) = input.title {
                sets.push(format!("title = ?{idx}"));
                params.push(Box::new(title.clone()));
                idx += 1;
            }
            if let Some(ref content) = input.content {
                sets.push(format!("content = ?{idx}"));
                params.push(Box::new(content.clone()));
                idx += 1;
            }
            if let Some(ref summary) = input.summary {
                sets.push(format!("summary = ?{idx}"));
                params.push(Box::new(summary.clone()));
                idx += 1;
            }
            if let Some(ref tags) = input.tags {
                sets.push(format!("tags = ?{idx}"));
                params.push(Box::new(serde_json::to_string(tags).unwrap_or_default()));
                idx += 1;
            }
            if let Some(importance) = input.importance {
                sets.push(format!("importance = ?{idx}"));
                params.push(Box::new(importance as f64));
                idx += 1;
            }
            if let Some(ref status) = input.status {
                sets.push(format!("status = ?{idx}"));
                params.push(Box::new(status_to_str(status)));
                idx += 1;
            }
            if let Some(ref privacy) = input.privacy {
                sets.push(format!("privacy = ?{idx}"));
                params.push(Box::new(privacy_to_str(privacy)));
                idx += 1;
            }
            if let Some(ref verification) = input.verification {
                sets.push(format!("verification = ?{idx}"));
                params.push(Box::new(verification_to_str(verification)));
                idx += 1;
            }

            // Always update updated_at
            sets.push(format!("updated_at = ?{idx}"));
            params.push(Box::new(chrono::Utc::now().to_rfc3339()));
            idx += 1;

            if sets.is_empty() {
                // Nothing to update — just return the current memory
                return conn
                    .query_row(
                        "SELECT * FROM memories WHERE id = ?1",
                        rusqlite::params![id.to_string()],
                        row_to_memory,
                    )
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => {
                            ShabkaError::NotFound(format!("memory {id}"))
                        }
                        _ => ShabkaError::Storage(format!("failed to get memory: {e}")),
                    });
            }

            let sql = format!(
                "UPDATE memories SET {} WHERE id = ?{idx}",
                sets.join(", ")
            );
            params.push(Box::new(id.to_string()));

            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let rows_affected = conn.execute(&sql, param_refs.as_slice()).map_err(|e| {
                ShabkaError::Storage(format!("failed to update memory: {e}"))
            })?;

            if rows_affected == 0 {
                return Err(ShabkaError::NotFound(format!("memory {id}")));
            }

            conn.query_row(
                "SELECT * FROM memories WHERE id = ?1",
                rusqlite::params![id.to_string()],
                row_to_memory,
            )
            .map_err(|e| ShabkaError::Storage(format!("failed to read updated memory: {e}")))
        })
        .await
    }

    async fn delete_memory(&self, id: uuid::Uuid) -> Result<()> {
        self.with_conn(move |conn| {
            let rows = conn
                .execute(
                    "DELETE FROM memories WHERE id = ?1",
                    rusqlite::params![id.to_string()],
                )
                .map_err(|e| ShabkaError::Storage(format!("failed to delete memory: {e}")))?;

            if rows == 0 {
                return Err(ShabkaError::NotFound(format!("memory {id}")));
            }
            Ok(())
        })
        .await
    }

    // Placeholder stubs for remaining trait methods (implemented in later tasks)
    async fn vector_search(&self, _embedding: &[f32], _limit: usize) -> Result<Vec<(Memory, f32)>> {
        Ok(Vec::new())
    }

    async fn timeline(&self, _query: &TimelineQuery) -> Result<Vec<TimelineEntry>> {
        Ok(Vec::new())
    }

    async fn add_relation(&self, _relation: &MemoryRelation) -> Result<()> {
        Ok(())
    }

    async fn get_relations(&self, _memory_id: uuid::Uuid) -> Result<Vec<MemoryRelation>> {
        Ok(Vec::new())
    }

    async fn count_relations(&self, _memory_ids: &[uuid::Uuid]) -> Result<Vec<(uuid::Uuid, usize)>> {
        Ok(Vec::new())
    }

    async fn count_contradictions(&self, _memory_ids: &[uuid::Uuid]) -> Result<Vec<(uuid::Uuid, usize)>> {
        Ok(Vec::new())
    }

    async fn save_session(&self, _session: &Session) -> Result<()> {
        Ok(())
    }

    async fn get_session(&self, _id: uuid::Uuid) -> Result<Session> {
        Err(ShabkaError::NotFound("not implemented".to_string()))
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests --nocapture`
Expected: 5 tests pass (save_and_get, not_found, batch, update, delete)

**Step 5: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): implement SQLite CRUD operations for Memory"
```

---

### Task 3: Implement vector_search with brute-force cosine similarity

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs`

**Step 1: Write failing test**

Add to the `tests` module in `sqlite.rs`:

```rust
    #[tokio::test]
    async fn test_vector_search() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        // Save 3 memories with embeddings
        let mut m1 = test_memory();
        m1.title = "Rust patterns".to_string();
        let emb1 = vec![1.0_f32, 0.0, 0.0]; // unit vector along x

        let mut m2 = test_memory();
        m2.title = "Rust lifetimes".to_string();
        let emb2 = vec![0.9, 0.1, 0.0]; // close to x

        let mut m3 = test_memory();
        m3.title = "Python basics".to_string();
        let emb3 = vec![0.0, 0.0, 1.0]; // unit vector along z — very different

        storage.save_memory(&m1, Some(&emb1)).await.unwrap();
        storage.save_memory(&m2, Some(&emb2)).await.unwrap();
        storage.save_memory(&m3, Some(&emb3)).await.unwrap();

        // Search for something close to x-axis
        let query = vec![1.0_f32, 0.0, 0.0];
        let results = storage.vector_search(&query, 2).await.unwrap();

        assert_eq!(results.len(), 2);
        // m1 should be first (exact match), m2 second (close)
        assert_eq!(results[0].0.title, "Rust patterns");
        assert_eq!(results[1].0.title, "Rust lifetimes");
        // Scores should be in [0, 1]
        assert!(results[0].1 > 0.99); // cosine sim of identical vectors ≈ 1.0
        assert!(results[1].1 > 0.9);
    }

    #[tokio::test]
    async fn test_vector_search_no_embeddings() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        let mem = test_memory();
        storage.save_memory(&mem, None).await.unwrap(); // No embedding

        let query = vec![1.0_f32, 0.0, 0.0];
        let results = storage.vector_search(&query, 10).await.unwrap();
        assert!(results.is_empty());
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests::test_vector_search --nocapture`
Expected: FAIL — vector_search returns empty vec (stub)

**Step 3: Implement vector_search**

Replace the `vector_search` stub in the `StorageBackend` impl:

```rust
    async fn vector_search(&self, embedding: &[f32], limit: usize) -> Result<Vec<(Memory, f32)>> {
        let query_vec = embedding.to_vec();

        self.with_conn(move |conn| {
            // Load all embeddings
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

                // Deserialize BLOB to Vec<f32>
                if blob.len() != (dims as usize) * 4 {
                    continue; // corrupted embedding, skip
                }
                let stored: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect();

                if stored.len() != query_vec.len() {
                    continue; // dimension mismatch, skip
                }

                let score = cosine_similarity(&query_vec, &stored);
                scored.push((id, score));
            }

            // Sort by score descending
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(limit);

            if scored.is_empty() {
                return Ok(Vec::new());
            }

            // Fetch full Memory records for top results
            let ids: Vec<String> = scored.iter().map(|(id, _)| id.clone()).collect();
            let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT * FROM memories WHERE id IN ({})",
                placeholders.join(", ")
            );
            let params: Vec<&dyn rusqlite::types::ToSql> =
                ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| {
                ShabkaError::Storage(format!("failed to prepare query: {e}"))
            })?;
            let mem_rows = stmt
                .query_map(params.as_slice(), row_to_memory)
                .map_err(|e| ShabkaError::Storage(format!("failed to query memories: {e}")))?;

            let mut memory_map: std::collections::HashMap<String, Memory> =
                std::collections::HashMap::new();
            for row in mem_rows {
                let mem = row.map_err(|e| {
                    ShabkaError::Storage(format!("failed to read memory row: {e}"))
                })?;
                memory_map.insert(mem.id.to_string(), mem);
            }

            // Reassemble in score order
            let results: Vec<(Memory, f32)> = scored
                .into_iter()
                .filter_map(|(id, score)| memory_map.remove(&id).map(|mem| (mem, score)))
                .collect();

            Ok(results)
        })
        .await
    }
```

Add the cosine similarity helper above the `impl StorageBackend` block:

```rust
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
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests::test_vector_search --nocapture`
Expected: 2 tests pass

**Step 5: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): implement brute-force cosine vector search for SQLite"
```

---

### Task 4: Implement timeline query

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs`

**Step 1: Write failing test**

Add to the `tests` module:

```rust
    #[tokio::test]
    async fn test_timeline_basic() {
        let storage = SqliteStorage::open_in_memory().unwrap();

        let mut m1 = test_memory();
        m1.title = "First".to_string();
        m1.created_at = chrono::Utc::now() - chrono::Duration::hours(2);
        m1.updated_at = m1.created_at;

        let mut m2 = test_memory();
        m2.title = "Second".to_string();
        // m2 has default Utc::now(), so it's more recent

        storage.save_memory(&m1, None).await.unwrap();
        storage.save_memory(&m2, None).await.unwrap();

        let query = TimelineQuery {
            memory_id: None,
            start: None,
            end: None,
            session_id: None,
            limit: Some(10),
            project_id: None,
        };
        let entries = storage.timeline(&query).await.unwrap();

        assert_eq!(entries.len(), 2);
        // Most recent first
        assert_eq!(entries[0].title, "Second");
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
            memory_id: None,
            start: None,
            end: None,
            session_id: None,
            limit: Some(10),
            project_id: Some("proj-a".to_string()),
        };
        let entries = storage.timeline(&query).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, m1.id);
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests::test_timeline --nocapture`
Expected: FAIL — timeline returns empty vec (stub)

**Step 3: Implement timeline**

Replace the `timeline` stub:

```rust
    async fn timeline(&self, query: &TimelineQuery) -> Result<Vec<TimelineEntry>> {
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

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let limit = query.limit.unwrap_or(50);

            let sql = format!(
                "SELECT m.*,
                    (SELECT COUNT(*) FROM relations r WHERE r.source_id = m.id) as related_count
                 FROM memories m
                 {where_clause}
                 ORDER BY m.created_at DESC
                 LIMIT ?{idx}"
            );
            params.push(Box::new(limit as i64));

            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| {
                ShabkaError::Storage(format!("failed to prepare timeline query: {e}"))
            })?;

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    let memory = row_to_memory(row)?;
                    let related_count: i64 = row.get("related_count")?;
                    Ok(TimelineEntry {
                        id: memory.id,
                        kind: memory.kind,
                        title: memory.title,
                        summary: memory.summary.clone(),
                        importance: memory.importance,
                        status: memory.status,
                        privacy: memory.privacy,
                        verification: memory.verification,
                        project_id: memory.project_id,
                        session_id: memory.session_id,
                        created_at: memory.created_at,
                        updated_at: memory.updated_at,
                        related_count: related_count as usize,
                    })
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
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests::test_timeline --nocapture`
Expected: 2 tests pass

**Step 5: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): implement SQLite timeline query with SQL filtering"
```

---

### Task 5: Implement graph methods (relations, counts, contradictions)

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs`

**Step 1: Write failing tests**

Add to the `tests` module:

```rust
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
        m2.title = "Contradicts".to_string();
        m3.title = "Related".to_string();

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
        assert_eq!(m1_count, Some(1)); // Only 1 contradiction, not the "related"
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests::test_add_and_get_relations sqlite::tests::test_count --nocapture`
Expected: FAIL — stubs return empty vecs

**Step 3: Implement graph methods**

Replace the stubs for `add_relation`, `get_relations`, `count_relations`, `count_contradictions`:

```rust
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

    async fn get_relations(&self, memory_id: uuid::Uuid) -> Result<Vec<MemoryRelation>> {
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
                    source_id: uuid::Uuid::parse_str(&source_str).unwrap_or_default(),
                    target_id: uuid::Uuid::parse_str(&target_str).unwrap_or_default(),
                    relation_type: serde_json::from_str(&format!("\"{rel_type_str}\""))
                        .unwrap_or(RelationType::Related),
                    strength,
                });
            }
            Ok(relations)
        })
        .await
    }

    async fn count_relations(&self, memory_ids: &[uuid::Uuid]) -> Result<Vec<(uuid::Uuid, usize)>> {
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
            let params: Vec<&dyn rusqlite::types::ToSql> =
                ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| {
                ShabkaError::Storage(format!("failed to prepare query: {e}"))
            })?;

            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    let id_str: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((id_str, count as usize))
                })
                .map_err(|e| ShabkaError::Storage(format!("failed to count relations: {e}")))?;

            let mut counts = Vec::new();
            for row in rows {
                let (id_str, count) = row.map_err(|e| {
                    ShabkaError::Storage(format!("failed to read count row: {e}"))
                })?;
                if let Ok(id) = uuid::Uuid::parse_str(&id_str) {
                    counts.push((id, count));
                }
            }
            Ok(counts)
        })
        .await
    }

    async fn count_contradictions(
        &self,
        memory_ids: &[uuid::Uuid],
    ) -> Result<Vec<(uuid::Uuid, usize)>> {
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
            let params: Vec<&dyn rusqlite::types::ToSql> =
                ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| {
                ShabkaError::Storage(format!("failed to prepare query: {e}"))
            })?;

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
                let (id_str, count) = row.map_err(|e| {
                    ShabkaError::Storage(format!("failed to read count row: {e}"))
                })?;
                if let Ok(id) = uuid::Uuid::parse_str(&id_str) {
                    counts.push((id, count));
                }
            }
            Ok(counts)
        })
        .await
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests --nocapture`
Expected: All 10 tests pass (5 CRUD + 2 vector + 2 timeline + 3 graph... actually let me recount: save_and_get, not_found, batch, update, delete, vector_search, vector_no_emb, timeline_basic, timeline_filters, add_get_relations, count_relations, count_contradictions — 12 tests)

**Step 5: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): implement SQLite graph methods (relations, counts, contradictions)"
```

---

### Task 6: Implement session methods (save_session, get_session)

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs`

**Step 1: Write failing tests**

Add to the `tests` module:

```rust
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

        // Update via save (upsert)
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests::test_save_and_get_session sqlite::tests::test_get_session_not_found sqlite::tests::test_save_session_upsert --nocapture`
Expected: FAIL — stubs

**Step 3: Implement session methods**

Replace the `save_session` and `get_session` stubs:

```rust
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

    async fn get_session(&self, id: uuid::Uuid) -> Result<Session> {
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
```

**Step 4: Run all SQLite tests**

Run: `cargo test -p shabka-core --no-default-features -- sqlite::tests --nocapture`
Expected: All 15 tests pass

**Step 5: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): implement SQLite session save/get with upsert"
```

---

### Task 7: Add StorageConfig and wire up backend selection

**Files:**
- Modify: `crates/shabka-core/src/config/mod.rs`
- Modify: (wherever `HelixStorage::new()` is called — MCP server, web, CLI, hooks)

**Step 1: Write failing config test**

Add to the `tests` module in `config/mod.rs`:

```rust
    #[test]
    fn test_storage_config_defaults() {
        let config = ShabkaConfig::default_config();
        assert_eq!(config.storage.backend, "sqlite");
        assert!(config.storage.path.is_none());
    }

    #[test]
    fn test_storage_config_helix() {
        let toml_str = r#"
[storage]
backend = "helix"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.storage.backend, "helix");
    }

    #[test]
    fn test_storage_config_sqlite_custom_path() {
        let toml_str = r#"
[storage]
backend = "sqlite"
path = "/tmp/my-shabka.db"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.storage.backend, "sqlite");
        assert_eq!(config.storage.path.as_deref(), Some("/tmp/my-shabka.db"));
    }

    #[test]
    fn test_storage_config_backward_compat() {
        // Old configs without [storage] should default to sqlite
        let toml_str = r#"
[embedding]
provider = "hash"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.storage.backend, "sqlite");
    }

    #[test]
    fn test_validate_unknown_storage_backend() {
        let mut config = ShabkaConfig::default_config();
        config.storage.backend = "banana".to_string();
        let warnings = config.validate();
        assert!(warnings.iter().any(|w| w.contains("unknown storage backend")));
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p shabka-core --no-default-features -- config::tests::test_storage --nocapture`
Expected: FAIL — no `storage` field on `ShabkaConfig`

**Step 3: Add StorageConfig to config**

In `crates/shabka-core/src/config/mod.rs`, add the struct and defaults:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_backend")]
    pub backend: String,
    /// Custom path for SQLite database. Defaults to `~/.config/shabka/shabka.db`.
    #[serde(default)]
    pub path: Option<String>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: default_storage_backend(),
            path: None,
        }
    }
}

fn default_storage_backend() -> String {
    "sqlite".to_string()
}

/// Valid storage backend names.
pub const VALID_STORAGE_BACKENDS: &[&str] = &["sqlite", "helix"];
```

Add the field to `ShabkaConfig`:

```rust
pub struct ShabkaConfig {
    #[serde(default)]
    pub storage: StorageConfig,
    // ... existing fields ...
```

Add to `default_config()`:

```rust
    pub fn default_config() -> Self {
        Self {
            storage: StorageConfig::default(),
            // ... existing fields ...
```

Add validation for the storage backend in `validate()`:

```rust
        // Storage backend
        if !VALID_STORAGE_BACKENDS.contains(&self.storage.backend.as_str()) {
            warnings.push(format!(
                "unknown storage backend '{}', valid: {}",
                self.storage.backend,
                VALID_STORAGE_BACKENDS.join(", ")
            ));
        }
```

**Step 4: Add a helper to create the storage backend from config**

Add to `crates/shabka-core/src/storage/mod.rs`:

```rust
use crate::config::ShabkaConfig;
use crate::error::{Result, ShabkaError};

/// Create a storage backend from the given configuration.
pub async fn create_backend(config: &ShabkaConfig) -> Result<Box<dyn StorageBackend>> {
    match config.storage.backend.as_str() {
        "sqlite" => {
            let path = match &config.storage.path {
                Some(p) => std::path::PathBuf::from(p),
                None => default_sqlite_path()?,
            };
            let storage = SqliteStorage::open(&path)?;
            Ok(Box::new(storage))
        }
        "helix" => {
            let storage = HelixStorage::new(
                Some(&config.helix.url),
                Some(config.helix.port),
                config.helix.api_key.as_deref(),
            );
            Ok(Box::new(storage))
        }
        other => Err(ShabkaError::Config(format!(
            "unknown storage backend: {other}"
        ))),
    }
}

/// Default SQLite path: `~/.config/shabka/shabka.db`
fn default_sqlite_path() -> Result<std::path::PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("shabka").join("shabka.db"))
        .ok_or_else(|| ShabkaError::Config("cannot determine config directory".to_string()))
}
```

Note: this requires making `StorageBackend` object-safe. Since the trait uses `impl Future` return types (RPITIT), it's already object-safe in Rust 1.75+. However, if there are issues, we may need to use `#[async_trait]` or `Box<dyn Future>`. The implementer should check and adapt.

**Alternative if RPITIT doesn't work with dyn dispatch:** Use an enum wrapper instead:

```rust
pub enum Storage {
    Sqlite(SqliteStorage),
    Helix(HelixStorage),
}
```

And implement `StorageBackend` on the enum by delegating. This avoids object safety issues entirely and has zero overhead. The implementer should choose whichever approach compiles.

**Step 5: Run tests**

Run: `cargo test -p shabka-core --no-default-features -- config::tests --nocapture`
Expected: All config tests pass (including 5 new storage tests)

Run: `cargo check -p shabka-core --no-default-features`
Expected: Compiles

**Step 6: Commit**

```bash
git add crates/shabka-core/src/config/mod.rs crates/shabka-core/src/storage/mod.rs
git commit -m "feat(config): add StorageConfig with sqlite/helix backend selection"
```

---

### Task 8: Wire up consumers (MCP, Web, CLI, Hooks)

**Files:**
- Modify: `crates/shabka-mcp/src/main.rs` (or wherever HelixStorage is instantiated)
- Modify: `crates/shabka-web/src/main.rs`
- Modify: `crates/shabka-cli/src/main.rs`
- Modify: `crates/shabka-hooks/src/lib.rs` (or handlers.rs)

This task depends heavily on how each consumer currently creates `HelixStorage`. The implementer should:

1. **Find all `HelixStorage::new()` call sites** using: `grep -rn "HelixStorage::new" crates/`
2. **Replace each with `storage::create_backend(&config).await?`** (or the enum approach)
3. **Update type signatures** from `HelixStorage` to `Box<dyn StorageBackend>` (or `Storage` enum)

**Key pattern:**

Before:
```rust
let storage = HelixStorage::new(
    Some(&config.helix.url),
    Some(config.helix.port),
    config.helix.api_key.as_deref(),
);
```

After:
```rust
let storage = shabka_core::storage::create_backend(&config).await?;
```

**Step 1: Search for all HelixStorage usage**

Run: `grep -rn "HelixStorage" crates/`

**Step 2: Update each consumer**

Follow the pattern above. Each crate may need `rusqlite` added as a transitive dependency if it references `SqliteStorage` directly (it shouldn't — it should go through the trait).

**Step 3: Verify everything compiles**

Run: `cargo check --workspace --no-default-features`

**Step 4: Run all tests**

Run: `cargo test -p shabka-core --no-default-features`

**Step 5: Commit**

```bash
git add crates/
git commit -m "refactor: wire up configurable storage backend across all consumers"
```

---

### Task 9: Update `shabka init` to default to SQLite

**Files:**
- Modify: `crates/shabka-cli/src/commands/init.rs` (or equivalent)

The init command currently generates a config.toml scaffold. Update it to:

1. Default `[storage]` section to `backend = "sqlite"`
2. Include commented-out `[storage.helix]` section for users who want to upgrade
3. The default config template should look like:

```toml
[storage]
backend = "sqlite"
# path = "~/.config/shabka/shabka.db"  # default

# Uncomment to use HelixDB instead:
# [storage]
# backend = "helix"
# [storage.helix]
# url = "http://localhost"
# port = 6969
```

**Step 1: Find and update the init command**

Run: `grep -rn "config.toml" crates/shabka-cli/src/`

**Step 2: Update the template string**

Add the `[storage]` section at the top of the generated config template.

**Step 3: Test manually**

Run: `cargo run -p shabka-cli --no-default-features -- init --check` (in a temp dir)

**Step 4: Commit**

```bash
git add crates/shabka-cli/
git commit -m "feat(cli): default shabka init to SQLite storage backend"
```

---

### Task 10: Full test suite and cleanup

**Step 1: Run the full test suite**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All existing tests + new SQLite tests pass

Run: `cargo clippy --workspace --no-default-features -- -D warnings`
Expected: No warnings

**Step 2: Run `just check`**

Run: `just check`
Expected: Clean

**Step 3: Update MEMORY.md**

Update the auto-memory to reflect:
- SQLite is now the default storage backend
- `rusqlite` 0.34 added as dependency
- `StorageConfig` added to config
- `SqliteStorage` implements all 12 `StorageBackend` methods
- Update test count

**Step 4: Final commit**

```bash
git add -A
git commit -m "feat(storage): SQLite storage mode — zero-dependency default backend

- Add SqliteStorage implementing all 12 StorageBackend trait methods
- Brute-force cosine similarity for vector search
- WAL mode for concurrent reads
- StorageConfig with backend selection (sqlite/helix)
- Default to sqlite for new installs
- All existing tests pass, N new SQLite tests"
```

---

## Implementation Notes for the Engineer

### Rust/SQLite bridge pattern
All `StorageBackend` methods are async, but `rusqlite` is sync. The `with_conn` helper wraps all DB access in `tokio::task::spawn_blocking` with a `Mutex<Connection>`. This is the standard pattern for rusqlite + tokio.

### Enum serialization gotcha
Simple enums like `MemoryKind::Observation` serialize to `"observation"` (with serde rename_all), but tagged enums like `MemorySource::AutoCapture { hook }` serialize to `{"auto_capture":{"hook":"..."}}`. The `row_to_memory` function handles both by using `serde_json::from_str` with the raw JSON string for tagged enums, and wrapping in quotes for simple enums.

### Object safety
The `StorageBackend` trait uses RPITIT (return position impl trait in trait). This is object-safe in Rust 1.75+ but may not work with `dyn` dispatch. If `Box<dyn StorageBackend>` doesn't compile, use a `Storage` enum instead — it's zero-cost and avoids the issue entirely.

### WAL mode
`PRAGMA journal_mode=WAL` is set on open for file-based databases. This allows concurrent readers (web dashboard) while the writer (hooks) is active. In-memory databases don't use WAL.

### CASCADE deletes
`ON DELETE CASCADE` on the `embeddings` and `relations` tables means deleting a memory automatically cleans up its embedding and all connected edges. This is handled by SQLite when `PRAGMA foreign_keys=ON` is set.
