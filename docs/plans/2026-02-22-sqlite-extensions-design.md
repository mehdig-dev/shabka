# SQLite Extensions: sqlite-vec + sqlean

**Date:** 2026-02-22
**Status:** Approved

## Summary

Add sqlite-vec and sqlean (fuzzy, stats, crypto) as SQLite extensions to shabka-core. sqlite-vec replaces the hand-rolled brute-force cosine similarity search with SIMD-accelerated in-database vector search. sqlean extensions add fuzzy string matching, statistical aggregations, and content hashing directly in SQL.

## Motivation

The current `vector_search()` implementation in `sqlite.rs` loads ALL embeddings from the database into Rust, deserializes every BLOB to `Vec<f32>`, computes cosine similarity in a scalar loop, sorts, and truncates. This works at current scale (hundreds of memories) but:

1. Transfers all vector data across the SQLite/Rust boundary on every search
2. Uses non-SIMD scalar math for distance computation
3. Cannot filter by metadata during vector search (only post-filter)
4. Scales linearly with total embedding count regardless of query

## Architecture

```
┌──────────────────────────────────────┐
│         rusqlite Connection           │
│  (existing Arc<Mutex<Connection>>)    │
├──────────────────────────────────────┤
│  sqlite-vec    │  sqlean extensions   │
│  ─────────────│──────────────────────│
│  vec0 virtual │  fuzzy_jarowin()     │
│  table for    │  fuzzy_leven()       │
│  KNN search   │  stats_median()      │
│               │  crypto_sha256()     │
├──────────────────────────────────────┤
│      bundled SQLite (rusqlite)        │
│  memories │ relations │ sessions      │
└──────────────────────────────────────┘
```

All extensions load into the same rusqlite connection via `sqlite3_auto_extension`. No new processes, no new storage engines. The existing `SqliteStorage` struct gains new capabilities without changing its public API.

## Schema Changes

New virtual table alongside existing tables:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories USING vec0(
    memory_id TEXT PRIMARY KEY,
    embedding float[{dimensions}],
    project_id TEXT PARTITION KEY,
    +kind TEXT,
    +importance REAL
);
```

- `memory_id` — foreign key to `memories.id`
- `embedding` — vector column, dimension set at table creation from config
- `project_id` — partition key for fast per-project search
- `+kind`, `+importance` — auxiliary columns (stored, not indexed)

## Vector Search: Before and After

### Before (sqlite.rs:527-590)

```rust
// 1. Load ALL embeddings
let rows = stmt.query_map([], |row| { ... })?;
// 2. Deserialize every BLOB
let embedding: Vec<f32> = bytes_to_f32_vec(&blob);
// 3. Cosine similarity in Rust loop
let score = cosine_similarity(&query, &embedding);
// 4. Sort + truncate
results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
results.truncate(limit);
// 5. Batch fetch Memory records
```

### After

```sql
SELECT m.*, vec_distance_cosine(v.embedding, ?1) AS distance
FROM vec_memories AS v
JOIN memories AS m ON m.id = v.memory_id
WHERE v.embedding MATCH ?1
  AND v.k = ?2
ORDER BY distance
```

One query. SIMD-accelerated. Metadata via JOIN.

## sqlean Extensions

### fuzzy (fuzzy string matching)

| Function | Use Case |
|----------|----------|
| `fuzzy_jarowin(a, b)` | Dedup scoring — title similarity between candidate and existing memories |
| `fuzzy_leven(a, b)` | Edit distance for typo-tolerant search |
| `fuzzy_damlev(a, b)` | Damerau-Levenshtein for transposition-aware matching |

### stats (statistical aggregations)

| Function | Use Case |
|----------|----------|
| `stats_median(col)` | Analytics dashboard — median importance, median age |
| `stats_p95(col)` | Performance percentiles |
| `stats_stddev(col)` | Distribution analysis |

### crypto (content hashing)

| Function | Use Case |
|----------|----------|
| `crypto_sha256(content)` | Content fingerprinting for fast dedup detection |

## Build Integration

### Dependencies

```toml
# crates/shabka-core/Cargo.toml
[dependencies]
sqlite-vec = "0.1.7-alpha"
zerocopy = { version = "0.8", features = ["derive"] }

[build-dependencies]
cc = "1"
```

### sqlean vendoring

Vendor only the 3 needed extensions under `crates/shabka-core/vendor/sqlean/`:

```
vendor/sqlean/
├── src/
│   ├── sqlite3-fuzzy.c
│   ├── sqlite3-stats.c
│   ├── sqlite3-crypto.c
│   ├── fuzzy/
│   ├── stats/
│   └── crypto/
```

Compiled via `cc` crate in `build.rs`, linked statically. Each extension is ~2-5 C files, no external dependencies.

## Extension Registration

```rust
pub fn register_extensions() {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(
            std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ())
        ));
        rusqlite::ffi::sqlite3_auto_extension(Some(
            std::mem::transmute(sqlite3_fuzzy_init as *const ())
        ));
        rusqlite::ffi::sqlite3_auto_extension(Some(
            std::mem::transmute(sqlite3_stats_init as *const ())
        ));
        rusqlite::ffi::sqlite3_auto_extension(Some(
            std::mem::transmute(sqlite3_crypto_init as *const ())
        ));
    }
}
```

Called once at startup before opening any connections.

## Migration Strategy

1. On first open with new code, detect if `vec_memories` table exists
2. If not, create it with dimension from embedding config
3. Populate `vec_memories` from existing `embeddings` table
4. Keep `embeddings` table as fallback (drop in future version)
5. If dimension config changes (provider switch), drop and recreate `vec_memories`

## What Doesn't Change

- `StorageBackend` trait — same 12 methods, same signatures
- `Storage` enum — still dispatches Sqlite/Helix
- Relations, sessions, timeline — all unchanged
- HelixDB backend — completely unaffected
- Config, embedding providers — unchanged
- CLI, MCP, hooks — no changes needed

## Platform Compatibility

| Platform | sqlite-vec | sqlean | Notes |
|----------|-----------|--------|-------|
| Linux x86_64 | AVX2 SIMD | Works | Primary target |
| Linux ARM64 | NEON SIMD | Works | Cross-compile friendly |
| macOS (Intel) | AVX2 SIMD | Works | |
| macOS (Apple Silicon) | NEON SIMD | Works | |
| Windows x86_64 | SIMD | Works | |
| WSL2 (Ubuntu 20.04) | Works | Works | No SIGILL issues (pure C, no AVX-512) |

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| sqlite-vec is pre-v1 (alpha) | API surface we use is small and stable; vec0 schema and MATCH syntax are core to the project |
| sqlean C source vendoring | Pin to specific commit; only 3 extensions, minimal surface |
| Dimension mismatch on provider change | Detect via config comparison, recreate vec_memories table |
| vec0 doesn't support cosine MATCH directly | Normalize vectors before insertion (L2 on normalized = cosine), or use `vec_distance_cosine()` scalar function |
