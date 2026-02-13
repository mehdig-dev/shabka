use crate::error::Result;

/// Trait for generating vector embeddings from text.
///
/// Implementations:
/// - `LocalEmbeddingProvider`: fastembed (ONNX), no API key needed
/// - `OpenAIEmbeddingProvider`: text-embedding-3-small, requires API key
/// - HelixDB native `Embed()` — embedding happens inside HelixQL queries (no provider needed)
pub trait EmbeddingProvider: Send + Sync {
    /// Generate an embedding vector for the given text.
    fn embed(&self, text: &str) -> impl std::future::Future<Output = Result<Vec<f32>>> + Send;

    /// Generate embeddings for multiple texts in a batch.
    fn embed_batch(
        &self,
        texts: &[&str],
    ) -> impl std::future::Future<Output = Result<Vec<Vec<f32>>>> + Send;

    /// The dimensionality of the embedding vectors.
    fn dimensions(&self) -> usize;

    /// Model identifier string for metadata tracking.
    fn model_id(&self) -> &str;
}
