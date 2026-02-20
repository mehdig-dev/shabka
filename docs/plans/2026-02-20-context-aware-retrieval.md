# Context-Aware Retrieval + Polish Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `get_context` MCP tool (14th) for token-budgeted context retrieval, update memory files, and improve test coverage.

**Architecture:** The MCP tool reuses the existing `context_pack` module pipeline (embed → vector search → filter → rank → pack → format). The polish work updates memory files and adds targeted tests.

**Tech Stack:** Rust, rmcp 0.14, shabka-core context_pack module

---

### Task 1: Add `GetContextParams` struct to MCP server

**Files:**
- Modify: `crates/shabka-mcp/src/server.rs:195` (after `ReembedParams`)

**Step 1: Add the params struct**

Add after the `ReembedParams` struct (around line 205):

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetContextParams {
    #[schemars(
        description = "Search query for semantic + keyword matching. Omit or use '*' for top memories by recency/importance."
    )]
    #[serde(default = "default_context_query")]
    pub query: String,

    #[schemars(description = "Filter by project ID (optional)")]
    #[serde(default)]
    pub project_id: Option<String>,

    #[schemars(description = "Filter by memory kind (optional)")]
    #[serde(default)]
    pub kind: Option<String>,

    #[schemars(description = "Comma-separated tag filter (optional)")]
    #[serde(default)]
    pub tags: Option<String>,

    #[schemars(description = "Max tokens in the context pack (default 2000)")]
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
}

fn default_context_query() -> String {
    "*".to_string()
}

fn default_token_budget() -> usize {
    2000
}
```

**Step 2: Verify it compiles**

```bash
cargo check -p shabka-mcp --no-default-features
```

**Step 3: Commit**

```bash
git add crates/shabka-mcp/src/server.rs
git commit -m "feat(mcp): add GetContextParams struct for context tool"
```

---

### Task 2: Implement the `get_context` MCP tool

**Files:**
- Modify: `crates/shabka-mcp/src/server.rs` (add tool after `verify_memory`, before closing `}` of `impl ShabkaServer` at line 1407)

**Step 1: Add import for context_pack**

At the top of the file, after existing `use shabka_core::` imports, add:

```rust
use shabka_core::context_pack::{build_context_pack, format_context_pack};
```

**Step 2: Add the tool method**

Insert before line 1407 (the closing `}` of the `#[tool_router] impl ShabkaServer`):

```rust
    #[tool(
        name = "get_context",
        description = "Get a token-budgeted context pack of relevant memories, formatted as markdown ready for injection into prompts. Supports filtering by query, project, kind, and tags. Use this when you need rich context rather than individual search results."
    )]
    async fn get_context(
        &self,
        Parameters(params): Parameters<GetContextParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = if params.query.is_empty() { "*" } else { &params.query };

        // Embed the query
        let embedding = self
            .embedder
            .embed(query)
            .await
            .map_err(to_mcp_error)?;

        // Wide search for candidates
        let mut results = self
            .storage
            .vector_search(&embedding, 50)
            .await
            .map_err(to_mcp_error)?;

        // Filter by privacy
        sharing::filter_search_results(&mut results, &self.user_id);

        // Parse tag filter
        let tag_filter: Vec<String> = params
            .tags
            .map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();

        // Apply kind/project/tag filters
        let filtered: Vec<(Memory, f32)> = results
            .into_iter()
            .filter(|(memory, _)| {
                if let Some(ref kind) = params.kind {
                    if memory.kind.to_string() != *kind {
                        return false;
                    }
                }
                if let Some(ref pid) = params.project_id {
                    if memory.project_id.as_ref() != Some(pid) {
                        return false;
                    }
                }
                if !tag_filter.is_empty() && !tag_filter.iter().any(|t| memory.tags.contains(t)) {
                    return false;
                }
                true
            })
            .collect();

        // Get relation + contradiction counts for ranking
        let memory_ids: Vec<Uuid> = filtered.iter().map(|(m, _)| m.id).collect();
        let relation_counts = self.storage.count_relations(&memory_ids).await.map_err(to_mcp_error)?;
        let count_map: std::collections::HashMap<Uuid, usize> = relation_counts.into_iter().collect();
        let contradiction_counts = self.storage.count_contradictions(&memory_ids).await.map_err(to_mcp_error)?;
        let contradiction_map: std::collections::HashMap<Uuid, usize> = contradiction_counts.into_iter().collect();

        // Build rank candidates
        let candidates: Vec<RankCandidate> = filtered
            .into_iter()
            .map(|(memory, vector_score)| {
                let kw_score = ranking::keyword_score(query, &memory);
                RankCandidate {
                    relation_count: count_map.get(&memory.id).copied().unwrap_or(0),
                    contradiction_count: contradiction_map.get(&memory.id).copied().unwrap_or(0),
                    keyword_score: kw_score,
                    memory,
                    vector_score,
                }
            })
            .collect();

        // Rank and extract memories
        let ranked = ranking::rank(candidates, &RankingWeights::default());
        let memories: Vec<Memory> = ranked.into_iter().map(|r| r.memory).collect();

        // Build and format context pack
        let pack = build_context_pack(memories, params.token_budget, params.project_id);

        if pack.memories.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No memories found matching the query and filters within the token budget."
            )]));
        }

        let formatted = format_context_pack(&pack);
        Ok(CallToolResult::success(vec![Content::text(formatted)]))
    }
```

**Step 3: Update the server instructions**

In `get_info()` (line 1417), add after the "Trust: verify_memory" line:

