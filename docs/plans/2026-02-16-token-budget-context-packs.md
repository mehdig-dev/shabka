# Token-Budgeted Retrieval & Context Packs — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add token-aware retrieval (`--token-budget` on search) and a `context-pack` CLI command that generates paste-ready project context within a token budget.

**Architecture:** Two features sharing a token estimation utility in `shabka-core`. Token-budgeted search adds greedy packing after existing ranking. Context packs reuse the search pipeline with a wider candidate net, then format full memory content as markdown.

**Tech Stack:** Rust, clap (CLI), serde (JSON output), existing shabka-core ranking pipeline. No new dependencies.

---

### Task 1: Token Estimation Module

**Files:**
- Create: `crates/shabka-core/src/tokens.rs`
- Modify: `crates/shabka-core/src/lib.rs:1-17`
- Test: inline `#[cfg(test)]` in `tokens.rs`

**Step 1: Write the failing tests**

```rust
// crates/shabka-core/src/tokens.rs
use crate::model::{Memory, MemoryIndex, MemoryKind};

/// Estimate token count from text using ~4 chars/token heuristic.
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// Estimate tokens for a full Memory (title + content + tags + metadata overhead).
pub fn estimate_memory_tokens(memory: &Memory) -> usize {
    estimate_tokens(&memory.title)
        + estimate_tokens(&memory.content)
        + estimate_tokens(&memory.tags.join(", "))
        + 20
}

/// Estimate tokens for a compact MemoryIndex (title + tags + metadata overhead).
pub fn estimate_index_tokens(index: &MemoryIndex) -> usize {
    estimate_tokens(&index.title)
        + estimate_tokens(&index.tags.join(", "))
        + 15
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_short() {
        // "hello" = 5 chars → (5+3)/4 = 2 tokens
        assert_eq!(estimate_tokens("hello"), 2);
    }

    #[test]
    fn test_estimate_tokens_long() {
        // 400 chars → 100 tokens
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    #[test]
    fn test_estimate_tokens_exact_multiple() {
        // 8 chars → (8+3)/4 = 2 tokens
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn test_estimate_memory_tokens() {
        let memory = Memory::new(
            "Test title".to_string(),          // 10 chars → 3 tokens
            "Some content here".to_string(),   // 17 chars → 5 tokens
            MemoryKind::Fact,
            "test".to_string(),
        )
        .with_tags(vec!["rust".to_string(), "testing".to_string()]); // "rust, testing" = 13 chars → 4 tokens

        let tokens = estimate_memory_tokens(&memory);
        // 3 (title) + 5 (content) + 4 (tags) + 20 (overhead) = 32
        assert_eq!(tokens, 32);
    }

    #[test]
    fn test_estimate_index_tokens() {
        let index = MemoryIndex {
            id: Uuid::now_v7(),
            title: "Test title".to_string(),    // 10 chars → 3 tokens
            kind: MemoryKind::Fact,
            created_at: Utc::now(),
            score: 0.9,
            tags: vec!["rust".to_string()],     // "rust" = 4 chars → 1 token
        };

        let tokens = estimate_index_tokens(&index);
        // 3 (title) + 1 (tags) + 15 (overhead) = 19
        assert_eq!(tokens, 19);
    }

    #[test]
    fn test_estimate_memory_tokens_no_tags() {
        let memory = Memory::new(
            "Title".to_string(),
            "Content".to_string(),
            MemoryKind::Fact,
            "test".to_string(),
        );
        // No tags → empty string → 0 tokens for tags
        let tokens = estimate_memory_tokens(&memory);
        assert!(tokens > 20); // At least the overhead
    }
}
```

**Step 2: Register the module**

In `crates/shabka-core/src/lib.rs`, add `pub mod tokens;` after `pub mod storage;` (line 17).

**Step 3: Run tests to verify they pass**

Run: `cargo test -p shabka-core --no-default-features -- tokens`
Expected: all 7 tests PASS

**Step 4: Commit**

```bash
git add crates/shabka-core/src/tokens.rs crates/shabka-core/src/lib.rs
git commit -m "feat: add token estimation module for budget-aware retrieval"
```

---

### Task 2: Budget Truncation in Ranking

**Files:**
- Modify: `crates/shabka-core/src/ranking.rs:1-273`
- Test: inline `#[cfg(test)]` in same file

**Step 1: Write the failing test**

Add to the `mod tests` block in `crates/shabka-core/src/ranking.rs` (after the existing `test_weights_sum_to_one` test at line 272):

