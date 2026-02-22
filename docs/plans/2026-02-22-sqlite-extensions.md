# SQLite Extensions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace brute-force vector search with sqlite-vec and add sqlean (fuzzy, stats, crypto) extensions to shabka-core's SQLite storage.

**Architecture:** All extensions load into the existing rusqlite connection via `sqlite3_auto_extension`. sqlite-vec provides a `vec0` virtual table for SIMD-accelerated KNN search. sqlean's fuzzy/stats/crypto C sources are vendored and compiled via `cc` in `build.rs`. The `StorageBackend` trait and public API remain unchanged.

**Tech Stack:** rusqlite (bundled), sqlite-vec crate, sqlean C sources (vendored), cc build crate, zerocopy for zero-copy vector passing.

---

### Task 1: Add sqlite-vec dependency and register extension

**Files:**
- Modify: `crates/shabka-core/Cargo.toml`
- Modify: `crates/shabka-core/src/storage/sqlite.rs:28-34` (open function)
- Modify: `crates/shabka-core/src/storage/sqlite.rs:38-51` (open_in_memory function)

**Step 1: Add dependencies to Cargo.toml**

Add after line 30 (`openssl` line):

```toml
sqlite-vec = "0.1.7-alpha"
```

**Step 2: Register sqlite-vec extension in SqliteStorage**

Add a `register_extensions()` function and call it from both `open()` and `open_in_memory()`. Add to `sqlite.rs` after the imports (around line 12):

```rust
use std::sync::Once;

static EXTENSIONS_REGISTERED: Once = Once::new();

fn register_extensions() {
    EXTENSIONS_REGISTERED.call_once(|| {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
    });
}
```

Call `register_extensions()` at the top of both `open()` (line 29) and `open_in_memory()` (line 39).

**Step 3: Write test to verify extension loads**

Add to the test module:

```rust
#[test]
fn sqlite_vec_extension_loaded() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let conn = storage.conn.lock().unwrap();
    let version: String = conn
        .query_row("SELECT vec_version()", [], |row| row.get(0))
        .unwrap();
    assert!(!version.is_empty(), "sqlite-vec should report a version");
}
```

**Step 4: Run tests**

Run: `cargo test -p shabka-core --no-default-features -- sqlite_vec_extension_loaded`
Expected: PASS

**Step 5: Run full test suite to verify no regressions**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All 290+ tests pass

**Step 6: Commit**

```bash
git add crates/shabka-core/Cargo.toml crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): add sqlite-vec extension registration"
```

---

### Task 2: Create vec_memories virtual table and migration

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs:72-136` (create_tables)

**Step 1: Write test for vec_memories table creation**

Add to the test module:

```rust
#[test]
fn vec_memories_table_created() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let conn = storage.conn.lock().unwrap();
    // vec0 virtual tables show up in sqlite_master
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='vec_memories'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "vec_memories virtual table should exist");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p shabka-core --no-default-features -- vec_memories_table_created`
Expected: FAIL — table doesn't exist yet

**Step 3: Add vec_memories creation to create_tables()**

In `create_tables()` (line 72), add after the existing `CREATE INDEX` statements (before the closing `"` on line 131):

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories USING vec0(
    memory_id TEXT PRIMARY KEY,
    embedding float[128]
);
```

Note: We use 128 as the default dimension (hash provider). The dimension will be handled dynamically in Task 4.

**Step 4: Run test to verify it passes**

Run: `cargo test -p shabka-core --no-default-features -- vec_memories_table_created`
Expected: PASS

**Step 5: Update the open_in_memory_creates_tables test**

The existing test at line 938 checks for table names. Update the assertion to also expect `vec_memories`:

```rust
assert!(tables.contains(&"vec_memories".to_string()));
```

**Step 6: Run full test suite**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All tests pass

**Step 7: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): create vec_memories virtual table"
```

---

### Task 3: Dual-write embeddings to vec_memories on save_memory

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs:340-349` (save_memory embedding insert)

**Step 1: Write test for dual-write**

Add to the test module:

```rust
#[tokio::test]
async fn test_save_memory_writes_to_vec_memories() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let mem = test_memory();
    let emb = vec![1.0_f32, 0.0, 0.0];
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p shabka-core --no-default-features -- test_save_memory_writes_to_vec_memories`
Expected: FAIL — count is 0

**Step 3: Add dual-write to save_memory**

In `save_memory()` at line 340, after the existing embedding insert into `embeddings` table (line 344-348), add:

```rust
// Also insert into vec_memories for sqlite-vec search
let blob: Vec<u8> = emb.iter().flat_map(|f| f.to_le_bytes()).collect();
tx.execute(
    "INSERT INTO vec_memories (memory_id, embedding) VALUES (?1, ?2)",
    params![memory.id.to_string(), blob],
)
.map_err(|e| ShabkaError::Storage(format!("failed to insert vec embedding: {e}")))?;
```

Note: The `blob` variable is already computed on line 343, so reuse it. The insertion uses the same little-endian bytes format that sqlite-vec expects for float vectors.

**Step 4: Run test to verify it passes**

Run: `cargo test -p shabka-core --no-default-features -- test_save_memory_writes_to_vec_memories`
Expected: PASS

**Step 5: Also handle save_memory with no embedding**

Add test:

```rust
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
```

This should already pass since the insert is inside the `if let Some(emb)` block.

**Step 6: Run full test suite**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All tests pass

**Step 7: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): dual-write embeddings to vec_memories"
```

