mod provider;

mod hash;

pub use hash::HashEmbeddingProvider;
pub use provider::EmbeddingProvider;

use crate::config::{self, EmbeddingConfig};
use crate::error::{Result, ShabkaError};
use crate::retry::with_retry;
use std::future::Future;
use std::pin::Pin;

/// Boxed future returning embed results — avoids `clippy::type_complexity` on the trait.
type EmbedFuture<'a> =
    Pin<Box<dyn Future<Output = std::result::Result<Vec<Vec<f64>>, String>> + Send + 'a>>;

// ---------------------------------------------------------------------------
// Object-safe adapter around Rig's EmbeddingModel trait
// ---------------------------------------------------------------------------

/// Object-safe wrapper for Rig's `EmbeddingModel`.
///
/// Rig's `EmbeddingModel` trait is not dyn-compatible (associated type `Client`,
/// `impl IntoIterator` parameter).  This thin adapter erases those, letting
/// `EmbeddingService` store any Rig model behind `Box<dyn RigEmbedAdapter>`.
trait RigEmbedAdapter: Send + Sync {
    /// Embed one or more texts, returning f64 vectors (Rig's native precision).
    fn embed_texts(&self, texts: Vec<String>) -> EmbedFuture<'_>;

    /// Model identifier string.
    fn model_id(&self) -> &str;
}

/// Blanket implementation: any concrete Rig `EmbeddingModel` can be used as
/// a `RigEmbedAdapter` provided its generic HTTP client type satisfies the
/// necessary bounds.
impl<M> RigEmbedAdapter for RigModelWrapper<M>
where
    M: rig::embeddings::EmbeddingModel + Send + Sync + 'static,
{
    fn embed_texts(&self, texts: Vec<String>) -> EmbedFuture<'_> {
        Box::pin(async move {
            let embeddings = self
                .model
                .embed_texts(texts)
                .await
                .map_err(|e| e.to_string())?;
            Ok(embeddings.into_iter().map(|e| e.vec).collect())
        })
    }

    fn model_id(&self) -> &str {
        &self.model_name
    }
}

/// Wrapper that pairs a Rig model with its string name (for `model_id()`).
struct RigModelWrapper<M> {
    model: M,
    model_name: String,
}

// ---------------------------------------------------------------------------
// EmbeddingService — public API (unchanged from callers' perspective)
// ---------------------------------------------------------------------------

enum EmbeddingInner {
    /// Any Rig-backed remote provider (OpenAI, Ollama, Gemini).
    Rig(Box<dyn RigEmbedAdapter>),
    /// Local deterministic hash provider (no network).
    Hash(HashEmbeddingProvider),
}

/// Concrete embedding service that dispatches to the configured provider.
pub struct EmbeddingService {
    inner: EmbeddingInner,
    provider: &'static str,
    dimensions: usize,
}

impl std::fmt::Debug for EmbeddingService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingService")
            .field("provider", &self.provider)
            .finish()
    }
}

impl EmbeddingService {
    /// Create an embedding service from configuration.
    pub fn from_config(config: &EmbeddingConfig) -> Result<Self> {
        match config.provider.as_str() {
            "local" => Err(ShabkaError::Config(
                "local embedding provider has been removed; use 'ollama', 'openai', 'gemini', 'cohere', or 'hash'".into(),
            )),

            "openai" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "OPENAI_API_KEY",
                    "openai",
                    "embedding",
                )?;

                let model_name = config.model.clone();
                let dims = config.dimensions.unwrap_or(1536);

                let mut builder =
                    rig::providers::openai::Client::<reqwest::Client>::builder()
                        .api_key(&api_key);

                if let Some(ref base_url) = config.base_url {
                    builder = builder.base_url(base_url);
                }

                let client = builder.build().map_err(|e| {
                    ShabkaError::Embedding(format!("failed to build OpenAI client: {e}"))
                })?;

                use rig::prelude::EmbeddingsClient;
                let model = client.embedding_model_with_ndims(&model_name, dims);

                Ok(Self {
                    inner: EmbeddingInner::Rig(Box::new(RigModelWrapper {
                        model,
                        model_name,
                    })),
                    provider: "openai",
                    dimensions: dims,
                })
            }

            "ollama" => {
                let model_name = if config.model == "hash-128d" {
                    "nomic-embed-text".to_string()
                } else {
                    config.model.clone()
                };

                let base_url = config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434".to_string());

                // Default to 768d — nomic-embed-text and most Ollama models use this
                let dims = config.dimensions.unwrap_or(768);

                let client =
                    rig::providers::ollama::Client::<reqwest::Client>::builder()
                        .api_key(rig::client::Nothing)
                        .base_url(&base_url)
                        .build()
                        .map_err(|e| {
                            ShabkaError::Embedding(format!("failed to build Ollama client: {e}"))
                        })?;

                use rig::prelude::EmbeddingsClient;
                let model = client.embedding_model_with_ndims(&model_name, dims);

                Ok(Self {
                    inner: EmbeddingInner::Rig(Box::new(RigModelWrapper {
                        model,
                        model_name,
                    })),
                    provider: "ollama",
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

                let model_name = if config.model == "hash-128d" {
                    "text-embedding-004".to_string()
                } else {
                    config.model.clone()
                };

                let dims = config.dimensions.unwrap_or(768);

                let mut builder =
                    rig::providers::gemini::Client::<reqwest::Client>::builder()
                        .api_key(&api_key);

                if let Some(ref base_url) = config.base_url {
                    builder = builder.base_url(base_url);
                }

                let client = builder.build().map_err(|e| {
                    ShabkaError::Embedding(format!("failed to build Gemini client: {e}"))
                })?;

                use rig::prelude::EmbeddingsClient;
                let model = client.embedding_model_with_ndims(&model_name, dims);

                Ok(Self {
                    inner: EmbeddingInner::Rig(Box::new(RigModelWrapper {
                        model,
                        model_name,
                    })),
                    provider: "gemini",
                    dimensions: dims,
                })
            }

            "cohere" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "COHERE_API_KEY",
                    "cohere",
                    "embedding",
                )?;

                let model_name = if config.model == "hash-128d" {
                    "embed-english-v3.0".to_string()
                } else {
                    config.model.clone()
                };

                let dims = config.dimensions.unwrap_or(1024);

                let client =
                    rig::providers::cohere::Client::<reqwest::Client>::builder()
                        .api_key(&api_key)
                        .build()
                        .map_err(|e| {
                            ShabkaError::Embedding(format!("failed to build Cohere client: {e}"))
                        })?;

                let model = client.embedding_model_with_ndims(&model_name, "search_document", dims);

                Ok(Self {
                    inner: EmbeddingInner::Rig(Box::new(RigModelWrapper {
                        model,
                        model_name,
                    })),
                    provider: "cohere",
                    dimensions: dims,
                })
            }

