# Rig Integration Design — Thin Adapter

**Date**: 2026-02-19
**Status**: Approved
**Scope**: Replace hand-rolled LLM + embedding HTTP code with Rig provider layer

## Goal

Replace Shabka's custom LLM and embedding provider implementations with [Rig](https://github.com/0xPlaygrounds/rig) (v0.31), a Rust LLM framework with 20+ provider integrations. The public API (`LlmService::generate()`, `EmbeddingService::embed()`) stays identical — only internals change.

## Motivation

| Problem | Impact |
|---------|--------|
| `EmbeddingService` enum has 7 manual match blocks | Every new provider requires touching 7+ sites |
| `LlmService` has 4 hand-rolled HTTP integrations (454 lines) | Hardcoded API versions, no retry, error type misuse |
| LLM errors use `ShabkaError::Embedding` | Misleading error classification |
| No retry on LLM calls | Transient failures (503, timeout) crash dedup/consolidate/auto-tag |
| Duplicated `resolve_api_key` in embedding/mod.rs and llm.rs | Maintenance burden |
| Separate `reqwest::Client` per service instance | Connection pool fragmentation |

## Approach: Thin Adapter

Keep `LlmService` and `EmbeddingService` as public types with the same API signatures. Internally, delegate to Rig's `CompletionModel` and `EmbeddingModel` traits.

### EmbeddingService After

```rust
pub struct EmbeddingService {
    inner: EmbeddingInner,
    provider_name: String,
    model_id: String,
    dimensions: usize,
}

enum EmbeddingInner {
    Rig(Box<dyn RigEmbeddingAdapter>),
    Hash(HashEmbeddingProvider),
}
```

- `from_config()` creates the appropriate `rig::providers::X::Client` and gets `client.embedding_model(model)`.
- `embed()`/`embed_batch()` delegate to Rig with `with_retry()` wrapping for remote providers.
- `f64 → f32` conversion at the boundary (safe for normalized unit vectors).
- Hash provider remains custom (no Rig equivalent, test-only).

### LlmService After

```rust
pub struct LlmService {
    model: Box<dyn RigCompletionAdapter>,
    config: LlmConfig,
}
```

- `from_config()` creates the appropriate Rig provider client and gets `client.completion_model(model)`.
- `generate(prompt, system)` builds a `CompletionRequest` and calls `model.completion()`.
- Now wrapped in `with_retry(3, 200, ...)` — fixes the missing retry bug.

### Error Handling Fix

Add `ShabkaError::Llm(String)` variant. All LLM errors use it instead of `ShabkaError::Embedding`. Update `is_transient()` to check `Llm` messages.

## Files Changed

| File | Change |
|------|--------|
| `Cargo.toml` (root) | Add `rig-core` to workspace deps |
| `shabka-core/Cargo.toml` | Add `rig-core`, remove `fastembed` |
| `shabka-core/src/embedding/mod.rs` | Rewrite: struct wrapping Rig + Hash |
| `shabka-core/src/embedding/provider.rs` | Keep (used by Hash provider) |
| `shabka-core/src/embedding/hash.rs` | Keep as-is |
| `shabka-core/src/embedding/openai.rs` | DELETE |
| `shabka-core/src/embedding/gemini.rs` | DELETE |
| `shabka-core/src/embedding/local.rs` | DELETE |
| `shabka-core/src/llm.rs` | Rewrite: wrap Rig CompletionModel |
| `shabka-core/src/error.rs` | Add `Llm` variant |
| `shabka-core/src/config/mod.rs` | Unify `resolve_api_key`, update `VALID_PROVIDERS` |

## Files NOT Changed

dedup.rs, consolidate.rs, auto_tag.rs, hooks, MCP, web, CLI — they all call the same public API.

## Dependencies

```toml
[workspace.dependencies]
rig-core = { version = "0.31", default-features = false, features = ["reqwest-rustls"] }
reqwest = { version = "0.13", features = ["json"] }  # kept for helix-rs
# fastembed removed (never worked on WSL2)
```

Using `reqwest-rustls` avoids OpenSSL linking issues on WSL2.

## f64 → f32 Conversion

Rig returns `Vec<f64>` embeddings. Shabka uses `Vec<f32>` everywhere. The conversion is safe:
- Embedding vectors are normalized (unit length, values in [-1.0, 1.0])
- f32 has 7 significant digits — more than enough for cosine similarity
- HelixDB stores F64 but Shabka already reads as f32
- Every Rig vector store integration does this same conversion internally

## Testing

- All 287 existing unit tests pass unchanged (same public API).
- Hash provider tests remain identical (no Rig involvement).
- `from_config` tests for API-key providers stay the same.
- New tests: f64→f32 conversion, `ShabkaError::Llm` variant, retry wraps LLM calls.

## Lines Impact

- ~665 lines removed (4 generate methods, OpenAI/Gemini/local providers, enum dispatch)
- ~150 lines added (Rig adapter code)
- Net: ~500 lines fewer

## What This Unlocks (Future)

Without additional work, Rig gives us the ability to later:
- Add 15+ more LLM providers (DeepSeek, Groq, xAI, Cohere, etc.) with zero code
- Add streaming for consolidation/auto-tag
- Use structured extraction (JSON schema) for dedup/auto-tag
- Build a RAG-based "Shabka assistant" agent
- Use `rig-helixdb` for vector store operations
