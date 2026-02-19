# Rig Integration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace hand-rolled LLM and embedding HTTP code with Rig v0.31, keeping the same public API.

**Architecture:** Thin adapter pattern — `LlmService` and `EmbeddingService` keep their public signatures but delegate to Rig's provider implementations internally. Hash embedding provider stays custom. A new `ShabkaError::Llm` variant fixes the error type misuse.

**Tech Stack:** rig-core 0.31 (reqwest-rustls), existing shabka-core infrastructure

**Design doc:** `docs/plans/2026-02-19-rig-integration-design.md`

---

### Task 1: Add rig-core dependency and update Cargo.toml files

**Files:**
- Modify: `Cargo.toml` (root workspace)
- Modify: `crates/shabka-core/Cargo.toml`

**Step 1: Add rig-core to workspace dependencies**

In root `Cargo.toml`, add under `[workspace.dependencies]`:

```toml
# LLM framework
rig-core = { version = "0.31", default-features = false, features = ["reqwest-rustls"] }
```

**Step 2: Update shabka-core dependencies**

In `crates/shabka-core/Cargo.toml`:

1. Add `rig-core = { workspace = true }` under `[dependencies]`
2. Remove `fastembed = { workspace = true, optional = true }` line
3. Change `[features]` section to:

```toml
[features]
default = []
```

This removes the `embed-local` feature entirely (fastembed never worked on WSL2).

**Step 3: Verify it compiles**

Run: `cargo check -p shabka-core --no-default-features`
Expected: Should compile. There will be dead code warnings for local.rs — that's fine, we delete it in a later task.

**Step 4: Commit**

```bash
git add Cargo.toml crates/shabka-core/Cargo.toml Cargo.lock
git commit -m "build: add rig-core 0.31, remove fastembed dependency"
```

---

### Task 2: Add ShabkaError::Llm variant and unify resolve_api_key

**Files:**
- Modify: `crates/shabka-core/src/error.rs`
- Modify: `crates/shabka-core/src/config/mod.rs`

**Step 1: Write the failing test for ShabkaError::Llm**

In `crates/shabka-core/src/error.rs`, add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn test_llm_error_display() {
    let err = ShabkaError::Llm("model not found".into());
    assert_eq!(err.to_string(), "LLM error: model not found");
}

#[test]
fn test_llm_transient_503() {
    let err = ShabkaError::Llm("API error 503: service unavailable".into());
    assert!(err.is_transient());
}

