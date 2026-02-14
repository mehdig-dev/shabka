mod provider;

#[cfg(feature = "embed-local")]
mod local;

mod gemini;
mod hash;
mod openai;

pub use provider::EmbeddingProvider;

#[cfg(feature = "embed-local")]
pub use local::LocalEmbeddingProvider;

pub use gemini::GeminiEmbeddingProvider;
pub use hash::HashEmbeddingProvider;
pub use openai::OpenAIEmbeddingProvider;

use crate::config::EmbeddingConfig;
use crate::error::{Result, ShabkaError};
use crate::retry::with_retry;

/// Concrete embedding service that dispatches to the configured provider.
/// Uses an enum instead of `dyn EmbeddingProvider` because the trait uses RPITIT.
pub enum EmbeddingService {
    #[cfg(feature = "embed-local")]
    Local(LocalEmbeddingProvider),
    OpenAI(OpenAIEmbeddingProvider),
    /// Ollama uses OpenAI-compatible API but needs a distinct variant for display.
    Ollama(OpenAIEmbeddingProvider),
    Gemini(GeminiEmbeddingProvider),
    Hash(HashEmbeddingProvider),
}

impl std::fmt::Debug for EmbeddingService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "embed-local")]
            Self::Local(_) => f.debug_tuple("Local").finish(),
            Self::OpenAI(_) => f.debug_tuple("OpenAI").finish(),
            Self::Ollama(_) => f.debug_tuple("Ollama").finish(),
            Self::Gemini(_) => f.debug_tuple("Gemini").finish(),
            Self::Hash(_) => f.debug_tuple("Hash").finish(),
        }
    }
}

/// Resolve an API key from config, a custom env var, or a default env var.
fn resolve_api_key(config: &EmbeddingConfig, default_env_var: &str) -> Result<String> {
    if let Some(ref key) = config.api_key {
        if !key.is_empty() {
            return Ok(key.clone());
        }
    }

    let env_var_name = config.env_var.as_deref().unwrap_or(default_env_var);

    std::env::var(env_var_name).map_err(|_| {
        ShabkaError::Config(format!(
            "{} embedding provider requires an API key \
             (set embedding.api_key or {})",
            config.provider, env_var_name
        ))
    })
}

impl EmbeddingService {
    /// Create an embedding service from configuration.
    pub fn from_config(config: &EmbeddingConfig) -> Result<Self> {
        match config.provider.as_str() {
            #[cfg(feature = "embed-local")]
            "local" => {
                let provider = LocalEmbeddingProvider::new()?;
                Ok(Self::Local(provider))
            }
            #[cfg(not(feature = "embed-local"))]
            "local" => Err(ShabkaError::Config(
                "local embedding provider requires the 'embed-local' feature".into(),
            )),
            "openai" => {
                let api_key = resolve_api_key(config, "OPENAI_API_KEY")?;
                let model = config.model.clone();
                Ok(Self::OpenAI(OpenAIEmbeddingProvider::with_config(
                    api_key,
                    model,
                    config.dimensions,
                    config.base_url.clone(),
                )))
            }
            "ollama" => {
                // Ollama uses OpenAI-compatible API — no auth required
                let api_key = config
                    .api_key
                    .clone()
                    .or_else(|| {
                        config
                            .env_var
                            .as_deref()
                            .and_then(|var| std::env::var(var).ok())
                    })
                    .unwrap_or_default();

                let model = if config.model == "hash-128d" {
                    "nomic-embed-text".to_string()
                } else {
                    config.model.clone()
                };

                let base_url = config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434/v1".to_string());

                // Default to 768d for nomic-embed-text; Ollama models typically aren't 1536d
                let dimensions = config.dimensions.or_else(|| {
                    if model == "nomic-embed-text" {
                        Some(768)
                    } else {
                        None
                    }
                });

                Ok(Self::Ollama(OpenAIEmbeddingProvider::with_config(
                    api_key,
                    model,
                    dimensions,
                    Some(base_url),
                )))
            }
            "gemini" => {
                let api_key = resolve_api_key(config, "GEMINI_API_KEY")?;
                let model = if config.model == "hash-128d" {
                    None
                } else {
                    Some(config.model.clone())
                };
                Ok(Self::Gemini(GeminiEmbeddingProvider::with_config(
                    api_key,
                    model,
                    config.dimensions,
                    config.base_url.clone(),
                )))
            }
            "hash" => Ok(Self::Hash(HashEmbeddingProvider::new())),
            other => Err(ShabkaError::Config(format!(
                "unknown embedding provider: '{other}' \
                 (expected 'local', 'openai', 'ollama', 'gemini', or 'hash')"
            ))),
        }
    }