---

### Task 4: Replace brute-force vector_search with sqlite-vec KNN

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs:527-614` (vector_search method)

**Step 1: Rewrite vector_search to use vec_memories**

Replace the entire `vector_search` method (lines 527-614) with:

```rust
async fn vector_search(&self, embedding: &[f32], limit: usize) -> Result<Vec<(Memory, f32)>> {
    let query_vec = embedding.to_vec();

    self.with_conn(move |conn| {
        // Serialize query vector to little-endian bytes for sqlite-vec
        let query_blob: Vec<u8> = query_vec.iter().flat_map(|f| f.to_le_bytes()).collect();

        // KNN search via vec_memories, JOIN with memories for full records
        let sql = "
            SELECT m.*, v.distance
            FROM vec_memories AS v
            JOIN memories AS m ON m.id = v.memory_id
            WHERE v.embedding MATCH ?1
              AND v.k = ?2
            ORDER BY v.distance
        ";

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| ShabkaError::Storage(format!("failed to prepare vec search: {e}")))?;

        let rows = stmt
            .query_map(params![query_blob, limit as i64], |row| {
                let mem = row_to_memory(row)?;
                let distance: f64 = row.get(18)?; // distance is the 19th column (0-indexed: 18)
                Ok((mem, distance))
            })
            .map_err(|e| ShabkaError::Storage(format!("failed to execute vec search: {e}")))?;

        let mut results = Vec::new();
        for row in rows {
            let (mem, distance) = row.map_err(|e| {
                ShabkaError::Storage(format!("failed to read vec search row: {e}"))
            })?;
            // Convert L2 distance to cosine similarity score (1.0 - distance for normalized vectors)
            // For non-normalized vectors, use 1.0 / (1.0 + distance) as a fallback
            let score = 1.0 / (1.0 + distance as f32);
            results.push((mem, score));
        }

        Ok(results)
    })
    .await
}
```

**Step 2: Run existing vector search tests**

Run: `cargo test -p shabka-core --no-default-features -- test_vector_search`
Expected: Both `test_vector_search` and `test_vector_search_no_embeddings` should pass. The score values may differ slightly from cosine similarity — adjust test assertions if needed.

**Step 3: If test assertions on exact scores fail, adjust them**

The existing test asserts `results[0].1 > 0.99` and `results[1].1 > 0.9`. With L2 distance converted via `1/(1+d)`, the values will be different. Update the test to check ordering (m1 scores higher than m2, m2 scores higher than m3) rather than exact thresholds:

```rust
assert_eq!(results.len(), 2);
assert_eq!(results[0].0.title, "Rust patterns");
assert_eq!(results[1].0.title, "Rust lifetimes");
// m1 should score higher than m2 (closer to query)
assert!(results[0].1 > results[1].1);
```

**Step 4: Run full test suite**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All tests pass

**Step 5: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): replace brute-force search with sqlite-vec KNN"
```

---

### Task 5: Handle delete_memory cleanup in vec_memories

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs` (delete_memory method)

**Step 1: Write test for vec_memories cleanup on delete**

```rust
#[tokio::test]
async fn test_delete_memory_removes_vec_embedding() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let mem = test_memory();
    let emb = vec![1.0_f32, 0.0, 0.0];
    storage.save_memory(&mem, Some(&emb)).await.unwrap();

    storage.delete_memory(mem.id).await.unwrap();

    let conn = storage.conn.lock().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM vec_memories", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0, "vec_memories should be empty after delete");
}
```

**Step 2: Run test to check if it already passes**

Run: `cargo test -p shabka-core --no-default-features -- test_delete_memory_removes_vec_embedding`

vec0 tables do NOT cascade from the memories table foreign key. If the test fails, add an explicit DELETE.

**Step 3: Add vec_memories cleanup to delete_memory if needed**

Find the `delete_memory` method and add before the existing DELETE:

```rust
conn.execute(
    "DELETE FROM vec_memories WHERE memory_id = ?1",
    params![id.to_string()],
)
.map_err(|e| ShabkaError::Storage(format!("failed to delete vec embedding: {e}")))?;
```

**Step 4: Run full test suite**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All tests pass

**Step 5: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): clean up vec_memories on memory delete"
```

---

### Task 6: Vendor sqlean fuzzy, stats, crypto extensions

**Files:**
- Create: `crates/shabka-core/build.rs`
- Create: `crates/shabka-core/vendor/sqlean/` (vendored C sources)
- Modify: `crates/shabka-core/Cargo.toml` (add cc build-dependency)