#[test]
fn test_llm_permanent_401() {
    let err = ShabkaError::Llm("API error 401: unauthorized".into());
    assert!(!err.is_transient());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p shabka-core --no-default-features --lib error::tests`
Expected: FAIL — `Llm` variant doesn't exist

**Step 3: Add the Llm variant**

In `crates/shabka-core/src/error.rs`, add to the `ShabkaError` enum:

```rust
#[error("LLM error: {0}")]
Llm(String),
```

And update `is_transient()`:

```rust
Self::Embedding(msg) | Self::Storage(msg) | Self::Llm(msg) => is_transient_message(msg),
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p shabka-core --no-default-features --lib error::tests`
Expected: PASS (all 10 error tests)

**Step 5: Add shared resolve_api_key to config module**

In `crates/shabka-core/src/config/mod.rs`, add this public function (near the end, before tests):

```rust
/// Resolve an API key: check config field first, then environment variable.
/// Used by both embedding and LLM service initialization.
pub fn resolve_api_key(
    api_key: Option<&str>,
    env_var_override: Option<&str>,
    default_env_var: &str,
    provider_name: &str,
    service_kind: &str,
) -> crate::error::Result<String> {
    if let Some(key) = api_key {
        if !key.is_empty() {
            return Ok(key.to_string());
        }
    }

    let env_var_name = env_var_override.unwrap_or(default_env_var);

    std::env::var(env_var_name).map_err(|_| {
        crate::error::ShabkaError::Config(format!(
            "{provider_name} {service_kind} provider requires an API key \
             (set {service_kind}.api_key or {env_var_name})"
        ))
    })
}
```

**Step 6: Verify everything compiles**

Run: `cargo check -p shabka-core --no-default-features`
Expected: PASS

**Step 7: Commit**

```bash
git add crates/shabka-core/src/error.rs crates/shabka-core/src/config/mod.rs
git commit -m "refactor: add ShabkaError::Llm variant, shared resolve_api_key"
```

---

### Task 3: Rewrite EmbeddingService to use Rig internally

This is the largest task. The new `EmbeddingService` wraps Rig's embedding models for OpenAI/Ollama/Gemini, keeps the Hash provider as-is, and maintains the exact same public API.

**Files:**
- Rewrite: `crates/shabka-core/src/embedding/mod.rs`
- Keep: `crates/shabka-core/src/embedding/hash.rs` (no changes)
- Keep: `crates/shabka-core/src/embedding/provider.rs` (no changes)
- Delete: `crates/shabka-core/src/embedding/openai.rs`
- Delete: `crates/shabka-core/src/embedding/gemini.rs`
- Delete: `crates/shabka-core/src/embedding/local.rs`

**Step 1: Delete the old provider files**

```bash
rm crates/shabka-core/src/embedding/openai.rs
rm crates/shabka-core/src/embedding/gemini.rs
rm crates/shabka-core/src/embedding/local.rs
```

**Step 2: Rewrite embedding/mod.rs**

Replace the entire file with:

```rust
mod provider;
mod hash;

pub use provider::EmbeddingProvider;
pub use hash::HashEmbeddingProvider;

use crate::config::{self, EmbeddingConfig};
use crate::error::{Result, ShabkaError};
use crate::retry::with_retry;

use std::future::Future;
use std::pin::Pin;

/// Object-safe wrapper trait for Rig embedding models.
/// Rig's `EmbeddingModel` uses RPITIT and isn't object-safe, so we wrap it.
trait RigEmbedAdapter: Send + Sync {
    fn embed_texts(
        &self,
        texts: Vec<String>,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<Vec<Vec<f64>>, String>> + Send + '_>>;

    fn ndims(&self) -> usize;
}

/// Blanket implementation for any Rig EmbeddingModel.
impl<M> RigEmbedAdapter for M
where
    M: rig_core::EmbeddingModel + Send + Sync,
{
    fn embed_texts(
        &self,
        texts: Vec<String>,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<Vec<Vec<f64>>, String>> + Send + '_>>
    {
        Box::pin(async move {
            let embeddings = rig_core::EmbeddingModel::embed_texts(self, texts)
                .await
                .map_err(|e| e.to_string())?;
            Ok(embeddings.into_iter().map(|e| e.vec.clone()).collect())
        })
    }

    fn ndims(&self) -> usize {
        rig_core::EmbeddingModel::ndims(self)
    }
}

/// Concrete embedding service that dispatches to the configured provider.
pub struct EmbeddingService {
    inner: EmbeddingInner,
    provider_name: String,
    model_id: String,
    dimensions: usize,
}

enum EmbeddingInner {
    /// Rig-backed provider (OpenAI, Ollama, Gemini)
    Rig(Box<dyn RigEmbedAdapter>),
    /// Deterministic hash for testing (no Rig equivalent)
    Hash(HashEmbeddingProvider),
}

impl std::fmt::Debug for EmbeddingService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingService")
            .field("provider", &self.provider_name)
            .field("model", &self.model_id)
            .field("dimensions", &self.dimensions)
            .finish()
    }
}

impl EmbeddingService {
    /// Create an embedding service from configuration.
    pub fn from_config(config: &EmbeddingConfig) -> Result<Self> {
        match config.provider.as_str() {
            "openai" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "OPENAI_API_KEY",
                    "openai",
                    "embedding",
                )?;
                let model = config.model.clone();
                let dims = config.dimensions.unwrap_or(1536);

                let mut builder = rig_core::providers::openai::Client::builder()
                    .api_key(&api_key);
                if let Some(ref base_url) = config.base_url {
                    builder = builder.base_url(base_url);
                }
                let client = builder.build().map_err(|e| {
                    ShabkaError::Embedding(format!("failed to build OpenAI client: {e}"))
                })?;
                let emb_model = client.embedding_model_with_ndims(&model, dims);

                Ok(Self {
                    inner: EmbeddingInner::Rig(Box::new(emb_model)),
                    provider_name: "openai".into(),
                    model_id: model,
                    dimensions: dims,
                })
            }
            "ollama" => {
                let model = if config.model == "hash-128d" {
                    "nomic-embed-text".to_string()
                } else {
                    config.model.clone()
                };
                let dims = config.dimensions.unwrap_or(if model == "nomic-embed-text" {
                    768
                } else {
                    768 // sensible default for Ollama models
                });
                let base_url = config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434".to_string());

                let client = rig_core::providers::ollama::Client::builder()
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        ShabkaError::Embedding(format!("failed to build Ollama client: {e}"))
                    })?;
                let emb_model = client.embedding_model_with_ndims(&model, dims);

                Ok(Self {
                    inner: EmbeddingInner::Rig(Box::new(emb_model)),
                    provider_name: "ollama".into(),
                    model_id: model,
                    dimensions: dims,
                })
            }
            "gemini" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "GEMINI_API_KEY",
                    "gemini",
                    "embedding",
                )?;
                let model = if config.model == "hash-128d" {
                    "text-embedding-004".to_string()
                } else {
                    config.model.clone()
                };
                let dims = config.dimensions.unwrap_or(768);

                let client = rig_core::providers::gemini::Client::builder()
                    .api_key(&api_key)
                    .build()
                    .map_err(|e| {
                        ShabkaError::Embedding(format!("failed to build Gemini client: {e}"))
                    })?;
                let emb_model = client.embedding_model_with_ndims(&model, dims);

                Ok(Self {
                    inner: EmbeddingInner::Rig(Box::new(emb_model)),
                    provider_name: "gemini".into(),
                    model_id: model,
                    dimensions: dims,
                })
            }
            "hash" => Ok(Self {
                inner: EmbeddingInner::Hash(HashEmbeddingProvider::new()),
                provider_name: "hash".into(),
                model_id: "hash-128d".into(),
                dimensions: 128,
            }),
            "local" => Err(ShabkaError::Config(
                "local embedding provider has been removed; use 'ollama' with a local model instead".into(),
            )),
            other => Err(ShabkaError::Config(format!(
                "unknown embedding provider: '{other}' \
                 (expected 'openai', 'ollama', 'gemini', or 'hash')"
            ))),
        }
    }

    /// Whether this provider makes remote API calls (and should use retry logic).
    fn is_remote(&self) -> bool {
        matches!(self.inner, EmbeddingInner::Rig(_))
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        match &self.inner {
            EmbeddingInner::Hash(p) => p.embed(text).await,
            EmbeddingInner::Rig(model) => {
                with_retry(3, 200, || async {
                    let results = model
                        .embed_texts(vec![text.to_string()])
                        .await
                        .map_err(|e| ShabkaError::Embedding(e))?;
                    results
                        .into_iter()
                        .next()
                        .map(|v| v.into_iter().map(|x| x as f32).collect())
                        .ok_or_else(|| {
                            ShabkaError::Embedding("empty embedding result".into())
                        })
                })
                .await
            }
        }
    }

    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        match &self.inner {
            EmbeddingInner::Hash(p) => p.embed_batch(texts).await,
            EmbeddingInner::Rig(model) => {
                with_retry(3, 200, || async {
                    let input: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
                    let results = model
                        .embed_texts(input)
                        .await
                        .map_err(|e| ShabkaError::Embedding(e))?;
                    Ok(results
                        .into_iter()
                        .map(|v| v.into_iter().map(|x| x as f32).collect())
                        .collect())
                })
                .await
            }
        }
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Provider name for display purposes.
    pub fn provider_name(&self) -> &str {
        &self.provider_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EmbeddingConfig;

    #[test]
    fn test_unknown_provider_errors() {
        let config = EmbeddingConfig {
            provider: "nonexistent".to_string(),
            model: "test".to_string(),
            api_key: None,
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown embedding provider"));
    }

    #[test]
    fn test_openai_without_key_errors() {
        let saved = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");

        let config = EmbeddingConfig {
            provider: "openai".to_string(),
            model: "text-embedding-3-small".to_string(),
            api_key: None,
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("API key"));

        if let Some(key) = saved {
            std::env::set_var("OPENAI_API_KEY", key);
        }
    }

    #[test]
    fn test_local_provider_removed() {
        let config = EmbeddingConfig {
            provider: "local".to_string(),
            model: "bge-small-en-v1.5".to_string(),
            api_key: None,
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("removed"));
    }

    #[test]
    fn test_ollama_no_auth_required() {
        let config = EmbeddingConfig {
            provider: "ollama".to_string(),
            model: "nomic-embed-text".to_string(),
            api_key: None,
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_ok());
        let service = result.unwrap();
        assert_eq!(service.model_id(), "nomic-embed-text");
    }

    #[test]
    fn test_gemini_without_key_errors() {
        let saved = std::env::var("GEMINI_API_KEY").ok();
        std::env::remove_var("GEMINI_API_KEY");

        let config = EmbeddingConfig {
            provider: "gemini".to_string(),
            model: "text-embedding-004".to_string(),
            api_key: None,
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("API key"));

        if let Some(key) = saved {
            std::env::set_var("GEMINI_API_KEY", key);
        }
    }

    #[test]
    fn test_openai_custom_base_url() {
        let config = EmbeddingConfig {
            provider: "openai".to_string(),
            model: "BAAI/bge-large-en-v1.5".to_string(),
            api_key: Some("dummy-key".to_string()),
            base_url: Some("http://localhost:8000/v1".to_string()),
            dimensions: Some(1024),
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_ok());
        let service = result.unwrap();
        assert_eq!(service.dimensions(), 1024);
        assert_eq!(service.model_id(), "BAAI/bge-large-en-v1.5");
    }

    #[test]
    fn test_ollama_default_model_override() {
        let config = EmbeddingConfig {
            provider: "ollama".to_string(),
            model: "hash-128d".to_string(),
            api_key: None,
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_ok());
        let service = result.unwrap();
        assert_eq!(service.model_id(), "nomic-embed-text");
    }

    #[test]
    fn test_hash_provider() {
        let config = EmbeddingConfig {
            provider: "hash".to_string(),
            model: "hash-128d".to_string(),
            api_key: None,
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_ok());
        let service = result.unwrap();
        assert_eq!(service.dimensions(), 128);
        assert_eq!(service.model_id(), "hash-128d");
        assert_eq!(service.provider_name(), "hash");
    }

    #[test]
    fn test_resolve_api_key_from_config() {
        let config = EmbeddingConfig {
            provider: "openai".to_string(),
            model: "test".to_string(),
            api_key: Some("config-key".to_string()),
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let key = config::resolve_api_key(
            config.api_key.as_deref(),
            config.env_var.as_deref(),
            "OPENAI_API_KEY",
            "openai",
            "embedding",
        )
        .unwrap();
        assert_eq!(key, "config-key");
    }

    #[test]
    fn test_resolve_api_key_custom_env_var() {
        std::env::set_var("MY_CUSTOM_KEY", "env-key");
        let config = EmbeddingConfig {
            provider: "openai".to_string(),
            model: "test".to_string(),
            api_key: None,
            base_url: None,
            dimensions: None,
            env_var: Some("MY_CUSTOM_KEY".to_string()),
        };
        let key = config::resolve_api_key(
            config.api_key.as_deref(),
            config.env_var.as_deref(),
            "OPENAI_API_KEY",
            "openai",
            "embedding",
        )
        .unwrap();
        assert_eq!(key, "env-key");
        std::env::remove_var("MY_CUSTOM_KEY");
    }
}
```

**Step 3: Verify tests pass**

Run: `cargo test -p shabka-core --no-default-features --lib embedding`
Expected: PASS — all embedding tests pass with the new implementation

**Step 4: Verify full test suite**

Run: `cargo test -p shabka-core --no-default-features`
Expected: PASS — 250 tests (some test counts may change slightly due to removed local provider test)

**Step 5: Commit**

```bash
git add -u crates/shabka-core/src/embedding/
git commit -m "refactor(embedding): replace hand-rolled providers with Rig

Delete openai.rs, gemini.rs, local.rs (~270 lines).
EmbeddingService now wraps Rig's EmbeddingModel via a thin
RigEmbedAdapter trait for object safety. Hash provider unchanged.
f64->f32 conversion at boundary. Same public API."
```

---

### Task 4: Rewrite LlmService to use Rig internally

**Files:**
- Rewrite: `crates/shabka-core/src/llm.rs`

**Step 1: Write the new LlmService**

The key design: we create our own `RigCompletionAdapter` trait (object-safe via Pin<Box<Future>>) and implement it for each Rig provider's CompletionModel. Then `LlmService` stores a `Box<dyn RigCompletionAdapter>`.

Replace the entire `crates/shabka-core/src/llm.rs` with:

```rust
use crate::config::{self, LlmConfig};
use crate::error::{Result, ShabkaError};
use crate::retry::with_retry;

use std::future::Future;
use std::pin::Pin;

/// Object-safe wrapper trait for Rig completion models.
trait RigCompletionAdapter: Send + Sync {
    fn generate(
        &self,
        prompt: &str,
        system: Option<&str>,
        max_tokens: usize,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send + '_>>;
}

/// Blanket implementation for any Rig CompletionModel.
/// Uses Rig's `completion_request` builder to construct the request.
impl<M> RigCompletionAdapter for M
where
    M: rig_core::completion::CompletionModel + Send + Sync,
    M::Response: Send,
{
    fn generate(
        &self,
        prompt: &str,
        system: Option<&str>,
        max_tokens: usize,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send + '_>>
    {
        let prompt = prompt.to_string();
        let system = system.map(|s| s.to_string());
        let max_tokens = max_tokens;

        Box::pin(async move {
            use rig_core::message::{Message, UserContent};
            use rig_core::one_or_many::OneOrMany;

            let user_msg = Message::User {
                content: OneOrMany::one(UserContent::text(&prompt)),
                name: None,
            };

            let request = rig_core::completion::CompletionRequest {
                prompt: user_msg,
                preamble: system,
                chat_history: vec![],
                temperature: None,
                max_tokens: Some(max_tokens),
                additional_params: None,
                tools: vec![],
            };

            let response = rig_core::completion::CompletionModel::completion(self, request)
                .await
                .map_err(|e| e.to_string())?;

            // Extract text from the response choice
            let text = response
                .choice
                .first_text()
                .ok_or_else(|| "LLM response contained no text content".to_string())?;

            Ok(text.to_string())
        })
    }
}

/// LLM text generation service backed by Rig providers.
pub struct LlmService {
    inner: Box<dyn RigCompletionAdapter>,
    config: LlmConfig,
}

impl std::fmt::Debug for LlmService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmService")
            .field("provider", &self.config.provider)
            .field("model", &self.config.model)
            .finish()
    }
}

impl LlmService {
    /// Create an LLM service from configuration.
    pub fn from_config(config: &LlmConfig) -> Result<Self> {
        let inner: Box<dyn RigCompletionAdapter> = match config.provider.as_str() {
            "ollama" => {
                let base_url = config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434".to_string());

                let client = rig_core::providers::ollama::Client::builder()
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| ShabkaError::Llm(format!("failed to build Ollama client: {e}")))?;

                Box::new(client.completion_model(&config.model))
            }
            "openai" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "OPENAI_API_KEY",
                    "openai",
                    "LLM",
                )?;

                let mut builder = rig_core::providers::openai::Client::builder()
                    .api_key(&api_key);
                if let Some(ref base_url) = config.base_url {
                    builder = builder.base_url(base_url);
                }
                let client = builder.build().map_err(|e| {
                    ShabkaError::Llm(format!("failed to build OpenAI client: {e}"))
                })?;

                Box::new(client.completion_model(&config.model))
            }
            "gemini" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "GEMINI_API_KEY",
                    "gemini",
                    "LLM",
                )?;

                let client = rig_core::providers::gemini::Client::builder()
                    .api_key(&api_key)
                    .build()
                    .map_err(|e| {
                        ShabkaError::Llm(format!("failed to build Gemini client: {e}"))
                    })?;

                Box::new(client.completion_model(&config.model))
            }
            "anthropic" | "claude" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "ANTHROPIC_API_KEY",
                    "anthropic",
                    "LLM",
                )?;

                let mut builder = rig_core::providers::anthropic::Client::builder()
                    .api_key(&api_key);
                if let Some(ref base_url) = config.base_url {
                    builder = builder.base_url(base_url);
                }
                let client = builder.build().map_err(|e| {
                    ShabkaError::Llm(format!("failed to build Anthropic client: {e}"))
                })?;

                Box::new(client.completion_model(&config.model))
            }
            other => {
                return Err(ShabkaError::Config(format!(
                    "unknown LLM provider: '{other}' (expected 'ollama', 'openai', 'gemini', or 'anthropic')"
                )));
            }
        };

        Ok(Self {
            inner,
            config: config.clone(),
        })
    }

    /// Generate text from a prompt with an optional system message.
    /// Wraps the Rig call with retry logic for transient errors.
    pub async fn generate(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        with_retry(3, 200, || async {
            self.inner
                .generate(prompt, system, self.config.max_tokens)
                .await
                .map_err(|e| ShabkaError::Llm(e))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_config_ollama() {
        let config = LlmConfig {
            enabled: true,
            provider: "ollama".into(),
            model: "llama3.2".into(),
            ..Default::default()
        };
        let service = LlmService::from_config(&config);
        assert!(service.is_ok());
    }

    #[test]
    fn test_from_config_unknown_provider() {
        let config = LlmConfig {
            provider: "banana".into(),
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unknown LLM provider"));
    }

    #[test]
    fn test_from_config_openai_without_key_errors() {
        let saved = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");

        let config = LlmConfig {
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
            api_key: None,
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key"));

        if let Some(key) = saved {
            std::env::set_var("OPENAI_API_KEY", key);
        }
    }

    #[test]
    fn test_from_config_openai_with_key() {
        let config = LlmConfig {
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
            api_key: Some("sk-test".into()),
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_config_gemini_without_key_errors() {
        let saved = std::env::var("GEMINI_API_KEY").ok();
        std::env::remove_var("GEMINI_API_KEY");

        let config = LlmConfig {
            provider: "gemini".into(),
            model: "gemini-2.0-flash".into(),
            api_key: None,
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_err());

        if let Some(key) = saved {
            std::env::set_var("GEMINI_API_KEY", key);
        }
    }

    #[test]
    fn test_from_config_anthropic_without_key_errors() {
        let saved = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");

        let config = LlmConfig {
            provider: "anthropic".into(),
            model: "claude-sonnet-4-5-20250929".into(),
            api_key: None,
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key"));

        if let Some(key) = saved {
            std::env::set_var("ANTHROPIC_API_KEY", key);
        }
    }

    #[test]
    fn test_from_config_anthropic_with_key() {
        let config = LlmConfig {
            provider: "anthropic".into(),
            model: "claude-sonnet-4-5-20250929".into(),
            api_key: Some("sk-ant-test".into()),
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_config_claude_alias() {
        let config = LlmConfig {
            provider: "claude".into(),
            model: "claude-sonnet-4-5-20250929".into(),
            api_key: Some("sk-ant-test".into()),
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_api_key_from_config() {
        let config = LlmConfig {
            provider: "openai".into(),
            api_key: Some("config-key".into()),
            ..Default::default()
        };
        let key = config::resolve_api_key(
            config.api_key.as_deref(),
            config.env_var.as_deref(),
            "OPENAI_API_KEY",
            "openai",
            "LLM",
        )
        .unwrap();
        assert_eq!(key, "config-key");
    }

    #[test]
    fn test_resolve_api_key_custom_env_var() {
        std::env::set_var("MY_LLM_KEY", "env-llm-key");
        let config = LlmConfig {
            provider: "openai".into(),
            api_key: None,
            env_var: Some("MY_LLM_KEY".into()),
            ..Default::default()
        };
        let key = config::resolve_api_key(
            config.api_key.as_deref(),
            config.env_var.as_deref(),
            "OPENAI_API_KEY",
            "openai",
            "LLM",
        )
        .unwrap();
        assert_eq!(key, "env-llm-key");
        std::env::remove_var("MY_LLM_KEY");
    }
}
```

**Step 2: Verify tests pass**

Run: `cargo test -p shabka-core --no-default-features --lib llm::tests`
Expected: PASS — all 10 LLM tests pass

**Step 3: Verify full test suite**

Run: `cargo test -p shabka-core --no-default-features`
Expected: PASS — all tests

**Step 4: Commit**

```bash
git add crates/shabka-core/src/llm.rs
git commit -m "refactor(llm): replace hand-rolled HTTP with Rig CompletionModel

Delete 4 generate methods (~300 lines). LlmService now wraps
Rig via RigCompletionAdapter trait. Uses ShabkaError::Llm (not
Embedding). Adds retry with exponential backoff (was missing)."
```

---

### Task 5: Update config VALID_PROVIDERS and clean up embed-local references

**Files:**
- Modify: `crates/shabka-core/src/config/mod.rs`
- Modify: `crates/shabka-mcp/Cargo.toml`
- Modify: `crates/shabka-hooks/Cargo.toml` (if it has embed-local feature)
- Modify: `crates/shabka-cli/Cargo.toml` (if it has embed-local feature)
- Modify: `crates/shabka-web/Cargo.toml` (if it has embed-local feature)

**Step 1: Update VALID_PROVIDERS**

In `crates/shabka-core/src/config/mod.rs`, change:

```rust
pub const VALID_PROVIDERS: &[&str] = &["hash", "ollama", "openai", "gemini", "local"];
```

to:

```rust
pub const VALID_PROVIDERS: &[&str] = &["hash", "ollama", "openai", "gemini"];
```

**Step 2: Remove embed-local feature from all downstream crates**

Check each crate's `Cargo.toml` for `embed-local` feature references and remove them:

- `crates/shabka-mcp/Cargo.toml`: Remove `[features]` section with `embed-local`
- Other crates: similarly remove if present

**Step 3: Remove old resolve_api_key from embedding/mod.rs**

The old local `resolve_api_key` function in `embedding/mod.rs` was replaced in Task 3. Verify it's gone.

**Step 4: Verify full workspace compiles and tests pass**

Run: `cargo check --workspace --no-default-features`
Run: `cargo test -p shabka-core --no-default-features`
Run: `cargo test -p shabka-hooks --no-default-features`
Expected: All PASS

**Step 5: Commit**

```bash
git add -u
git commit -m "chore: remove embed-local feature, update VALID_PROVIDERS"
```

---

### Task 6: Run clippy and fix warnings

**Step 1: Run clippy across workspace**

Run: `cargo clippy --workspace --no-default-features -- -D warnings`

**Step 2: Fix any warnings**

Common expected warnings:
- Unused imports from removed modules
- Dead code from removed providers
- Type conversion suggestions for `as f32`

**Step 3: Run full test suite one final time**

Run: `cargo test -p shabka-core --no-default-features && cargo test -p shabka-hooks --no-default-features`
Expected: All PASS

**Step 4: Commit**

```bash
git add -u
git commit -m "chore: fix clippy warnings after Rig integration"
```

---

### Task 7: Final validation and release commit

**Step 1: Run the full check suite**

Run: `just check` (or `cargo clippy --workspace --no-default-features -- -D warnings && cargo test -p shabka-core --no-default-features`)

**Step 2: Verify the dependency tree is clean**

Run: `cargo tree -p shabka-core --no-default-features | head -30`
Verify: `rig-core` appears, `fastembed` does not

**Step 3: Squash or keep commits as-is**

The task produced 5 clean commits:
1. `build: add rig-core 0.31, remove fastembed dependency`
2. `refactor: add ShabkaError::Llm variant, shared resolve_api_key`
3. `refactor(embedding): replace hand-rolled providers with Rig`
4. `refactor(llm): replace hand-rolled HTTP with Rig CompletionModel`
5. `chore: remove embed-local feature, update VALID_PROVIDERS`
6. `chore: fix clippy warnings after Rig integration`

---

## Critical Notes for the Implementer

### Rig API Gotchas

1. **Rig's `EmbeddingModel::embed_texts` returns `Vec<Embedding>`** where `Embedding` has a `.vec` field of type `Vec<f64>`. You need to access `.vec` to get the raw vector, then cast `as f32`.

2. **Rig's `CompletionModel::completion` returns `CompletionResponse`** with a `.choice` field. Use `.choice.first_text()` to get the text content. If the model returns a tool call instead of text, `first_text()` returns `None`.

3. **The blanket `impl<M> RigCompletionAdapter for M` requires `M::Response: Send`** — this should work for all standard providers but check the compiler if it complains.

4. **Rig's `Client::builder()` pattern** — `build()` returns a `Result`. Don't unwrap — map the error to `ShabkaError`.

5. **Ollama doesn't need an API key** — use `Client::builder().base_url(url).build()` without `.api_key()`. The Ollama provider uses `Nothing` type for auth.

### What NOT to Change

- `embedding/hash.rs` — stays as-is
- `embedding/provider.rs` — stays as-is (Hash implements it)
- All call sites (dedup.rs, consolidate.rs, auto_tag.rs, hooks, MCP, web, CLI) — same public API
- Config TOML format — all user configs continue to work
- Test assertions — same expected behavior

### If Rig's API Doesn't Match

The plan is written against Rig 0.31 based on docs and Context7. If the actual API differs (method names, type signatures), adapt accordingly — the APPROACH (thin adapter with object-safe wrapper traits) stays the same, only the specific Rig calls may need adjustment. Use `cargo doc --open -p rig-core` to browse the actual API locally.
