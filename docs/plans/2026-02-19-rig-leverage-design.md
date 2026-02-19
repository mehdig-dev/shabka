# Rig Leverage + Edge Property Fix — Design

**Date**: 2026-02-19
**Status**: Approved
**Scope**: Fix HelixDB edge properties, add Rig structured extraction, add 5 new LLM providers

## 1. HelixDB Edge Property Fix

### Problem

The `get_relations` and `get_incoming_relations` HQL queries correctly declare `edges <- source::OutE<RelatesTo>`, but the HelixDB code generator produced an incomplete handler. The generated Rust handler at `helix/.helix/dev/helix-container/src/queries.rs` only calls `.out_node("RelatesTo")` — it never calls `.out_edge("RelatesTo")` and omits the `edges` field from the JSON response.

Result: `relation_type` always defaults to `Related`, `strength` always defaults to `0.5`. Three integration tests fail.

### Fix

Manually patch the generated handler to:
1. Add `.out_edge("RelatesTo")` traversal alongside `.out_node()`
2. Create an edge return type struct with `relation_type` and `strength` fields
3. Include `edges` array in the JSON response, paired by index with `target`
4. Same fix for `get_incoming_relations` (uses `InE`/`In` instead of `OutE`/`Out`)

Then `helix push dev` to redeploy.

The Rust client code (`storage/helix.rs`) is already correct — it parses `EdgeRecord` from the `edges` array and pairs by index. No client-side changes needed.

### Affected Tests

| Test | File | Expected Result |
|------|------|-----------------|
| `test_relations` | `tests/helix_roundtrip.rs:251` | `Fixes` instead of `Related` |
| `test_count_contradictions` | `tests/mcp_integration.rs:385` | 1 instead of 0 |
| `test_ollama_search_with_helix` | `tests/ollama_embedding.rs:175` | May still fail (timing) |

## 2. Rig Structured Extraction

### Problem

Three modules manually parse LLM JSON responses with markdown-stripping, field extraction, and custom error handling (~135 lines total). This is fragile — LLMs sometimes wrap JSON in markdown fences or return invalid JSON.

### Approach

Replace manual parsing with Rig's `Extractor<M, T>` API:
- Define response structs with `#[derive(Deserialize, Serialize, JsonSchema)]`
- Use `extractor.extract(prompt)` instead of `llm.generate(prompt)` + manual parse
- Providers enforce JSON schema compliance natively (Ollama `format` field, OpenAI `response_format`)

### LlmService Change

Add method to expose the inner completion model for building extractors:

```rust
impl LlmService {
    /// Build a Rig Extractor for structured output.
    pub fn extractor<T: JsonSchema + DeserializeOwned + Serialize + Send + Sync>(
        &self,
    ) -> ExtractorBuilder<T> { ... }

    // existing generate() stays for unstructured use
}
```

### Modules

**auto_tag.rs** — simplest
- Delete `parse_auto_tag_response()` (~30 lines)
- Define `AutoTagResponse { tags: Vec<String>, importance: f32 }`
- Clamp importance after extraction

**consolidate.rs** — medium
- Delete `parse_consolidated_response()` (~40 lines)
- Define `ConsolidateResponse { title, content, kind, tags, importance }`
- Map kind string to `MemoryKind` enum after extraction

**dedup.rs** — most complex
- Delete `parse_llm_response()` + `resolve_target_id()` (~65 lines)
- Define `DedupResponse { decision, target_id, merged_title, merged_content, reason }`
- Keep ID-mapping logic (simpler — operates on typed struct, not raw JSON)

### Test Impact

- Parse tests become struct deserialization tests (simpler)
- Markdown-strip test cases removed (providers enforce JSON natively)
- `build_dedup_prompt` test unchanged (prompt construction, not parsing)

## 3. New LLM Providers

### Providers

| Provider | Env Var | Default Model | Embeddings |
|----------|---------|---------------|------------|
| Anthropic | `ANTHROPIC_API_KEY` | claude-sonnet-4-20250514 | No |
| DeepSeek | `DEEPSEEK_API_KEY` | deepseek-chat | No |
| Groq | `GROQ_API_KEY` | llama-3.3-70b-versatile | No |
| xAI | `XAI_API_KEY` | grok-3-mini-fast | No |
| Cohere | `COHERE_API_KEY` | command-r | Yes (embed-english-v3.0, 1024d) |

### Implementation

Each provider is a new match arm in `from_config()` using the existing `RigCompletionWrapper<M>` / `RigModelWrapper<M>` adapter pattern. No architectural changes.

### Config Changes

- `VALID_LLM_PROVIDERS`: add `anthropic`, `deepseek`, `groq`, `xai`, `cohere`
- `VALID_PROVIDERS` (embedding): add `cohere`
- `kaizen init` provider picker: include new options
- Config validation: check env vars for new providers

### Limitations

- No custom `base_url` for these 5 providers (Rig v0.31 hardcodes URLs)
- OpenAI and Ollama retain `base_url` support
- No streaming (not needed for Shabka's short LLM calls)

## Files Changed

| File | Change |
|------|--------|
| `helix/.helix/dev/helix-container/src/queries.rs` | Patch edge traversal in get_relations + get_incoming_relations |
| `shabka-core/src/llm.rs` | Add extractor method, 5 new provider match arms |
| `shabka-core/src/embedding/mod.rs` | Add cohere match arm |
| `shabka-core/src/dedup.rs` | Replace parse_llm_response with Extractor |
| `shabka-core/src/auto_tag.rs` | Replace parse_auto_tag_response with Extractor |
| `shabka-core/src/consolidate.rs` | Replace parse_consolidated_response with Extractor |
| `shabka-core/src/config/mod.rs` | Update VALID_PROVIDERS, VALID_LLM_PROVIDERS, validation |
| `shabka-cli/src/commands/init.rs` | Add new providers to picker |

## What This Unlocks

- Edge properties work → trust scoring counts real contradictions → more accurate ranking
- Structured extraction → fewer LLM parse failures → more reliable dedup/auto-tag/consolidate
- 5 new providers → users can pick their preferred LLM without config hacks