```rust
#[test]
fn test_budget_truncate_fits_all() {
    let results = vec![
        MemoryIndex {
            id: uuid::Uuid::now_v7(),
            title: "Short".to_string(),
            kind: MemoryKind::Fact,
            created_at: Utc::now(),
            score: 0.9,
            tags: vec![],
        },
        MemoryIndex {
            id: uuid::Uuid::now_v7(),
            title: "Also short".to_string(),
            kind: MemoryKind::Fact,
            created_at: Utc::now(),
            score: 0.8,
            tags: vec![],
        },
    ];
    let packed = budget_truncate(results, 10000);
    assert_eq!(packed.len(), 2);
}

#[test]
fn test_budget_truncate_exceeds_budget() {
    let results = vec![
        MemoryIndex {
            id: uuid::Uuid::now_v7(),
            title: "a".repeat(100),
            kind: MemoryKind::Fact,
            created_at: Utc::now(),
            score: 0.9,
            tags: vec![],
        },
        MemoryIndex {
            id: uuid::Uuid::now_v7(),
            title: "b".repeat(100),
            kind: MemoryKind::Fact,
            created_at: Utc::now(),
            score: 0.8,
            tags: vec![],
        },
    ];
    // Each index: ~25 title tokens + 15 overhead = ~40 tokens
    // Budget of 45 should fit only the first one
    let packed = budget_truncate(results, 45);
    assert_eq!(packed.len(), 1);
    assert!(packed[0].title.starts_with('a'));
}

#[test]
fn test_budget_truncate_zero_budget() {
    let results = vec![MemoryIndex {
        id: uuid::Uuid::now_v7(),
        title: "Something".to_string(),
        kind: MemoryKind::Fact,
        created_at: Utc::now(),
        score: 0.9,
        tags: vec![],
    }];
    let packed = budget_truncate(results, 0);
    assert!(packed.is_empty());
}

#[test]
fn test_budget_truncate_empty_input() {
    let packed = budget_truncate(vec![], 1000);
    assert!(packed.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p shabka-core --no-default-features -- budget_truncate`
Expected: FAIL — `budget_truncate` not found

**Step 3: Implement `budget_truncate`**

Add to `crates/shabka-core/src/ranking.rs` before the `#[cfg(test)]` block (before line 151):

```rust
/// Greedily pack ranked results into a token budget.
/// Results must already be sorted by score (descending).
/// Stops as soon as the next result would exceed the remaining budget.
pub fn budget_truncate(results: Vec<MemoryIndex>, token_budget: usize) -> Vec<MemoryIndex> {
    use crate::tokens::estimate_index_tokens;

    let mut remaining = token_budget;
    let mut packed = Vec::new();
    for result in results {
        let cost = estimate_index_tokens(&result);
        if cost > remaining {
            break;
        }
        remaining -= cost;
        packed.push(result);
    }
    packed
}
```

Also add to the test module's imports: `use crate::model::MemoryIndex;`

**Step 4: Run tests to verify they pass**

Run: `cargo test -p shabka-core --no-default-features -- budget_truncate`
Expected: all 4 tests PASS

**Step 5: Commit**

```bash
git add crates/shabka-core/src/ranking.rs
git commit -m "feat: add budget_truncate for token-aware search results"
```

---

### Task 3: Context Pack Module

**Files:**
- Create: `crates/shabka-core/src/context_pack.rs`
- Modify: `crates/shabka-core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `context_pack.rs`

**Step 1: Write the full module with tests**

```rust
// crates/shabka-core/src/context_pack.rs
use crate::model::Memory;
use crate::tokens::estimate_memory_tokens;
use serde::Serialize;

/// A packed set of memories that fits within a token budget.
#[derive(Debug, Serialize)]
pub struct ContextPack {
    pub memories: Vec<Memory>,
    pub total_tokens: usize,
    pub budget: usize,
    pub project_id: Option<String>,
}

/// Build a context pack by greedily packing ranked memories into a token budget.
/// Memories must already be sorted by relevance (highest first).
pub fn build_context_pack(
    memories: Vec<Memory>,
    token_budget: usize,
    project_id: Option<String>,
) -> ContextPack {
    let mut remaining = token_budget;
    let mut packed = Vec::new();
    let mut total = 0;
    for memory in memories {
        let cost = estimate_memory_tokens(&memory);
        if cost > remaining {
            break;
        }
        remaining -= cost;
        total += cost;
        packed.push(memory);
    }
    ContextPack {
        memories: packed,
        total_tokens: total,
        budget: token_budget,
        project_id,
    }
}

