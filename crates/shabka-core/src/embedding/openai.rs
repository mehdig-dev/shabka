use crate::embedding::EmbeddingProvider;
use crate::error::{Result, ShabkaError};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// OpenAI-compatible embedding provider.
///
/// Works with OpenAI, Ollama, vLLM, LiteLLM, LocalAI, Azure OpenAI — anything
/// that exposes an OpenAI-compatible `/v1/embeddings` endpoint.
pub struct OpenAIEmbeddingProvider {
    client: Client,
    api_key: String,
    model: String,
    dimensions: usize,
    base_url: String,
    request_dimensions: Option<usize>,
}

impl OpenAIEmbeddingProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
            base_url: "https://api.openai.com/v1".to_string(),
            request_dimensions: None,
        }
    }

    pub fn with_model(api_key: String, model: String, dimensions: usize) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            dimensions,
            base_url: "https://api.openai.com/v1".to_string(),
            request_dimensions: None,
        }
    }

    pub fn with_config(
        api_key: String,
        model: String,
        dimensions: Option<usize>,
        base_url: Option<String>,
    ) -> Self {
        let base_url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let dims = dimensions.unwrap_or(1536);
        Self {
            client: Client::new(),
            api_key,
            model,
            dimensions: dims,
            base_url,
            request_dimensions: dimensions,
        }
    }
}

#[derive(Serialize)]
struct EmbeddingRequest {
    input: Vec<String>,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl EmbeddingProvider for OpenAIEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_batch(&[text]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| ShabkaError::Embedding("empty embedding result".into()))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let req = EmbeddingRequest {
            input: texts.iter().map(|s| s.to_string()).collect(),
            model: self.model.clone(),
            dimensions: self.request_dimensions,
        };

        let url = format!("{}/embeddings", self.base_url);

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await
            .map_err(|e| ShabkaError::Embedding(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(ShabkaError::Embedding(format!(
                "OpenAI API error {status}: {body}"
            )));
        }

        let result: EmbeddingResponse = response
            .json()
            .await
            .map_err(|e| ShabkaError::Embedding(e.to_string()))?;

        Ok(result.data.into_iter().map(|d| d.embedding).collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
