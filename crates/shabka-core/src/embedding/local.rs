use crate::embedding::EmbeddingProvider;
use crate::error::{Result, ShabkaError};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Local embedding provider using fastembed (ONNX runtime).
/// Default model: BGE-Small-EN-V1.5 (384 dimensions).
pub struct LocalEmbeddingProvider {
    model: Arc<Mutex<TextEmbedding>>,
    model_id: String,
    dimensions: usize,
}

impl LocalEmbeddingProvider {
    pub fn new() -> Result<Self> {
        Self::with_model(EmbeddingModel::BGESmallENV15)
    }

    pub fn with_model(model: EmbeddingModel) -> Result<Self> {
        let dimensions = match model {
            EmbeddingModel::BGESmallENV15 => 384,
            EmbeddingModel::AllMiniLML6V2 => 384,
            _ => 384, // fallback
        };

        let model_id = format!("{:?}", model);

        let embedding =
            TextEmbedding::try_new(InitOptions::new(model).with_show_download_progress(true))
                .map_err(|e| ShabkaError::Embedding(e.to_string()))?;

        Ok(Self {
            model: Arc::new(Mutex::new(embedding)),
            model_id,
            dimensions,
        })
    }
}

impl EmbeddingProvider for LocalEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let model = self.model.clone();
        let text = text.to_string();
        // fastembed is sync + CPU-bound, run in blocking task
        let result = tokio::task::spawn_blocking(move || {
            let model = model.blocking_lock();
            model.embed(vec![text], None)
        })
        .await
        .map_err(|e| ShabkaError::Embedding(e.to_string()))?
        .map_err(|e| ShabkaError::Embedding(e.to_string()))?;

        result
            .into_iter()
            .next()
            .ok_or_else(|| ShabkaError::Embedding("empty embedding result".into()))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let model = self.model.clone();
        let texts: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
        let result = tokio::task::spawn_blocking(move || {
            let model = model.blocking_lock();
            model.embed(texts, None)
        })
        .await
        .map_err(|e| ShabkaError::Embedding(e.to_string()))?
        .map_err(|e| ShabkaError::Embedding(e.to_string()))?;

        Ok(result)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}
