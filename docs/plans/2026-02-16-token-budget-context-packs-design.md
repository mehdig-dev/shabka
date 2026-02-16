# Token-Budgeted Retrieval & Context Packs

**Date:** 2026-02-16
**Status:** Approved

## Problem

Users on rate-limited or budget-constrained AI services (free tiers, local models) need to maximize the value of every API call. Currently Shabka controls result count via `--limit` but has no way to control the *token cost* of results. Users who hop between AI providers (ChatGPT free, Claude, local Ollama) also lack a way to export portable project context.

## Features

### 1. Token-Budgeted Retrieval

Add `--token-budget N` flag to `shabka search` and MCP `search` tool. After ranking, greedily pack results until budget is exhausted.

**CLI usage:**
```bash
shabka search "experiment results" --token-budget 1500
shabka search "methodology" --token-budget 800 --kind decision --project thesis
```

**MCP:** Optional `token_budget: Option<usize>` on `SearchParams`.

**Behavior:**
- Results are already ranked by score; greedy packing = best results first
- If both `--limit` and `--token-budget` are set, apply limit first, then budget
- No budget set = unchanged behavior (uses `--limit` as today)

### 2. Context Packs

New CLI command that retrieves full memory content and formats as a paste-ready context block.

**CLI usage:**
```bash
shabka context-pack --project thesis --tokens 2000
shabka context-pack --project thesis --tokens 1500 --kind decision --tag methodology
shabka context-pack --project thesis --tokens 2000 --json
shabka context-pack --project thesis --tokens 2000 -o context.md
```

**Default `--tokens`:** 2000.

**Flow:** Search with limit=50 for wide candidate net -> rank -> fetch full memories for top results -> greedily pack within token budget -> format output.

**Text output format:**
```markdown
# Project Context: thesis (3 memories, ~1847 tokens)

## [decision] Authentication flow with JWT tokens
*2026-02-10 | importance: 0.8 | tags: auth, jwt*

Full content of the memory here...

---

## [fix] Token rotation race condition
*2026-02-12 | importance: 0.7 | tags: auth, bugfix*

Full content here...
```

**JSON output (`--json`):** Full `ContextPack` struct with memories array + token/budget metadata.

## Technical Design

### Token Estimation

Shared utility in `shabka-core/src/tokens.rs`. Char-based heuristic (~4 chars/token), no external dependencies.

```rust
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

pub fn estimate_memory_tokens(memory: &Memory) -> usize {
    estimate_tokens(&memory.title)
        + estimate_tokens(&memory.content)
        + estimate_tokens(&memory.tags.join(", "))
        + 20 // metadata overhead (kind, date, id)
}

pub fn estimate_index_tokens(index: &MemoryIndex) -> usize {
    estimate_tokens(&index.title)
        + estimate_tokens(&index.tags.join(", "))
        + 15 // metadata overhead
}
```

### Budget Truncation (ranking.rs)

```rust
pub fn budget_truncate(results: Vec<MemoryIndex>, token_budget: usize) -> Vec<MemoryIndex> {
    let mut remaining = token_budget;
    let mut packed = Vec::new();
    for result in results {
        let cost = estimate_index_tokens(&result);
        if cost > remaining { break; }
        remaining -= cost;
        packed.push(result);
    }
    packed
}
```

### Context Pack Builder (context_pack.rs)

```rust
pub struct ContextPack {
    pub memories: Vec<Memory>,
    pub total_tokens: usize,
    pub budget: usize,
    pub project_id: Option<String>,
}

pub fn build_context_pack(
    memories: Vec<Memory>,
    token_budget: usize,
) -> ContextPack { /* greedy packing */ }

pub fn format_context_pack(pack: &ContextPack) -> String { /* markdown output */ }
```

## File Changes

**New files:**
- `shabka-core/src/tokens.rs` — token estimation utilities
- `shabka-core/src/context_pack.rs` — `ContextPack` struct, builder, text formatter

**Modified files:**
- `shabka-core/src/lib.rs` — add `pub mod tokens; pub mod context_pack;`
- `shabka-core/src/ranking.rs` — add `budget_truncate()`
- `shabka-cli/src/main.rs` — add `--token-budget` flag to search, add `context-pack` subcommand
- `shabka-mcp/src/server.rs` — add `token_budget` field to `SearchParams`

## Tests

- `tokens.rs` — estimation: empty string, short text, long text, memory struct
- `context_pack.rs` — packing: fits all, budget exceeded mid-list, zero budget, oversized single memory
- `ranking.rs` — `budget_truncate` unit test

## Not in Scope

- MCP context-pack tool (CLI-first, can add MCP tool later)
- Saved/named packs
- Relation inclusion in context packs
- Tiktoken accurate token counting
- New config sections