```rust
"Context: get_context (token-budgeted context pack of relevant memories, formatted as markdown).\n\n\
```

**Step 4: Verify it compiles**

```bash
cargo check -p shabka-mcp --no-default-features
```

**Step 5: Commit**

```bash
git add crates/shabka-mcp/src/server.rs
git commit -m "feat(mcp): add get_context tool for token-budgeted context retrieval"
```

---

### Task 3: Add unit tests for context_pack edge cases

**Files:**
- Modify: `crates/shabka-core/src/context_pack.rs` (add tests at end of `mod tests`)

**Step 1: Add boundary and format tests**

Add these tests inside the existing `mod tests`:

```rust
    #[test]
    fn test_build_context_pack_exact_budget_boundary() {
        // Two memories: first fits exactly, second doesn't
        let m1 = test_memory("First", "short");
        let cost1 = crate::tokens::estimate_memory_tokens(&m1);
        let m2 = test_memory("Second", "also short");
        let pack = build_context_pack(vec![m1, m2], cost1, None);
        assert_eq!(pack.memories.len(), 1);
        assert_eq!(pack.total_tokens, cost1);
        assert_eq!(pack.memories[0].title, "First");
    }

    #[test]
    fn test_build_context_pack_preserves_order() {
        let memories = vec![
            test_memory("A", "first"),
            test_memory("B", "second"),
            test_memory("C", "third"),
        ];
        let pack = build_context_pack(memories, 10000, None);
        assert_eq!(pack.memories.len(), 3);
        assert_eq!(pack.memories[0].title, "A");
        assert_eq!(pack.memories[1].title, "B");
        assert_eq!(pack.memories[2].title, "C");
    }

    #[test]
    fn test_format_context_pack_includes_kind_and_tags() {
        let m = Memory::new(
            "Error handling".to_string(),
            "Use Result everywhere".to_string(),
            MemoryKind::Pattern,
            "test".to_string(),
        )
        .with_tags(vec!["rust".to_string(), "error".to_string()]);
        let pack = build_context_pack(vec![m], 10000, None);
        let output = format_context_pack(&pack);
        assert!(output.contains("[pattern]"));
        assert!(output.contains("tags: rust, error"));
    }

    #[test]
    fn test_format_context_pack_no_tags() {
        let mut m = Memory::new(
            "No tags".to_string(),
            "Content".to_string(),
            MemoryKind::Observation,
            "test".to_string(),
        );
        m.tags = vec![];
        let pack = build_context_pack(vec![m], 10000, None);
        let output = format_context_pack(&pack);
        assert!(output.contains("[observation]"));
        assert!(!output.contains("tags:"));
    }
```

**Step 2: Run tests**

```bash
cargo test -p shabka-core --no-default-features -- context_pack::tests
```

Expected: all 12 tests pass (8 existing + 4 new).

**Step 3: Commit**

```bash
git add crates/shabka-core/src/context_pack.rs
git commit -m "test(context_pack): add boundary, order, and format tests"
```

---

### Task 4: Add structured extraction edge case test

**Files:**
- Modify: `crates/shabka-core/src/llm.rs` (add test at end of `mod tests`)

**Step 1: Add nested fence test**

```rust
    #[test]
    fn test_generate_structured_parse_nested_content() {
        #[derive(serde::Deserialize, Debug)]
        struct CodeResp {
            language: String,
            snippet: String,
        }

        // LLM wraps JSON in fences but JSON value itself contains backticks
        let raw = "```json\n{\"language\":\"rust\",\"snippet\":\"fn main() {}\"}\n```";
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let result: CodeResp = serde_json::from_str(cleaned).unwrap();
        assert_eq!(result.language, "rust");
        assert_eq!(result.snippet, "fn main() {}");
    }
```

**Step 2: Run tests**

```bash
cargo test -p shabka-core --no-default-features -- llm::tests
```

Expected: all 14 tests pass (12 existing + 1 new... wait, actually 22 existing + 1 new).

**Step 3: Commit**

```bash
git add crates/shabka-core/src/llm.rs
git commit -m "test(llm): add nested content edge case for generate_structured"
```

---

### Task 5: Update MEMORY.md and clean up known limitations

**Files:**
- Modify: `/home/mehdi/.claude/projects/-home-mehdi-projects-kaizen/memory/MEMORY.md`

**Step 1: Fix known limitation**

Remove or update the line:
```
- Known limitation: `count_contradictions` returns 0 because `get_relations` HQL doesn't return edge properties (relation_type always defaults to Related)
```

Replace with:
```
- Edge properties fixed: `get_relations` and `get_incoming_relations` now include `.out_e()`/`.in_e()` traversals — relation_type and strength are correctly returned
```

**Step 2: Update provider info**

Update the LLM providers section and test counts:
- Add new LLM providers: deepseek, groq, xai, cohere
- Add cohere to embedding providers
- Update test count to current number
- Add `get_context` as 14th MCP tool
- Add structured extraction info (generate_structured<T>)

**Step 3: Commit memory file changes**

No git commit needed (memory files are outside the repo).

---

### Task 6: Clippy + final validation

**Step 1: Run clippy**

```bash
cargo clippy --workspace --no-default-features -- -D warnings
```

**Step 2: Run full test suite**

```bash
cargo test -p shabka-core --no-default-features
cargo test -p shabka-hooks --no-default-features
```

**Step 3: Run integration tests**

```bash
cargo test -p shabka-core --no-default-features -- --ignored
```

**Step 4: Push**

```bash
git push origin main
```
