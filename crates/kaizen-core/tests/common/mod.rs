#![allow(unused_imports, dead_code)]

use kaizen_core::config::EmbeddingConfig;
use kaizen_core::embedding::{EmbeddingService, HashEmbeddingProvider};
use kaizen_core::model::{Memory, MemoryKind};
use kaizen_core::storage::HelixStorage;

/// Check if HelixDB is reachable at localhost:6969.
pub async fn helix_available() -> bool {
    let client = reqwest::Client::new();
    // The timeline endpoint accepts POST — any valid body will do.
    client
        .post("http://localhost:6969/timeline")
        .json(&serde_json::json!({"limit": 1}))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Check if Ollama is reachable at localhost:11434.
pub async fn ollama_available() -> bool {
    let client = reqwest::Client::new();
    client
        .get("http://localhost:11434/api/tags")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Create a test memory with a unique UUID-tagged title to prevent collisions.
pub fn test_memory(title: &str, kind: MemoryKind) -> Memory {
    let unique_title = format!("{} [test-{}]", title, uuid::Uuid::now_v7());
    Memory::new(
        unique_title,
        format!("Integration test content for: {title}"),
        kind,
        "integration-test".to_string(),
    )
}

/// Create a `HashEmbeddingProvider` (128d deterministic, no external deps).
pub fn hash_embedder() -> HashEmbeddingProvider {
    HashEmbeddingProvider::new()
}

/// Create an `EmbeddingService` configured for Ollama (nomic-embed-text).
pub fn ollama_embedder() -> EmbeddingService {
    let config = EmbeddingConfig {
        provider: "ollama".to_string(),
        model: "nomic-embed-text".to_string(),
        api_key: None,
        base_url: None,
        dimensions: None,
        env_var: None,
    };
    EmbeddingService::from_config(&config).expect("ollama embedder config should be valid")
}

/// Create a `HelixStorage` pointing at localhost:6969.
pub fn test_storage() -> HelixStorage {
    HelixStorage::new(None, None, None)
}
