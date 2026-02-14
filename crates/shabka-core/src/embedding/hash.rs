use super::provider::EmbeddingProvider;
use crate::error::Result;

/// Hash-based embedding provider for environments where fastembed (ONNX) and
/// OpenAI are unavailable (e.g. WSL2 without GPU or API key).
///
/// Generates deterministic 128-dimensional vectors from text using a simple
/// hash function. NOT suitable for real semantic search — similar texts will NOT
/// produce similar vectors. Use only for testing the full pipeline.
pub struct HashEmbeddingProvider;

const DIMENSIONS: usize = 128;

impl Default for HashEmbeddingProvider {
    fn default() -> Self {
        Self
    }
}

impl HashEmbeddingProvider {
    pub fn new() -> Self {
        Self
    }

    fn hash_text(text: &str) -> Vec<f32> {
        let mut vec = vec![0.0f32; DIMENSIONS];
        // Simple deterministic hash: spread bytes across dimensions
        for (i, byte) in text.bytes().enumerate() {
            let idx = i % DIMENSIONS;
            // Mix position and byte value
            vec[idx] += ((byte as f32) - 128.0) * 0.01 * ((i as f32 + 1.0).ln() + 1.0);
        }
        // L2-normalize so vectors have unit length
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vec {
                *v /= norm;
            }
        }
        vec
    }
}

impl EmbeddingProvider for HashEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(Self::hash_text(text))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| Self::hash_text(t)).collect())
    }

    fn dimensions(&self) -> usize {
        DIMENSIONS
    }

    fn model_id(&self) -> &str {
        "hash-128d"
    }
}