/// Format a context pack as paste-ready markdown.
pub fn format_context_pack(pack: &ContextPack) -> String {
    let mut out = String::new();

    // Header
    let project_label = pack
        .project_id
        .as_deref()
        .unwrap_or("all");
    out.push_str(&format!(
        "# Project Context: {} ({} memories, ~{} tokens)\n\n",
        project_label,
        pack.memories.len(),
        pack.total_tokens,
    ));

    // Each memory
    for (i, memory) in pack.memories.iter().enumerate() {
        if i > 0 {
            out.push_str("---\n\n");
        }

        // Title line
        out.push_str(&format!("## [{}] {}\n", memory.kind, memory.title));

        // Metadata line
        let date = memory.created_at.format("%Y-%m-%d");
        let tags_str = if memory.tags.is_empty() {
            String::new()
        } else {
            format!(" | tags: {}", memory.tags.join(", "))
        };
        out.push_str(&format!(
            "*{} | importance: {}{}*\n\n",
            date, memory.importance, tags_str,
        ));

        // Content
        out.push_str(&memory.content);
        out.push_str("\n\n");
    }

    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MemoryKind;

    fn test_memory(title: &str, content: &str) -> Memory {
        Memory::new(
            title.to_string(),
            content.to_string(),
            MemoryKind::Decision,
            "test".to_string(),
        )
        .with_tags(vec!["test".to_string()])
    }

    #[test]
    fn test_build_context_pack_fits_all() {
        let memories = vec![
            test_memory("First", "Short content"),
            test_memory("Second", "Also short"),
        ];
        let pack = build_context_pack(memories, 10000, Some("thesis".to_string()));
        assert_eq!(pack.memories.len(), 2);
        assert_eq!(pack.budget, 10000);
        assert!(pack.total_tokens > 0);
        assert!(pack.total_tokens <= 10000);
        assert_eq!(pack.project_id, Some("thesis".to_string()));
    }

    #[test]
    fn test_build_context_pack_exceeds_budget() {
        let memories = vec![
            test_memory("First", &"a".repeat(200)),
            test_memory("Second", &"b".repeat(200)),
            test_memory("Third", &"c".repeat(200)),
        ];
        // Each memory: ~50 content + ~5 title + ~2 tags + 20 overhead ≈ 77 tokens
        // Budget 100 should fit only 1
        let pack = build_context_pack(memories, 100, None);
        assert_eq!(pack.memories.len(), 1);
        assert_eq!(pack.memories[0].title, "First");
    }

    #[test]
    fn test_build_context_pack_zero_budget() {
        let memories = vec![test_memory("Title", "Content")];
        let pack = build_context_pack(memories, 0, None);
        assert!(pack.memories.is_empty());
        assert_eq!(pack.total_tokens, 0);
    }

    #[test]
    fn test_build_context_pack_single_oversized() {
        let memories = vec![test_memory("Big", &"x".repeat(10000))];
        // Memory is ~2500+ tokens, budget is 100
        let pack = build_context_pack(memories, 100, None);
        assert!(pack.memories.is_empty());
    }

    #[test]
    fn test_format_context_pack_output() {
        let memories = vec![test_memory("Auth flow", "Use JWT tokens for auth.")];
        let pack = build_context_pack(memories, 10000, Some("thesis".to_string()));
        let output = format_context_pack(&pack);

        assert!(output.contains("# Project Context: thesis"));
        assert!(output.contains("## [decision] Auth flow"));
        assert!(output.contains("importance: 0.5"));
        assert!(output.contains("tags: test"));
        assert!(output.contains("Use JWT tokens for auth."));
    }

    #[test]
    fn test_format_context_pack_no_project() {
        let memories = vec![test_memory("Title", "Content")];
        let pack = build_context_pack(memories, 10000, None);
        let output = format_context_pack(&pack);
        assert!(output.contains("Project Context: all"));
    }

    #[test]
    fn test_format_context_pack_multiple_memories() {
        let memories = vec![
            test_memory("First", "Content 1"),
            test_memory("Second", "Content 2"),
        ];
        let pack = build_context_pack(memories, 10000, None);
        let output = format_context_pack(&pack);

        // Should have separator between memories
        assert!(output.contains("---"));
        assert!(output.contains("## [decision] First"));
        assert!(output.contains("## [decision] Second"));
    }

    #[test]
    fn test_format_context_pack_empty() {
        let pack = build_context_pack(vec![], 1000, Some("empty".to_string()));
        let output = format_context_pack(&pack);
        assert!(output.contains("0 memories"));
        assert!(!output.contains("---"));
    }
}
```

**Step 2: Register the module**

In `crates/shabka-core/src/lib.rs`, add `pub mod context_pack;` after `pub mod consolidate;` (alphabetical order, line 4).

**Step 3: Run tests to verify they pass**

Run: `cargo test -p shabka-core --no-default-features -- context_pack`
Expected: all 8 tests PASS

**Step 4: Commit**

```bash
git add crates/shabka-core/src/context_pack.rs crates/shabka-core/src/lib.rs
git commit -m "feat: add context pack builder and markdown formatter"
```

---

### Task 4: CLI `--token-budget` Flag on Search

**Files:**
- Modify: `crates/shabka-cli/src/main.rs:34-52` (Search variant)
- Modify: `crates/shabka-cli/src/main.rs:189-204` (match arm)
- Modify: `crates/shabka-cli/src/main.rs:525-643` (cmd_search function)

**Step 1: Add flag to CLI enum**

In the `Search` variant of the `Cli` enum (line 34-52), add after the `json` field:

```rust
        /// Cap results to fit within a token budget (estimated)
        #[arg(long)]
        token_budget: Option<usize>,
