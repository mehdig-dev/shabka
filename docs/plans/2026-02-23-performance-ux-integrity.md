# Performance, UX & Integrity Improvements

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Push timeline filtering to the storage layer for 10x faster pagination, complete bulk operations UI, add TUI create/edit, add `shabka check` integrity command, and add request logging middleware.

**Architecture:** Five independent improvements. Task 1 modifies the core `TimelineQuery` + SQLite backend + web handlers. Tasks 2-5 are leaf changes with no cross-dependencies.

**Tech Stack:** Rust, SQLite, Axum, tower-http (TraceLayer), ratatui, clap

---

## Task 1: Push filtering to storage layer (pagination performance)

**Problem:** Web list and analytics pages fetch 10,000 rows via `TimelineQuery { limit: 10000 }`, then filter in Rust. With thousands of memories this is O(n) per page load.

**Fix:** Add `offset` and `privacy` fields to `TimelineQuery`, wire them into SQLite's `timeline()`, and update web handlers to pass filters down.

**Files:**
- Modify: `crates/shabka-core/src/model/memory.rs` — add `offset`, `privacy` to `TimelineQuery`
- Modify: `crates/shabka-core/src/storage/sqlite.rs` — handle `offset` and `privacy` in `timeline()`
- Modify: `crates/shabka-core/src/storage/helix.rs` — handle `offset` (pass-through; Helix doesn't support privacy filter)
- Modify: `crates/shabka-web/src/routes/memories.rs` — pass filters into TimelineQuery instead of post-filtering
- Modify: `crates/shabka-web/src/routes/analytics.rs` — pass status filter for active-only queries
- Test: existing tests in `sqlite.rs` + new tests for offset/privacy

### Step 1: Add fields to TimelineQuery

In `crates/shabka-core/src/model/memory.rs`, add to `TimelineQuery`:

```rust
#[serde(default)]
pub offset: usize,
#[serde(default)]
pub privacy: Option<MemoryPrivacy>,
#[serde(default)]
pub created_by: Option<String>,
```

And in the `Default` impl, add:
```rust
offset: 0,
privacy: None,
created_by: None,
```

Also add a `count` field for getting total count without fetching all rows:
```rust
/// When true, the timeline response represents a count query.
/// The `limit` is ignored for counting; all matching rows are counted.
#[serde(default)]
pub count_only: bool,
```

### Step 2: Update SQLite timeline() to handle new fields

In `crates/shabka-core/src/storage/sqlite.rs`, in the `timeline()` method:

Add `privacy` filter:
```rust
if let Some(ref privacy) = query.privacy {
    conditions.push(format!("m.privacy = ?{idx}"));
    params.push(Box::new(privacy_to_str(privacy)));
    idx += 1;
}
```

Add `created_by` filter:
```rust
if let Some(ref created_by) = query.created_by {
    conditions.push(format!("m.created_by = ?{idx}"));
    params.push(Box::new(created_by.clone()));
    idx += 1;
}
```

Add `OFFSET` to the SQL:
```rust
let sql = format!(
    "SELECT m.*,
        (SELECT COUNT(*) FROM relations r WHERE r.source_id = m.id) as related_count
     FROM memories m
     {where_clause}
     ORDER BY m.created_at DESC
     LIMIT ?{idx} OFFSET ?{next_idx}"
);
params.push(Box::new(query.limit as i64));
params.push(Box::new(query.offset as i64));
```

Add a `count_only` branch that returns a single count entry instead of full rows.

### Step 3: Add new tests for offset and privacy filtering

```rust
#[tokio::test]
async fn test_timeline_with_offset() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    // Insert 5 memories, query with limit=2 offset=2, verify we get items 3-4
}

#[tokio::test]
async fn test_timeline_with_privacy_filter() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    // Insert memories with different privacy levels, filter by Private
}

#[tokio::test]
async fn test_timeline_count_only() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    // Insert 5 memories, count_only=true, verify count
}
```

### Step 4: Update web list handler

In `crates/shabka-web/src/routes/memories.rs`, replace the 10k-fetch-then-filter pattern:

```rust
async fn list_memories(...) -> Result<Html<String>, AppError> {
    let filter_kind = params.kind.unwrap_or_default();
    let filter_project = params.project.unwrap_or_default();

    // Build a targeted query with filters pushed to DB
    let kind_filter = if !filter_kind.is_empty() {
        filter_kind.parse::<MemoryKind>().ok()
    } else {
        None
    };

    // First: get total count for pagination
    let count_query = TimelineQuery {
        limit: 0, // ignored for count
        kind: kind_filter.clone(),
        project_id: if filter_project.is_empty() { None } else { Some(filter_project.clone()) },
        status: Some(MemoryStatus::Active),
        count_only: true,
        ..Default::default()
    };
    let count_entries = state.storage.timeline(&count_query).await?;
    let total_count = count_entries.len(); // or use a dedicated count method

    // Then: fetch only the page we need
    let page = params.page.unwrap_or(1).max(1);
    let total_pages = if total_count == 0 { 1 } else { total_count.div_ceil(PAGE_SIZE) };
    let page = page.min(total_pages);

    let page_query = TimelineQuery {
        limit: PAGE_SIZE,
        offset: (page - 1) * PAGE_SIZE,
        kind: kind_filter,
        project_id: if filter_project.is_empty() { None } else { Some(filter_project.clone()) },
        status: Some(MemoryStatus::Active),
        ..Default::default()
    };
    let page_entries = state.storage.timeline(&page_query).await?;
    // ... rest of handler
}
```

### Step 5: Update analytics to use targeted queries where possible

The analytics page needs all entries for aggregation, so the 10k pattern is acceptable there. But push `status: Some(Active)` into the query to skip archived memories where appropriate.

### Step 6: Run tests and commit

```bash
cargo test --workspace --no-default-features
cargo clippy --workspace --no-default-features -- -D warnings
```

Commit: `perf: push timeline filtering to storage layer with offset pagination`

---

## Task 2: Complete web bulk operations

**Problem:** Bulk archive/delete API endpoints exist, HTML checkboxes + floating bar exist, but need verification the JS wiring works end-to-end.

**Files:**
- Modify: `crates/shabka-web/templates/memories/list.html` — verify/fix bulk bar visibility toggle
- Test: manual E2E in browser

### Step 1: Verify bulk bar JavaScript

Read `list.html` lines 100-174. The bulk bar should:
- Show when any checkbox is checked (display: flex)
- Hide when no checkboxes are checked (display: none)
- Update count text
- Call `/api/v1/memories/bulk/archive` and `/api/v1/memories/bulk/delete`

Check if there's an event listener wiring the checkbox `onchange` to toggling the bar. The checkboxes have `onclick="event.stopPropagation()"` but may be missing an `onchange` handler to show/hide the bar.

### Step 2: Fix checkbox → bar toggle if missing

Add event delegation for `.bulk-select` checkboxes:
```javascript
document.querySelectorAll('.bulk-select').forEach(cb => {
    cb.addEventListener('change', updateBulkBar);
});

function updateBulkBar() {
    const checked = document.querySelectorAll('.bulk-select:checked');
    const bar = document.getElementById('bulk-bar');
    const count = document.getElementById('bulk-count');
    if (checked.length > 0) {
        bar.style.display = 'flex';
        count.textContent = checked.length + ' selected';
    } else {
        bar.style.display = 'none';
    }
}
```

### Step 3: Verify API endpoints return correct responses

Add a web test in `crates/shabka-web/src/routes/api.rs` tests:

```rust
#[tokio::test]
async fn test_bulk_archive() {
    // Create 3 memories, bulk archive 2, verify status changed
}

#[tokio::test]
async fn test_bulk_delete() {
    // Create 3 memories, bulk delete 2, verify 1 remains
}
```

### Step 4: Run tests and commit

```bash
cargo test -p shabka-web --no-default-features
```

Commit: `fix(web): wire bulk operations checkbox toggle and add tests`

---

## Task 3: TUI create and edit

**Problem:** TUI is read-only — users can browse but not create or edit memories.

**Files:**
- Modify: `crates/shabka-cli/src/tui/mod.rs` — add `Screen::Create`, input fields, save action

### Step 1: Add Create screen and input state

Add to `Screen` enum:
```rust
pub enum Screen {
    List,
    Detail,
    Status,
    Create,
}
```

Add create form state to `App`:
```rust
// Create form state
pub create_title: String,
pub create_content: String,
pub create_kind_index: usize,
pub create_field: usize, // 0=title, 1=content, 2=kind
pub create_cursor: usize,
```

### Step 2: Add key handler for Create screen

- `n` key in List Normal mode → switch to Create screen
- In Create screen:
  - Tab → cycle between fields (title/content/kind)
  - Enter on kind field → cycle kind values
  - Ctrl+S → save memory
  - Esc → cancel, back to list
  - Normal text input for title/content fields

### Step 3: Add AsyncAction::SaveMemory

```rust
AsyncAction::SaveMemory { title, content, kind } => {
    let memory = Memory::builder()
        .title(title)
        .content(content)
        .kind(kind)
        .created_by(user_id.clone())
        .build();
    storage.save_memory(&memory, None).await?;
    // Optionally embed if embedding service available
    AsyncResult::MemorySaved
}
```

### Step 4: Add render function for Create screen

Render a simple form with:
- Title input (highlighted when active)
- Content input (multi-line, highlighted when active)
- Kind selector (cycle with Enter)
- Footer: `Tab: next field | Ctrl+S: save | Esc: cancel`

### Step 5: Add edit mode from Detail screen

- `e` key in Detail Normal mode → switch to Create screen pre-filled with current memory
- Save triggers `update_memory()` instead of `save_memory()`
- Track `editing_id: Option<Uuid>` in App state

### Step 6: Run tests and commit

```bash
cargo test -p shabka-cli --no-default-features
cargo clippy --workspace --no-default-features -- -D warnings
```

Commit: `feat(tui): add create and edit memory screens`

---

## Task 4: Database integrity check (`shabka check`)

**Problem:** No way to detect orphaned embeddings, broken relation references, or other data corruption.

**Files:**
- Modify: `crates/shabka-core/src/storage/sqlite.rs` — add `integrity_check()` method
- Modify: `crates/shabka-core/src/storage/mod.rs` — expose on `Storage` enum
- Modify: `crates/shabka-cli/src/main.rs` — add `Check` command

### Step 1: Define IntegrityReport struct

In `crates/shabka-core/src/storage/sqlite.rs`:

```rust
#[derive(Debug, Default)]
pub struct IntegrityReport {
    pub total_memories: usize,
    pub total_embeddings: usize,
    pub total_relations: usize,
    pub total_sessions: usize,
    pub orphaned_embeddings: Vec<String>,    // memory_ids in embeddings but not in memories
    pub broken_relations: Vec<(String, String)>, // (source_id, target_id) where either is missing
    pub missing_embeddings: usize,           // memories without embeddings
    pub sqlite_integrity_ok: bool,           // PRAGMA integrity_check result
}
```

### Step 2: Implement integrity_check()

```rust
pub fn integrity_check(&self) -> Result<IntegrityReport> {
    let conn = self.conn.lock()...;

    let mut report = IntegrityReport::default();

    // Counts
    report.total_memories = conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?;
    report.total_embeddings = conn.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))?;
    report.total_relations = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0))?;
    report.total_sessions = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;

    // Orphaned embeddings (in embeddings but not in memories)
    let mut stmt = conn.prepare(
        "SELECT e.memory_id FROM embeddings e
         LEFT JOIN memories m ON m.id = e.memory_id
         WHERE m.id IS NULL"
    )?;
    report.orphaned_embeddings = stmt.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();

    // Broken relations
    let mut stmt = conn.prepare(
        "SELECT r.source_id, r.target_id FROM relations r
         LEFT JOIN memories m1 ON m1.id = r.source_id
         LEFT JOIN memories m2 ON m2.id = r.target_id
         WHERE m1.id IS NULL OR m2.id IS NULL"
    )?;
    report.broken_relations = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.filter_map(|r| r.ok()).collect();

    // Missing embeddings
    report.missing_embeddings = conn.query_row(
        "SELECT COUNT(*) FROM memories m
         LEFT JOIN embeddings e ON e.memory_id = m.id
         WHERE e.memory_id IS NULL", [], |r| r.get(0))?;

    // SQLite integrity check
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    report.sqlite_integrity_ok = integrity == "ok";

    Ok(report)
}
```

### Step 3: Expose on Storage enum

In `crates/shabka-core/src/storage/mod.rs`:
```rust
pub fn integrity_check(&self) -> Option<IntegrityReport> {
    match self {
        Storage::Sqlite(s) => s.integrity_check().ok(),
        Storage::Helix(_) => None,
    }
}
```

### Step 4: Add `shabka check` CLI command

```rust
/// Check database integrity
Check {
    /// Auto-repair: remove orphaned embeddings and broken relations
    #[arg(long)]
    repair: bool,
},
```

Handler prints a report:
```
Database Integrity Check
  Memories:    142
  Embeddings:  140 (2 missing)
  Relations:   87
  Sessions:    15
  SQLite:      ok

  Issues:
    0 orphaned embeddings
    0 broken relations
    2 memories without embeddings (run `shabka reembed`)

  Result: PASS ✓
```

If `--repair` is passed, delete orphaned embeddings and broken relations.

### Step 5: Add tests

```rust
#[test]
fn test_integrity_check_clean_db() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let report = storage.integrity_check().unwrap();
    assert!(report.sqlite_integrity_ok);
    assert!(report.orphaned_embeddings.is_empty());
    assert!(report.broken_relations.is_empty());
}

#[tokio::test]
async fn test_integrity_check_detects_orphaned_embedding() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    // Insert an embedding without a corresponding memory
    // Verify integrity_check catches it
}
```

### Step 6: Run tests and commit

```bash
cargo test --workspace --no-default-features
```

Commit: `feat(cli): add shabka check for database integrity verification`

---

## Task 5: Request logging middleware

**Problem:** No visibility into web request timing or errors.

**Files:**
- Modify: `Cargo.toml` (workspace) — add `"trace"` feature to `tower-http`
- Modify: `crates/shabka-web/src/main.rs` — add `TraceLayer` middleware

### Step 1: Add trace feature to tower-http

In workspace `Cargo.toml`:
```toml
tower-http = { version = "0.6", features = ["cors", "fs", "trace"] }
```

### Step 2: Add TraceLayer to router

In `crates/shabka-web/src/main.rs`:

```rust
use tower_http::trace::TraceLayer;

let app = routes::router()
    .with_state(state)
    .nest_service("/mcp", mcp_service)
    .layer(tower_http::cors::CorsLayer::permissive())
    .layer(TraceLayer::new_for_http());
```

This logs: `INFO request{method=GET uri=/memories} ... latency=12ms status=200`

### Step 3: Run and verify

```bash
just web
# In another terminal:
curl http://localhost:37737/health
# Check stderr for trace output
```

### Step 4: Commit

```bash
cargo clippy --workspace --no-default-features -- -D warnings
```

Commit: `feat(web): add request logging with tower-http TraceLayer`

---

## Execution Order

Tasks are independent, but recommended order for smooth integration:

1. **Task 1** (storage-layer filtering) — core change, most impactful
2. **Task 5** (request logging) — 5-minute change, immediate value
3. **Task 4** (integrity check) — useful for validating Task 1 didn't break anything
4. **Task 2** (bulk operations) — web UX polish
5. **Task 3** (TUI create/edit) — biggest new feature, least coupled

## Test Verification

After all tasks:
```bash
cargo clippy --workspace --no-default-features -- -D warnings
cargo test --workspace --no-default-features
shabka check
shabka --version
```
