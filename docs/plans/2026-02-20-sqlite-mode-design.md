# Shabka SQLite Storage Mode — Design

**Date:** 2026-02-20
**Goal:** Add SQLite as the default storage backend, eliminating the HelixDB dependency for onboarding. HelixDB becomes an optional upgrade for power users.

---

## Motivation

HelixDB is the #1 adoption blocker for Shabka. Users must install Docker, pull the HelixDB image, run `helix push dev`, and keep it running. That's 4 steps before they can even try the tool. SQLite removes all of that — `cargo install shabka-cli && shabka init` and you're running.

## Architecture

```
StorageBackend (trait — already exists)
├── HelixStorage      (existing, HTTP → HelixDB)
└── SqliteStorage     (new, rusqlite → ~/.config/shabka/shabka.db)
```

The `StorageBackend` trait is already a clean abstraction with 12 async methods. `SqliteStorage` implements the same trait. All consumers (MCP server, web dashboard, CLI, hooks) get SQLite support for free through the trait.

## Schema

```sql
-- Core memory storage
CREATE TABLE memories (
    id TEXT PRIMARY KEY,               -- UUID as text
    kind TEXT NOT NULL,                 -- observation, decision, pattern, etc.
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '[]',    -- JSON array of strings
    source TEXT NOT NULL DEFAULT '"manual"',  -- JSON enum
    scope TEXT NOT NULL DEFAULT '"global"',   -- JSON enum
    importance REAL NOT NULL DEFAULT 0.5,
    status TEXT NOT NULL DEFAULT 'active',
    privacy TEXT NOT NULL DEFAULT 'private',
    verification TEXT NOT NULL DEFAULT 'unverified',
    project_id TEXT,
    session_id TEXT,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,          -- RFC3339
    updated_at TEXT NOT NULL,
    accessed_at TEXT NOT NULL
);

-- Embeddings stored separately (BLOB for efficiency)
CREATE TABLE embeddings (
    memory_id TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
    vector BLOB NOT NULL,              -- f32 array as raw bytes (4 * dimensions)
    dimensions INTEGER NOT NULL        -- track vector size
);

-- Graph edges between memories
CREATE TABLE relations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    target_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL DEFAULT 'related',  -- caused_by, fixes, supersedes, related, contradicts
    strength REAL NOT NULL DEFAULT 0.5,
    UNIQUE(source_id, target_id, relation_type)
);

-- Session tracking
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    summary TEXT,
    memory_count INTEGER NOT NULL DEFAULT 0
);

-- Indexes for common access patterns
CREATE INDEX idx_memories_created_at ON memories(created_at DESC);
CREATE INDEX idx_memories_project_id ON memories(project_id);
CREATE INDEX idx_memories_status ON memories(status);
CREATE INDEX idx_memories_session_id ON memories(session_id);
CREATE INDEX idx_relations_source ON relations(source_id);
CREATE INDEX idx_relations_target ON relations(target_id);
```

## Vector Search

**Approach:** Brute-force cosine similarity in Rust.

On `vector_search(embedding, limit)`:
1. Load all vectors from `embeddings` table: `SELECT memory_id, vector FROM embeddings`
2. Deserialize BLOB to `Vec<f32>` (zero-copy with bytemuck or manual)
3. Compute cosine similarity for each vector against the query
4. Sort by score descending, take top `limit`
5. Fetch full `Memory` records for the top IDs

**Performance estimate:**
- 100 memories × 768d: < 1ms
- 1,000 memories × 768d: ~5ms
- 10,000 memories × 768d: ~50ms (still acceptable for single-user)

**Future optimization path:** If performance becomes an issue at scale, swap to sqlite-vec extension or add HNSW index. The trait interface doesn't change.

## StorageBackend Method Mapping

| Trait Method | SQLite Implementation |
|---|---|
| `save_memory(memory, embedding)` | `INSERT INTO memories` + `INSERT INTO embeddings` in transaction |
| `get_memory(id)` | `SELECT * FROM memories WHERE id = ?` |
| `get_memories(ids)` | `SELECT * FROM memories WHERE id IN (...)` |
| `update_memory(id, input)` | `UPDATE memories SET ... WHERE id = ?` (native UPDATE, no delete+recreate) |
| `delete_memory(id)` | `DELETE FROM memories WHERE id = ?` (CASCADE deletes embedding + relations) |
| `vector_search(embedding, limit)` | Load all embeddings, cosine in Rust, return top-K |
| `timeline(query)` | `SELECT ... FROM memories WHERE ... ORDER BY created_at DESC LIMIT ?` with real SQL filtering |
| `add_relation(relation)` | `INSERT INTO relations` |
| `get_relations(memory_id)` | `SELECT r.*, m.* FROM relations r JOIN memories m ON r.target_id = m.id WHERE r.source_id = ?` |
| `count_relations(memory_ids)` | `SELECT source_id, COUNT(*) FROM relations WHERE source_id IN (...) GROUP BY source_id` |
| `count_contradictions(memory_ids)` | Same + `AND relation_type = 'contradicts'` |
| `save_session(session)` | `INSERT OR REPLACE INTO sessions` |
| `get_session(id)` | `SELECT * FROM sessions WHERE id = ?` |

**Improvements over HelixDB:**
- `update_memory` uses native SQL UPDATE (not delete+recreate)
- `timeline` filters in SQL (not fetch-1000-then-filter-in-Rust)
- `count_relations` is a single GROUP BY query (not N separate get_relations calls)
- Transactions for multi-step operations (save + relate)

## Configuration

```toml
# ~/.config/shabka/config.toml

[storage]
backend = "sqlite"              # "sqlite" (default) or "helix"
# path = "~/.config/shabka/shabka.db"   # optional, defaults to config dir

# Only needed when backend = "helix"
[storage.helix]
url = "http://localhost"
port = 6969
```

**Default behavior:**
- New installs default to `sqlite`
- `shabka init` creates `~/.config/shabka/shabka.db` automatically
- Existing users with `[storage.helix]` config keep working

## Dependencies

- `rusqlite` with `bundled` feature (statically links SQLite, no system dependency)
- No other new dependencies

## Migration Between Backends

No automatic migration. Users can switch via:
```bash
shabka export -o backup.json        # export from current backend
# edit config.toml: backend = "helix" (or "sqlite")
shabka import backup.json           # import into new backend
```

This already works because export/import operates through the StorageBackend trait.

## Implementation Order

1. Add `rusqlite` dependency to shabka-core
2. Implement `SqliteStorage` struct with schema creation
3. Implement all 12 `StorageBackend` methods
4. Add cosine similarity function for vector search
5. Wire up config: `backend = "sqlite"` creates `SqliteStorage`
6. Update `shabka init` to default to SQLite
7. Unit tests for SqliteStorage (same patterns as helix.rs tests)
8. Integration tests (no external service needed — they run in CI!)
9. Update docs: README, architecture, CLI
10. Publish v0.3.0

## What SQLite Doesn't Change

- Embedding service (same providers, same vectors)
- Ranking formula (same 7 signals)
- Dedup, auto-tag, consolidation (same LLM workflows)
- History audit trail (still JSONL file)
- Config format (additive, backward-compatible)
- CLI commands (all work the same)
- MCP tools (all work the same)
- Web dashboard (all work the same)