```

**Step 2: Pass it through the match arm**

Update the `Cli::Search` match arm (line 189-204) to include `token_budget` in destructuring and pass it to `cmd_search`:

```rust
        Cli::Search {
            query,
            kind,
            limit,
            tag,
            project,
            json,
            token_budget,
        } => {
            // ... existing storage/embedder setup ...
            cmd_search(
                &storage, &embedder, user_id, &query, kind, limit, tag, project, json,
                token_budget,
            )
            .await
        }
```

**Step 3: Update `cmd_search` function**

Add `token_budget: Option<usize>` parameter to `cmd_search` (line 525). After the `take(limit)` line (line 599), apply budget truncation:

```rust
    let results: Vec<MemoryIndex> = ranked
        .into_iter()
        .take(limit)
        .map(|r| MemoryIndex::from((&r.memory, r.score)))
        .collect();

    // Apply token budget if set
    let results = match token_budget {
        Some(budget) => ranking::budget_truncate(results, budget),
        None => results,
    };
```

Add `use shabka_core::tokens;` is not needed — `budget_truncate` is in `ranking` module and imports `tokens` internally.

**Step 4: Run `cargo check` to verify compilation**

Run: `cargo check -p shabka-cli`
Expected: compiles with no errors

**Step 5: Run existing tests**

Run: `cargo test -p shabka-core --no-default-features`
Expected: all tests PASS (no regressions)

**Step 6: Commit**

```bash
git add crates/shabka-cli/src/main.rs
git commit -m "feat: add --token-budget flag to shabka search"
```

---

### Task 5: MCP `token_budget` on Search Tool

**Files:**
- Modify: `crates/shabka-mcp/src/server.rs:76-95` (SearchParams struct)
- Modify: `crates/shabka-mcp/src/server.rs:410-416` (search tool, after ranking)

**Step 1: Add field to `SearchParams`**

In the `SearchParams` struct (line 76-95), add after `limit`:

```rust
    #[schemars(description = "Cap results to fit within a token budget (estimated ~4 chars/token). Omit for no budget limit.")]
    #[serde(default)]
    pub token_budget: Option<usize>,
```

**Step 2: Apply budget in search tool**

In the `search` method (after line 416 where `top` is built), apply budget:

```rust
        // Apply token budget if set
        let top = match params.token_budget {
            Some(budget) => ranking::budget_truncate(top, budget),
            None => top,
        };
```

**Step 3: Run `cargo check` to verify compilation**

Run: `cargo check -p shabka-mcp`
Expected: compiles with no errors

**Step 4: Commit**

```bash
git add crates/shabka-mcp/src/server.rs
git commit -m "feat: add token_budget to MCP search tool"
```

---

### Task 6: CLI `context-pack` Subcommand

**Files:**
- Modify: `crates/shabka-cli/src/main.rs` (Cli enum + match arm + new cmd function)

**Step 1: Add subcommand to `Cli` enum**

After the `Reembed` variant, add:

```rust
    /// Generate a paste-ready context pack from project memories
    ContextPack {
        /// Search query to find relevant memories (default: all)
        #[arg(default_value = "")]
        query: String,
        /// Token budget for the pack (default 2000)
        #[arg(long, default_value = "2000")]
        tokens: usize,
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
        /// Filter by memory kind
        #[arg(short, long)]
        kind: Option<String>,
        /// Filter by tags (can be repeated)
        #[arg(short, long)]
        tag: Option<Vec<String>>,
        /// Output raw JSON instead of markdown
        #[arg(long)]
        json: bool,
        /// Write output to file instead of stdout
        #[arg(short, long)]
        output: Option<String>,
    },
