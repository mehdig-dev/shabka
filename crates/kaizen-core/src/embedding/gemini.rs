use crate::embedding::EmbeddingProvider;
use crate::error::{KaizenError, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Google Gemini embedding provider using the embedContent API.
pub struct GeminiEmbeddingProvider {
    client: Client,
    api_key: String,
    model: String,
    dimensions: usize,
    base_url: String,
}

impl GeminiEmbeddingProvider {
    pub fn with_config(
        api_key: String,
        model: Option<String>,
        dimensions: Option<usize>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model: model.unwrap_or_else(|| "text-embedding-004".to_string()),
            dimensions: dimensions.unwrap_or(768),
            base_url: base_url
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta".to_string()),
        }
    }
}

#[derive(Serialize)]
struct GeminiEmbedRequest {
    content: GeminiContent,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Deserialize)]
struct GeminiEmbedResponse {
    embedding: GeminiEmbedding,
}

#[derive(Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}

// -- Batch embedding types for batchEmbedContents --

#[derive(Serialize)]
struct GeminiBatchEmbedRequest {
    requests: Vec<GeminiBatchEntry>,
}

#[derive(Serialize)]
struct GeminiBatchEntry {
    model: String,
    content: GeminiContent,
}

#[derive(Deserialize)]
struct GeminiBatchEmbedResponse {
    embeddings: Vec<GeminiEmbedding>,
}

impl GeminiEmbeddingProvider {
    /// Sequential fallback when batchEmbedContents fails.
    async fn embed_batch_sequential(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(EmbeddingProvider::embed(self, text).await?);
        }
        Ok(results)
    }
}

impl EmbeddingProvider for GeminiEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let req = GeminiEmbedRequest {
            content: GeminiContent {
                parts: vec![GeminiPart {
                    text: text.to_string(),
                }],
            },
        };

        let url = format!(
            "{}/models/{}:embedContent?key={}",
            self.base_url, self.model, self.api_key
        );

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| KaizenError::Embedding(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(KaizenError::Embedding(format!(
                "Gemini API error {status}: {body}"
            )));
        }

        let result: GeminiEmbedResponse = response
            .json()
            .await
            .map_err(|e| KaizenError::Embedding(e.to_string()))?;

        Ok(result.embedding.values)
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let model_path = format!("models/{}", self.model);
        let requests: Vec<GeminiBatchEntry> = texts
            .iter()
            .map(|text| GeminiBatchEntry {
                model: model_path.clone(),
                content: GeminiContent {
                    parts: vec![GeminiPart {
                        text: text.to_string(),
                    }],
                },
            })
            .collect();

        let batch_req = GeminiBatchEmbedRequest { requests };

        let url = format!(
            "{}/models/{}:batchEmbedContents?key={}",
            self.base_url, self.model, self.api_key
        );

        let response = self
            .client
            .post(&url)
            .json(&batch_req)
            .send()
            .await
            .map_err(|e| KaizenError::Embedding(e.to_string()));

        match response {
            Ok(resp) if resp.status().is_success() => {
                let result: GeminiBatchEmbedResponse = resp
                    .json()
                    .await
                    .map_err(|e| KaizenError::Embedding(e.to_string()))?;
                Ok(result.embeddings.into_iter().map(|e| e.values).collect())
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_else(|_| "unknown error".into());
                tracing::warn!(
                    "Gemini batch API error {status}: {body}, falling back to sequential"
                );
                self.embed_batch_sequential(texts).await
            }
            Err(e) => {
                tracing::warn!("Gemini batch request failed: {e}, falling back to sequential");
                self.embed_batch_sequential(texts).await
            }
        }
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_request_serialization() {
        let req = GeminiEmbedRequest {
            content: GeminiContent {
                parts: vec![GeminiPart {
                    text: "hello world".to_string(),
                }],
            },
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["content"]["parts"][0]["text"], "hello world");
    }

    #[test]
    fn test_gemini_response_deserialization() {
        let json = r#"{"embedding": {"values": [0.1, 0.2, 0.3]}}"#;
        let resp: GeminiEmbedResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.embedding.values, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn test_gemini_defaults() {
        let provider = GeminiEmbeddingProvider::with_config("key".into(), None, None, None);
        assert_eq!(provider.model, "text-embedding-004");
        assert_eq!(provider.dimensions, 768);
        assert!(provider
            .base_url
            .contains("generativelanguage.googleapis.com"));
    }

    #[test]
    fn test_gemini_batch_request_serialization() {
        let req = GeminiBatchEmbedRequest {
            requests: vec![
                GeminiBatchEntry {
                    model: "models/text-embedding-004".to_string(),
                    content: GeminiContent {
                        parts: vec![GeminiPart {
                            text: "hello".to_string(),
                        }],
                    },
                },
                GeminiBatchEntry {
                    model: "models/text-embedding-004".to_string(),
                    content: GeminiContent {
                        parts: vec![GeminiPart {
                            text: "world".to_string(),
                        }],
                    },
                },
            ],
        };
        let json = serde_json::to_value(&req).unwrap();
        let requests = json["requests"].as_array().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["model"], "models/text-embedding-004");
        assert_eq!(requests[0]["content"]["parts"][0]["text"], "hello");
        assert_eq!(requests[1]["content"]["parts"][0]["text"], "world");
    }

    #[test]
    fn test_gemini_batch_response_deserialization() {
        let json = r#"{"embeddings": [{"values": [0.1, 0.2]}, {"values": [0.3, 0.4]}]}"#;
        let resp: GeminiBatchEmbedResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.embeddings.len(), 2);
        assert_eq!(resp.embeddings[0].values, vec![0.1, 0.2]);
        assert_eq!(resp.embeddings[1].values, vec![0.3, 0.4]);
    }
}
