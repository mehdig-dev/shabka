use crate::config::LlmConfig;
use crate::error::{Result, ShabkaError};

/// LLM text generation service. Reuses the same providers as embeddings
/// (Ollama, OpenAI, Gemini) but for chat/completion instead of embeddings.
pub struct LlmService {
    provider: LlmProvider,
    config: LlmConfig,
    client: reqwest::Client,
}

impl std::fmt::Debug for LlmService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmService")
            .field("provider", &self.provider)
            .field("model", &self.config.model)
            .finish()
    }
}

#[derive(Debug)]
enum LlmProvider {
    Ollama,
    OpenAI,
    Gemini,
    Anthropic,
}

impl LlmService {
    /// Create an LLM service from configuration.
    pub fn from_config(config: &LlmConfig) -> Result<Self> {
        let provider = match config.provider.as_str() {
            "ollama" => LlmProvider::Ollama,
            "openai" => LlmProvider::OpenAI,
            "gemini" => LlmProvider::Gemini,
            "anthropic" | "claude" => LlmProvider::Anthropic,
            other => {
                return Err(ShabkaError::Config(format!(
                    "unknown LLM provider: '{other}' (expected 'ollama', 'openai', 'gemini', or 'anthropic')"
                )));
            }
        };

        // Validate API key for providers that need one
        match &provider {
            LlmProvider::OpenAI => {
                resolve_api_key(config, "OPENAI_API_KEY")?;
            }
            LlmProvider::Gemini => {
                resolve_api_key(config, "GEMINI_API_KEY")?;
            }
            LlmProvider::Anthropic => {
                resolve_api_key(config, "ANTHROPIC_API_KEY")?;
            }
            LlmProvider::Ollama => {}
        }

        Ok(Self {
            provider,
            config: config.clone(),
            client: reqwest::Client::new(),
        })
    }

    /// Generate text from a prompt with an optional system message.
    pub async fn generate(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        match &self.provider {
            LlmProvider::Ollama => self.generate_ollama(prompt, system).await,
            LlmProvider::OpenAI => self.generate_openai(prompt, system).await,
            LlmProvider::Gemini => self.generate_gemini(prompt, system).await,
            LlmProvider::Anthropic => self.generate_anthropic(prompt, system).await,
        }
    }

    /// Ollama: POST {base_url}/api/generate
    async fn generate_ollama(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("http://localhost:11434");

        let url = format!("{}/api/generate", base_url.trim_end_matches('/'));

        let mut body = serde_json::json!({
            "model": self.config.model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "num_predict": self.config.max_tokens,
            }
        });

        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys.to_string());
        }

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ShabkaError::Embedding(format!("Ollama LLM request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ShabkaError::Embedding(format!(
                "Ollama LLM error {status}: {text}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ShabkaError::Embedding(format!("Ollama LLM response parse error: {e}")))?;

        json["response"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                ShabkaError::Embedding("Ollama LLM response missing 'response' field".into())
            })
    }

    /// OpenAI: POST {base_url}/v1/chat/completions
    async fn generate_openai(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let api_key = resolve_api_key(&self.config, "OPENAI_API_KEY")?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com");

        let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));

        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(serde_json::json!({"role": "system", "content": sys}));
        }
        messages.push(serde_json::json!({"role": "user", "content": prompt}));

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "max_tokens": self.config.max_tokens,
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| ShabkaError::Embedding(format!("OpenAI LLM request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ShabkaError::Embedding(format!(
                "OpenAI LLM error {status}: {text}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ShabkaError::Embedding(format!("OpenAI LLM response parse error: {e}")))?;

        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ShabkaError::Embedding("OpenAI LLM response missing content".into()))
    }

    /// Anthropic: POST {base_url}/v1/messages
    async fn generate_anthropic(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let api_key = resolve_api_key(&self.config, "ANTHROPIC_API_KEY")?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com");

        let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

        let mut body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "messages": [{"role": "user", "content": prompt}],
        });

        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys.to_string());
        }

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ShabkaError::Embedding(format!("Anthropic LLM request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ShabkaError::Embedding(format!(
                "Anthropic LLM error {status}: {text}"
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ShabkaError::Embedding(format!("Anthropic LLM response parse error: {e}"))
        })?;

        // Anthropic response: {"content": [{"type": "text", "text": "..."}]}
        json["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                ShabkaError::Embedding("Anthropic LLM response missing text content".into())
            })
    }

    /// Gemini: POST generativelanguage.googleapis.com/v1beta/models/{model}:generateContent
    async fn generate_gemini(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let api_key = resolve_api_key(&self.config, "GEMINI_API_KEY")?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://generativelanguage.googleapis.com");

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            base_url.trim_end_matches('/'),
            self.config.model,
            api_key,
        );

        let mut body = serde_json::json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "maxOutputTokens": self.config.max_tokens,
            }
        });

        if let Some(sys) = system {
            body["systemInstruction"] = serde_json::json!({"parts": [{"text": sys}]});
        }

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ShabkaError::Embedding(format!("Gemini LLM request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ShabkaError::Embedding(format!(
                "Gemini LLM error {status}: {text}"
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ShabkaError::Embedding(format!("Gemini LLM response parse error: {e}")))?;

        json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ShabkaError::Embedding("Gemini LLM response missing text".into()))
    }
}

/// Resolve an API key from config, a custom env var, or a default env var.
fn resolve_api_key(config: &LlmConfig, default_env_var: &str) -> Result<String> {
    if let Some(ref key) = config.api_key {
        if !key.is_empty() {
            return Ok(key.clone());
        }
    }

    let env_var_name = config.env_var.as_deref().unwrap_or(default_env_var);

    std::env::var(env_var_name).map_err(|_| {
        ShabkaError::Config(format!(
            "{} LLM provider requires an API key (set llm.api_key or {})",
            config.provider, env_var_name
        ))
    })
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
        let key = resolve_api_key(&config, "OPENAI_API_KEY").unwrap();
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
        let key = resolve_api_key(&config, "OPENAI_API_KEY").unwrap();
        assert_eq!(key, "env-llm-key");
        std::env::remove_var("MY_LLM_KEY");
    }
}