```

**Step 2: Add match arm**

In the main match block, add:

```rust
        Cli::ContextPack {
            query,
            tokens,
            project,
            kind,
            tag,
            json,
            output,
        } => {
            let storage = make_storage(config);
            let embedder = EmbeddingService::from_config(&config.embedding)
                .context("failed to create embedding service")?;
            cmd_context_pack(
                &storage, &embedder, user_id, &query, tokens, project, kind, tag, json, output,
            )
            .await
        }
```

**Step 3: Implement `cmd_context_pack`**

Add the function (near `cmd_search`):

```rust
async fn cmd_context_pack(
    storage: &HelixStorage,
    embedder: &EmbeddingService,
    user_id: &str,
    query: &str,
    token_budget: usize,
    project: Option<String>,
    kind: Option<String>,
    tags: Option<Vec<String>>,
    json: bool,
    output: Option<String>,
) -> Result<()> {
    use shabka_core::context_pack::{build_context_pack, format_context_pack};

    let kind_filter: Option<MemoryKind> = match &kind {
        Some(k) => Some(k.parse().map_err(|e: String| anyhow::anyhow!("{}", e))?),
        None => None,
    };
    let tag_filter: Vec<String> = tags.unwrap_or_default();

    // Wide search for candidates
    let search_query = if query.is_empty() { "*" } else { query };
    let embedding = embedder
        .embed(search_query)
        .await
        .context("failed to embed query")?;

    let mut candidates = storage
        .vector_search(&embedding, 50)
        .await
        .context("vector search failed")?;

    // Filter by privacy
    sharing::filter_search_results(&mut candidates, user_id);

    // Get relation counts for ranking
    let memory_ids: Vec<Uuid> = candidates.iter().map(|(m, _)| m.id).collect();
    let counts = storage
        .count_relations(&memory_ids)
        .await
        .unwrap_or_default();
    let count_map: HashMap<Uuid, usize> = counts.into_iter().collect();

    // Build rank candidates, applying filters
    let rank_candidates: Vec<RankCandidate> = candidates
        .into_iter()
        .filter(|(m, _)| {
            if let Some(ref kf) = kind_filter {
                if m.kind != *kf {
                    return false;
                }
            }
            if !tag_filter.is_empty() && !tag_filter.iter().any(|t| m.tags.contains(t)) {
                return false;
            }
            if let Some(ref p) = project {
                if m.project_id.as_deref() != Some(p.as_str()) {
                    return false;
                }
            }
            true
        })
        .map(|(memory, vector_score)| {
            let kw_score = ranking::keyword_score(search_query, &memory);
            RankCandidate {
                relation_count: count_map.get(&memory.id).copied().unwrap_or(0),
                keyword_score: kw_score,
                memory,
                vector_score,
            }
        })
        .collect();

    let ranked = ranking::rank(rank_candidates, &RankingWeights::default());
    let memories: Vec<Memory> = ranked.into_iter().map(|r| r.memory).collect();

    // Build context pack
    let pack = build_context_pack(memories, token_budget, project.clone());

    if pack.memories.is_empty() {
        eprintln!("{}", "No memories fit within the token budget.".dimmed());
        return Ok(());
    }

    // Format output
    let text = if json {
        serde_json::to_string_pretty(&pack)?
    } else {
        format_context_pack(&pack)
    };

    // Write to file or stdout
    match output {
        Some(path) => {
            std::fs::write(&path, &text)
                .with_context(|| format!("failed to write to {path}"))?;
            eprintln!(
                "{} {} ({} memories, ~{} tokens)",
                "Wrote".green(),
                path,
                pack.memories.len(),
                pack.total_tokens,
            );
        }
        None => {
            println!("{text}");
        }
    }

    Ok(())
}
```

**Step 4: Run `cargo check` to verify compilation**

Run: `cargo check -p shabka-cli`
Expected: compiles with no errors

**Step 5: Commit**

```bash
git add crates/shabka-cli/src/main.rs
git commit -m "feat: add shabka context-pack command for paste-ready project context"
```

---

### Task 7: Full Validation

**Step 1: Run all unit tests**

Run: `cargo test -p shabka-core --no-default-features`
Expected: all tests PASS (206 existing + 19 new = 225 total)

**Step 2: Run clippy**

Run: `cargo clippy --workspace --no-default-features -- -D warnings`
Expected: no warnings

**Step 3: Run `just check`**

Run: `just check`
Expected: all checks pass

**Step 4: Commit (if any fixups needed)**

```bash
git add -A && git commit -m "fix: address clippy warnings from token-budget feature"
```

Only if Step 2 or 3 revealed issues. Skip if clean.