    /// Whether this provider makes remote API calls (and should use retry logic).
    fn is_remote(&self) -> bool {
        match self {
            #[cfg(feature = "embed-local")]
            Self::Local(_) => false,
            Self::OpenAI(_) | Self::Ollama(_) => true,
            Self::Gemini(_) => true,
            Self::Hash(_) => false,
        }
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        if self.is_remote() {
            return with_retry(3, 200, || async {
                match self {
                    Self::OpenAI(p) | Self::Ollama(p) => p.embed(text).await,
                    Self::Gemini(p) => p.embed(text).await,
                    _ => Err(ShabkaError::Embedding(
                        "unexpected non-remote variant in remote embed path".into(),
                    )),
                }
            })
            .await;
        }
        match self {
            #[cfg(feature = "embed-local")]
            Self::Local(p) => p.embed(text).await,
            Self::Hash(p) => p.embed(text).await,
            _ => Err(ShabkaError::Embedding(
                "unexpected non-local variant in local embed path".into(),
            )),
        }
    }

    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if self.is_remote() {
            return with_retry(3, 200, || async {
                match self {
                    Self::OpenAI(p) | Self::Ollama(p) => p.embed_batch(texts).await,
                    Self::Gemini(p) => p.embed_batch(texts).await,
                    _ => Err(ShabkaError::Embedding(
                        "unexpected non-remote variant in remote embed path".into(),
                    )),
                }
            })
            .await;
        }
        match self {
            #[cfg(feature = "embed-local")]
            Self::Local(p) => p.embed_batch(texts).await,
            Self::Hash(p) => p.embed_batch(texts).await,
            _ => Err(ShabkaError::Embedding(
                "unexpected non-local variant in local embed path".into(),
            )),
        }
    }

    pub fn dimensions(&self) -> usize {
        match self {
            #[cfg(feature = "embed-local")]
            Self::Local(p) => p.dimensions(),
            Self::OpenAI(p) | Self::Ollama(p) => p.dimensions(),
            Self::Gemini(p) => p.dimensions(),
            Self::Hash(p) => p.dimensions(),
        }
    }

    pub fn model_id(&self) -> &str {
        match self {
            #[cfg(feature = "embed-local")]
            Self::Local(p) => p.model_id(),
            Self::OpenAI(p) | Self::Ollama(p) => p.model_id(),
            Self::Gemini(p) => p.model_id(),
            Self::Hash(p) => p.model_id(),
        }
    }

    /// Provider name for display purposes.
    pub fn provider_name(&self) -> &str {
        match self {
            #[cfg(feature = "embed-local")]
            Self::Local(_) => "local",
            Self::OpenAI(_) => "openai",
            Self::Ollama(_) => "ollama",
            Self::Gemini(_) => "gemini",
            Self::Hash(_) => "hash",
        }
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

    #[cfg(not(feature = "embed-local"))]
    #[test]
    fn test_local_without_feature_errors() {
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
        assert!(err.contains("embed-local"));
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
    fn test_resolve_api_key_from_config() {
        let config = EmbeddingConfig {
            provider: "openai".to_string(),
            model: "test".to_string(),
            api_key: Some("config-key".to_string()),
            base_url: None,
            dimensions: None,
            env_var: None,
        };
        let key = resolve_api_key(&config, "OPENAI_API_KEY").unwrap();
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
        let key = resolve_api_key(&config, "OPENAI_API_KEY").unwrap();
        assert_eq!(key, "env-key");
        std::env::remove_var("MY_CUSTOM_KEY");
    }
}