**Step 1: Add cc to build-dependencies**

Add to `crates/shabka-core/Cargo.toml`:

```toml
[build-dependencies]
cc = "1"
```

**Step 2: Vendor sqlean source files**

Clone sqlean and copy only the needed extensions:

```bash
cd /tmp && git clone --depth 1 https://github.com/nalgeon/sqlean.git
mkdir -p crates/shabka-core/vendor/sqlean/src
cp -r /tmp/sqlean/src/fuzzy crates/shabka-core/vendor/sqlean/src/
cp -r /tmp/sqlean/src/stats crates/shabka-core/vendor/sqlean/src/
cp -r /tmp/sqlean/src/crypto crates/shabka-core/vendor/sqlean/src/
cp /tmp/sqlean/src/sqlite3-fuzzy.c crates/shabka-core/vendor/sqlean/src/
cp /tmp/sqlean/src/sqlite3-stats.c crates/shabka-core/vendor/sqlean/src/
cp /tmp/sqlean/src/sqlite3-crypto.c crates/shabka-core/vendor/sqlean/src/
cp /tmp/sqlean/src/sqlean.h crates/shabka-core/vendor/sqlean/src/
rm -rf /tmp/sqlean
```

**Step 3: Create build.rs**

Create `crates/shabka-core/build.rs`:

```rust
fn main() {
    let sqlean = "vendor/sqlean/src";

    cc::Build::new()
        .file(format!("{sqlean}/sqlite3-fuzzy.c"))
        .file(format!("{sqlean}/sqlite3-stats.c"))
        .file(format!("{sqlean}/sqlite3-crypto.c"))
        .include(sqlean)
        .define("SQLITE_CORE", None)
        .warnings(false)
        .compile("sqlean_extensions");
}
```

**Step 4: Verify it compiles**

Run: `cargo build -p shabka-core --no-default-features`
Expected: Compiles without errors. The `cc` crate compiles the C files and links them.

**Step 5: Commit**

```bash
git add crates/shabka-core/build.rs crates/shabka-core/Cargo.toml crates/shabka-core/vendor/
git commit -m "build: vendor sqlean fuzzy, stats, crypto extensions"
```

---

### Task 7: Register sqlean extensions and add tests

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs` (register_extensions, extern declarations)

**Step 1: Add extern declarations for sqlean init functions**

Add near the `register_extensions()` function:

```rust
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
```

**Step 2: Register all extensions in register_extensions()**

Update `register_extensions()` to also register the sqlean extensions:

```rust
fn register_extensions() {
    EXTENSIONS_REGISTERED.call_once(|| {
        unsafe {
            // sqlite-vec: vector search
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
            // sqlean: fuzzy string matching
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_fuzzy_init as *const (),
            )));
            // sqlean: statistical aggregations
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_stats_init as *const (),
            )));
            // sqlean: cryptographic hashing
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_crypto_init as *const (),
            )));
        }
    });
}
```

**Step 3: Write tests for each extension**

```rust
#[test]
fn sqlean_fuzzy_loaded() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let conn = storage.conn.lock().unwrap();
    let score: f64 = conn
        .query_row(
            "SELECT fuzzy_damlev('kitten', 'sitting')",
            [],
            |row| row.get(0),
        )
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
        .query_row(
            "SELECT hex(crypto_sha256('hello'))",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 should produce 32 bytes (64 hex chars)");
}
```

**Step 4: Run tests**

Run: `cargo test -p shabka-core --no-default-features -- sqlean`
Expected: All 3 tests pass

**Step 5: Run full test suite**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All tests pass

**Step 6: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "feat(storage): register sqlean fuzzy, stats, crypto extensions"
```

---

### Task 8: Remove the old cosine_similarity helper and embeddings table fallback

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs`

**Step 1: Remove the cosine_similarity function**

Delete the `cosine_similarity()` helper function (lines 288-296). It's no longer called by anything after the vector_search rewrite in Task 4.

**Step 2: Verify no other code references it**

Run: `grep -r "cosine_similarity" crates/shabka-core/src/`
Expected: No results (only the deleted function used it)

**Step 3: Run full test suite**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All tests pass

**Step 4: Run clippy**

Run: `cargo clippy --workspace --no-default-features -- -D warnings`
Expected: No warnings

**Step 5: Commit**

```bash
git add crates/shabka-core/src/storage/sqlite.rs
git commit -m "refactor(storage): remove unused cosine_similarity helper"
```

---

### Task 9: Full validation

**Files:** None (verification only)

**Step 1: Run full check**

Run: `just check`
Expected: clippy + all tests pass

**Step 2: Verify test count hasn't decreased**

Run: `cargo test -p shabka-core --no-default-features 2>&1 | grep "test result"`
Expected: test count should be 290 + new tests (approximately 296+)

**Step 3: Final commit with any fixups**

If any small fixups needed, commit them:

```bash
git commit -m "chore: sqlite-vec + sqlean integration fixups"
```
