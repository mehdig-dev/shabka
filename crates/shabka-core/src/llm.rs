use crate::config::{self, LlmConfig};
use crate::error::{Result, ShabkaError};
use crate::retry::with_retry;
use std::future::Future;
use std::pin::Pin;

/// Boxed future returning generated text — avoids `clippy::type_complexity` on the trait.
type GenerateFuture<'a> =
    Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send + 'a>>;

// ---------------------------------------------------------------------------
// Object-safe adapter around Rig's CompletionModel trait
// ---------------------------------------------------------------------------

/// Object-safe wrapper for Rig's `CompletionModel`.
///
/// Rig's `CompletionModel` trait is not dyn-compatible (associated types
/// `Response`, `StreamingResponse`, `Client`). This thin adapter erases
/// those, letting `LlmService` store any Rig model behind
/// `Box<dyn RigCompletionAdapter>`.
trait RigCompletionAdapter: Send + Sync {
    /// Generate text from a prompt with an optional system message.
    fn generate(
        &self,
        prompt: String,
        system: Option<String>,
        max_tokens: u64,
    ) -> GenerateFuture<'_>;
}

/// Wrapper that pairs a Rig completion model with its string name.
struct RigCompletionWrapper<M> {
    model: M,
    #[allow(dead_code)]
    model_name: String,
}

/// Blanket implementation: any concrete Rig `CompletionModel` can be used as
/// a `RigCompletionAdapter` provided its generic HTTP client type satisfies the
/// necessary bounds.
impl<M> RigCompletionAdapter for RigCompletionWrapper<M>
where
    M: rig::completion::CompletionModel + Send + Sync + 'static,
{
    fn generate(
        &self,
        prompt: String,
        system: Option<String>,
        max_tokens: u64,
    ) -> GenerateFuture<'_> {
        Box::pin(async move {
            use rig::completion::AssistantContent;

            let mut builder = self.model.completion_request(prompt);
            if let Some(sys) = system {
                builder = builder.preamble(sys);
            }
            builder = builder.max_tokens(max_tokens);

            let response = builder.send().await.map_err(|e| e.to_string())?;

            // response.choice is OneOrMany<AssistantContent>
            // Extract the first text content by pattern matching
            let text = response
                .choice
                .iter()
                .find_map(|content| match content {
                    AssistantContent::Text(t) => Some(t.text.clone()),
                    _ => None,
                })
                .ok_or_else(|| "completion response contained no text content".to_string())?;

            Ok(text)
        })
    }
}

// ---------------------------------------------------------------------------
// LlmService — public API (unchanged from callers' perspective)
// ---------------------------------------------------------------------------

