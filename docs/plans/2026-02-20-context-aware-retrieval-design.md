# Context-Aware Retrieval + Polish — Design

## Goal

Add a `get_context` MCP tool (14th tool) for token-budgeted context retrieval, update memory files to reflect recent fixes, and improve test coverage.

## MCP `get_context` Tool

**Purpose:** Return a formatted, token-budgeted context pack of relevant memories. LLMs using Shabka via MCP can request rich context to inject into their reasoning.

**Parameters:**
- `query: String` — Search query (optional, defaults to `"*"` for top memories)
- `project_id: Option<String>` — Filter by project
- `kind: Option<String>` — Filter by memory kind
- `tags: Option<String>` — Comma-separated tag filter
- `token_budget: Option<u32>` — Max tokens in pack (default 2000)

**Pipeline** (reuses existing modules):
1. Embed query via `EmbeddingService`
2. Vector search top 50 candidates via `HelixStorage`
3. Filter by privacy, kind, project, tags
4. Rank via 7-factor formula (`ranking::rank()`)
5. Pack into budget via `context_pack::build_context_pack()`
6. Format via `context_pack::format_context_pack()`
7. Return as text content block

**Return:** Markdown-formatted context pack with project header, memories with metadata, separated by `---`.

**Error handling:** `ErrorData::internal_error()` for storage/embedding failures.

**Progressive disclosure:** `search` → quick results, `get_context` → rich formatted context, `get_memories` → full detail for specific IDs.

## Polish

### Memory File Updates
- Fix known limitation: `count_contradictions` now works (edge property fix in commit `49eca2c`)
- Add new providers: deepseek, groq, xai, cohere to provider tables
- Update test counts
- Add `get_context` as 14th MCP tool

### Test Improvements
- `context_pack` module: budget boundary tests, project_id filtering, format verification
- MCP tool: unit tests for query/filter/budget/empty results
- Structured extraction: nested fence edge case

## Non-Goals
- No auto-injection into hooks (future work)
- No search explanation / signal breakdown (future work)
- No relation-aware packing (future work)
- No web dashboard changes