            "hash" => Ok(Self {
                inner: EmbeddingInner::Hash(HashEmbeddingProvider::new()),
                provider: "hash",
                dimensions: 128,
            }),

            other => Err(ShabkaError::Config(format!(
                "unknown embedding provider: '{other}' \
                 (expected 'openai', 'ollama', 'gemini', 'cohere', or 'hash')"
            ))),
        }
    }

    /// Whether this provider makes remote API calls (and should use retry logic).
    fn is_remote(&self) -> bool {
        matches!(self.inner, EmbeddingInner::Rig(_))
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        if self.is_remote() {
            return with_retry(3, 200, || async {
                match &self.inner {
                    EmbeddingInner::Rig(adapter) => {
                        let vecs = adapter
                            .embed_texts(vec![text.to_string()])
                            .await
                            .map_err(ShabkaError::Embedding)?;
                        vecs.into_iter()
                            .next()
                            .map(|v| v.into_iter().map(|x| x as f32).collect())
                            .ok_or_else(|| ShabkaError::Embedding("empty embedding result".into()))
                    }
                    _ => Err(ShabkaError::Embedding(
                        "unexpected non-remote variant in remote embed path".into(),
                    )),
                }
            })
            .await;
        }
        match &self.inner {
            EmbeddingInner::Hash(p) => p.embed(text).await,
            _ => Err(ShabkaError::Embedding(
                "unexpected non-local variant in local embed path".into(),
            )),
        }
    }

    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if self.is_remote() {
            return with_retry(3, 200, || async {
                match &self.inner {
                    EmbeddingInner::Rig(adapter) => {
                        let owned: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
                        let vecs = adapter
                            .embed_texts(owned)
                            .await
                            .map_err(ShabkaError::Embedding)?;
                        Ok(vecs
                            .into_iter()
                            .map(|v| v.into_iter().map(|x| x as f32).collect())
                            .collect())
                    }
                    _ => Err(ShabkaError::Embedding(
                        "unexpected non-remote variant in remote embed path".into(),
                    )),
                }
            })
            .await;
        }
        match &self.inner {
            EmbeddingInner::Hash(p) => p.embed_batch(texts).await,
            _ => Err(ShabkaError::Embedding(
                "unexpected non-local variant in local embed path".into(),
            )),
        }
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    pub fn model_id(&self) -> &str {
        match &self.inner {
            EmbeddingInner::Rig(adapter) => adapter.model_id(),
            EmbeddingInner::Hash(p) => p.model_id(),
        }
    }

    /// Provider name for display purposes.
    pub fn provider_name(&self) -> &str {
        self.provider
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
        // Unset the env var for this test
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

        // Restore env var
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
        // When model is still the default "hash-128d", ollama should use "nomic-embed-text"
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
    fn test_cohere_without_key_errors() {
        let saved = std::env::var("COHERE_API_KEY").ok();
        std::env::remove_var("COHERE_API_KEY");

        let config = EmbeddingConfig {
            provider: "cohere".to_string(),
            model: "embed-english-v3.0".to_string(),
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
            std::env::set_var("COHERE_API_KEY", key);
        }
    }

    #[test]
    fn test_cohere_with_key() {
        let config = EmbeddingConfig {
            provider: "cohere".to_string(),
            model: "embed-english-v3.0".to_string(),
            api_key: Some("co-test-key".to_string()),
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_ok());
        let service = result.unwrap();
        assert_eq!(service.provider_name(), "cohere");
        assert_eq!(service.dimensions(), 1024);
        assert_eq!(service.model_id(), "embed-english-v3.0");
    }

    #[test]
    fn test_cohere_default_model_override() {
        let config = EmbeddingConfig {
            provider: "cohere".to_string(),
            model: "hash-128d".to_string(),
            api_key: Some("co-test-key".to_string()),
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let result = EmbeddingService::from_config(&config);
        assert!(result.is_ok());
        let service = result.unwrap();
        assert_eq!(service.model_id(), "embed-english-v3.0");
    }

    #[test]
    fn test_resolve_api_key_from_config() {
        let key = config::resolve_api_key(
            Some("config-key"),
            None,
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
        let key = config::resolve_api_key(
            None,
            Some("MY_CUSTOM_KEY"),
            "OPENAI_API_KEY",
            "openai",
            "embedding",
        )
        .unwrap();
        assert_eq!(key, "env-key");
        std::env::remove_var("MY_CUSTOM_KEY");
    }
}