/// LLM text generation service. Uses Rig's CompletionModel under the hood
/// to support Ollama, OpenAI, Gemini, and Anthropic providers.
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

                let client: rig::providers::ollama::Client<reqwest::Client> =
                    rig::providers::ollama::Client::<reqwest::Client>::builder()
                        .api_key(rig::client::Nothing)
                        .base_url(&base_url)
                        .build()
                        .map_err(|e| {
                            ShabkaError::Llm(format!("failed to build Ollama LLM client: {e}"))
                        })?;

                use rig::prelude::CompletionClient;
                let model = client.completion_model(&config.model);
                let model_name = config.model.clone();

                Box::new(RigCompletionWrapper { model, model_name })
            }

            "openai" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "OPENAI_API_KEY",
                    "openai",
                    "LLM",
                )?;

                let mut builder =
                    rig::providers::openai::Client::<reqwest::Client>::builder().api_key(&api_key);

                if let Some(ref base_url) = config.base_url {
                    builder = builder.base_url(base_url);
                }

                let client = builder.build().map_err(|e| {
                    ShabkaError::Llm(format!("failed to build OpenAI LLM client: {e}"))
                })?;

                use rig::prelude::CompletionClient;
                let model = client.completion_model(&config.model);
                let model_name = config.model.clone();

                Box::new(RigCompletionWrapper { model, model_name })
            }

            "gemini" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "GEMINI_API_KEY",
                    "gemini",
                    "LLM",
                )?;

                let mut builder =
                    rig::providers::gemini::Client::<reqwest::Client>::builder().api_key(&api_key);

                if let Some(ref base_url) = config.base_url {
                    builder = builder.base_url(base_url);
                }

                let client = builder.build().map_err(|e| {
                    ShabkaError::Llm(format!("failed to build Gemini LLM client: {e}"))
                })?;

                use rig::prelude::CompletionClient;
                let model = client.completion_model(&config.model);
                let model_name = config.model.clone();

                Box::new(RigCompletionWrapper { model, model_name })
            }

            "anthropic" | "claude" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "ANTHROPIC_API_KEY",
                    "anthropic",
                    "LLM",
                )?;

                let mut builder = rig::providers::anthropic::Client::<reqwest::Client>::builder()
                    .api_key(&api_key);

                if let Some(ref base_url) = config.base_url {
                    builder = builder.base_url(base_url);
                }

                let client = builder.build().map_err(|e| {
                    ShabkaError::Llm(format!("failed to build Anthropic LLM client: {e}"))
                })?;

                use rig::prelude::CompletionClient;
                let model = client.completion_model(&config.model);
                let model_name = config.model.clone();

                Box::new(RigCompletionWrapper { model, model_name })
            }

            "deepseek" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "DEEPSEEK_API_KEY",
                    "deepseek",
                    "LLM",
                )?;

                let client = rig::providers::deepseek::Client::<reqwest::Client>::builder()
                    .api_key(&api_key)
                    .build()
                    .map_err(|e| {
                        ShabkaError::Llm(format!("failed to build DeepSeek LLM client: {e}"))
                    })?;

                use rig::prelude::CompletionClient;
                let model = client.completion_model(&config.model);
                let model_name = config.model.clone();

                Box::new(RigCompletionWrapper { model, model_name })
            }

            "groq" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "GROQ_API_KEY",
                    "groq",
                    "LLM",
                )?;

                let client = rig::providers::groq::Client::<reqwest::Client>::builder()
                    .api_key(&api_key)
                    .build()
                    .map_err(|e| {
                        ShabkaError::Llm(format!("failed to build Groq LLM client: {e}"))
                    })?;

                use rig::prelude::CompletionClient;
                let model = client.completion_model(&config.model);
                let model_name = config.model.clone();

                Box::new(RigCompletionWrapper { model, model_name })
            }

            "xai" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "XAI_API_KEY",
                    "xai",
                    "LLM",
                )?;

                let client = rig::providers::xai::Client::<reqwest::Client>::builder()
                    .api_key(&api_key)
                    .build()
                    .map_err(|e| {
                        ShabkaError::Llm(format!("failed to build xAI LLM client: {e}"))
                    })?;

                use rig::prelude::CompletionClient;
                let model = client.completion_model(&config.model);
                let model_name = config.model.clone();

                Box::new(RigCompletionWrapper { model, model_name })
            }

            "cohere" => {
                let api_key = config::resolve_api_key(
                    config.api_key.as_deref(),
                    config.env_var.as_deref(),
                    "COHERE_API_KEY",
                    "cohere",
                    "LLM",
                )?;

                let client = rig::providers::cohere::Client::<reqwest::Client>::builder()
                    .api_key(&api_key)
                    .build()
                    .map_err(|e| {
                        ShabkaError::Llm(format!("failed to build Cohere LLM client: {e}"))
                    })?;

                use rig::prelude::CompletionClient;
                let model = client.completion_model(&config.model);
                let model_name = config.model.clone();

                Box::new(RigCompletionWrapper { model, model_name })
            }

            other => {
                return Err(ShabkaError::Config(format!(
                    "unknown LLM provider: '{other}' (expected 'ollama', 'openai', 'gemini', \
                     'anthropic', 'deepseek', 'groq', 'xai', or 'cohere')"
                )));
            }
        };

        Ok(Self {
            inner,
            config: config.clone(),
        })
    }

    /// Generate text from a prompt with an optional system message.
    /// Wraps the Rig call with retry logic (3 retries, 200ms base delay).
    pub async fn generate(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let max_tokens = self.config.max_tokens as u64;
        let prompt_owned = prompt.to_string();
        let system_owned = system.map(|s| s.to_string());

        with_retry(3, 200, || {
            let p = prompt_owned.clone();
            let s = system_owned.clone();
            async move {
                self.inner
                    .generate(p, s, max_tokens)
                    .await
                    .map_err(ShabkaError::Llm)
            }
        })
        .await
    }

    /// Generate structured output from the LLM.
    ///
    /// Calls `generate()` and deserializes the JSON response into `T`.
    /// Strips markdown fences if present.
    pub async fn generate_structured<T: serde::de::DeserializeOwned>(
        &self,
        prompt: &str,
        system: Option<&str>,
    ) -> Result<T> {
        let raw = self.generate(prompt, system).await?;
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        serde_json::from_str(cleaned)
            .map_err(|e| ShabkaError::Llm(format!("failed to parse structured LLM response: {e}")))
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
    fn test_from_config_deepseek_without_key_errors() {
        let saved = std::env::var("DEEPSEEK_API_KEY").ok();
        std::env::remove_var("DEEPSEEK_API_KEY");

        let config = LlmConfig {
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            api_key: None,
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key"));

        if let Some(key) = saved {
            std::env::set_var("DEEPSEEK_API_KEY", key);
        }
    }

    #[test]
    fn test_from_config_deepseek_with_key() {
        let config = LlmConfig {
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            api_key: Some("sk-test".into()),
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_config_groq_without_key_errors() {
        let saved = std::env::var("GROQ_API_KEY").ok();
        std::env::remove_var("GROQ_API_KEY");

        let config = LlmConfig {
            provider: "groq".into(),
            model: "llama-3.1-70b-versatile".into(),
            api_key: None,
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key"));

        if let Some(key) = saved {
            std::env::set_var("GROQ_API_KEY", key);
        }
    }

    #[test]
    fn test_from_config_groq_with_key() {
        let config = LlmConfig {
            provider: "groq".into(),
            model: "llama-3.1-70b-versatile".into(),
            api_key: Some("gsk-test".into()),
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_config_xai_without_key_errors() {
        let saved = std::env::var("XAI_API_KEY").ok();
        std::env::remove_var("XAI_API_KEY");

        let config = LlmConfig {
            provider: "xai".into(),
            model: "grok-2".into(),
            api_key: None,
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key"));

        if let Some(key) = saved {
            std::env::set_var("XAI_API_KEY", key);
        }
    }

    #[test]
    fn test_from_config_xai_with_key() {
        let config = LlmConfig {
            provider: "xai".into(),
            model: "grok-2".into(),
            api_key: Some("xai-test".into()),
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_config_cohere_without_key_errors() {
        let saved = std::env::var("COHERE_API_KEY").ok();
        std::env::remove_var("COHERE_API_KEY");

        let config = LlmConfig {
            provider: "cohere".into(),
            model: "command-r-plus".into(),
            api_key: None,
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key"));

        if let Some(key) = saved {
            std::env::set_var("COHERE_API_KEY", key);
        }
    }

    #[test]
    fn test_from_config_cohere_with_key() {
        let config = LlmConfig {
            provider: "cohere".into(),
            model: "command-r-plus".into(),
            api_key: Some("co-test".into()),
            ..Default::default()
        };
        let result = LlmService::from_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_api_key_from_config() {
        let key =
            config::resolve_api_key(Some("config-key"), None, "OPENAI_API_KEY", "openai", "LLM")
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

    #[test]
    fn test_generate_structured_parse() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct TestResponse {
            name: String,
            value: f32,
        }

        // Simulate the markdown-stripping + deserialization logic
        let raw = "```json\n{\"name\":\"test\",\"value\":0.5}\n```";
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let result: TestResponse = serde_json::from_str(cleaned).unwrap();
        assert_eq!(result.name, "test");
        assert!((result.value - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_generate_structured_parse_no_fences() {
        #[derive(serde::Deserialize, Debug)]
        struct Resp {
            ok: bool,
        }

        let raw = "{\"ok\":true}";
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let result: Resp = serde_json::from_str(cleaned).unwrap();
        assert!(result.ok);
    }

    #[test]
    fn test_generate_structured_parse_nested_content() {
        #[derive(serde::Deserialize, Debug)]
        struct CodeResp {
            language: String,
            snippet: String,
        }

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
}
