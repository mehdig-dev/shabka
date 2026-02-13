//! Integration tests for Ollama embedding provider.
//!
//! Requires:
//! - Ollama running at localhost:11434 with `nomic-embed-text` model pulled
//! - For `test_ollama_search_with_helix`: HelixDB running at localhost:6969
//!
//! Run: `cargo test -p kaizen-core --no-default-features --test ollama_embedding -- --ignored`

mod common;

use common::{helix_available, ollama_available, ollama_embedder, test_memory, test_storage};
use kaizen_core::model::MemoryKind;
use kaizen_core::storage::StorageBackend;

/// Embed a single text and verify the result has the right shape.
#[tokio::test]
#[ignore]
async fn test_ollama_embed_single() {
    if !ollama_available().await {
        eprintln!("SKIP: Ollama not available at localhost:11434");
        return;
    }

    let embedder = ollama_embedder();
    let result = embedder.embed("Rust programming language").await;
    assert!(result.is_ok(), "embed failed: {:?}", result.err());

    let vec = result.unwrap();
    assert!(!vec.is_empty(), "embedding vector should not be empty");
    // nomic-embed-text produces 768d vectors
    assert_eq!(vec.len(), 768, "expected 768 dimensions, got {}", vec.len());
}

/// Embed a batch of 3 texts and verify all results.
#[tokio::test]
#[ignore]
async fn test_ollama_embed_batch() {
    if !ollama_available().await {
        eprintln!("SKIP: Ollama not available at localhost:11434");
        return;
    }

    let embedder = ollama_embedder();
    let texts = ["Rust programming", "Go concurrency", "Python data science"];
    let result = embedder.embed_batch(&texts).await;
    assert!(result.is_ok(), "embed_batch failed: {:?}", result.err());

    let vecs = result.unwrap();
    assert_eq!(vecs.len(), 3, "expected 3 embedding vectors");
    for (i, vec) in vecs.iter().enumerate() {
        assert_eq!(
            vec.len(),
            768,
            "vector {i} has wrong dimensions: {}",
            vec.len()
        );
    }
}

/// Same text embedded twice should produce identical vectors.
#[tokio::test]
#[ignore]
async fn test_ollama_embed_deterministic() {
    if !ollama_available().await {
        eprintln!("SKIP: Ollama not available at localhost:11434");
        return;
    }

    let embedder = ollama_embedder();
    let text = "Deterministic embedding test";

    let v1 = embedder.embed(text).await.expect("first embed failed");
    let v2 = embedder.embed(text).await.expect("second embed failed");

    assert_eq!(v1.len(), v2.len());
    for (i, (a, b)) in v1.iter().zip(v2.iter()).enumerate() {
        assert!(
            (a - b).abs() < 1e-6,
            "vectors differ at index {i}: {a} vs {b}"
        );
    }
}

/// Verify that semantically related texts are closer than unrelated ones.
/// "Rust programming" should be closer to "Go programming" than to "chocolate cake".
#[tokio::test]
#[ignore]
async fn test_ollama_semantic_similarity() {
    if !ollama_available().await {
        eprintln!("SKIP: Ollama not available at localhost:11434");
        return;
    }

    let embedder = ollama_embedder();
    let texts = [
        "Rust programming language",
        "Go programming language",
        "chocolate cake recipe",
    ];
    let vecs = embedder
        .embed_batch(&texts)
        .await
        .expect("embed_batch failed");

    let rust_go = cosine_similarity(&vecs[0], &vecs[1]);
    let rust_cake = cosine_similarity(&vecs[0], &vecs[2]);

    assert!(
        rust_go > rust_cake,
        "expected Rust-Go similarity ({rust_go:.4}) > Rust-cake similarity ({rust_cake:.4})"
    );
}

/// End-to-end: save 3 memories with Ollama embeddings, then vector search
/// should return the most relevant one first.
///
/// Requires both Ollama and HelixDB.
#[tokio::test]
#[ignore]
async fn test_ollama_search_with_helix() {
    if !ollama_available().await {
        eprintln!("SKIP: Ollama not available at localhost:11434");
        return;
    }
    if !helix_available().await {
        eprintln!("SKIP: HelixDB not available at localhost:6969");
        return;
    }

    let storage = test_storage();
    let embedder = ollama_embedder();

    // Create 3 topically distinct memories
    let m_rust = test_memory("Rust ownership and borrowing", MemoryKind::Lesson);
    let m_cooking = test_memory("How to make sourdough bread", MemoryKind::Fact);
    let m_music = test_memory("Jazz improvisation techniques", MemoryKind::Observation);

    let e_rust = embedder
        .embed(&m_rust.embedding_text())
        .await
        .expect("embed rust");
    let e_cooking = embedder
        .embed(&m_cooking.embedding_text())
        .await
        .expect("embed cooking");
    let e_music = embedder
        .embed(&m_music.embedding_text())
        .await
        .expect("embed music");

    // Save all 3
    storage
        .save_memory(&m_rust, Some(&e_rust))
        .await
        .expect("save rust");
    storage
        .save_memory(&m_cooking, Some(&e_cooking))
        .await
        .expect("save cooking");
    storage
        .save_memory(&m_music, Some(&e_music))
        .await
        .expect("save music");

    // Search for something related to programming
    let query_vec = embedder
        .embed("memory safety in systems programming")
        .await
        .expect("embed query");
    let results = storage
        .vector_search(&query_vec, 3)
        .await
        .expect("vector search");

    assert!(
        !results.is_empty(),
        "vector search should return at least 1 result"
    );
    // The top result should be the Rust memory
    assert_eq!(
        results[0].0.id, m_rust.id,
        "expected Rust memory as top result, got '{}'",
        results[0].0.title
    );

    // Cleanup
    let _ = storage.delete_memory(m_rust.id).await;
    let _ = storage.delete_memory(m_cooking.id).await;
    let _ = storage.delete_memory(m_music.id).await;
}

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
