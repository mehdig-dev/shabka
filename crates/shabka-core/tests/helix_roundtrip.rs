//! Integration tests for HelixDB storage round-trips.
//!
//! Requires HelixDB running at localhost:6969 (`just db`) and Ollama with nomic-embed-text.
//!
//! Run: `cargo test -p shabka-core --no-default-features --test helix_roundtrip -- --ignored`

mod common;

use common::{helix_available, ollama_available, ollama_embedder, test_memory, test_storage};
use shabka_core::model::{
    MemoryKind, MemoryRelation, MemoryStatus, RelationType, TimelineQuery, UpdateMemoryInput,
};
use shabka_core::storage::StorageBackend;

/// save_memory → get_memory → verify fields → delete → get returns error.
#[tokio::test]
#[ignore]
async fn test_save_get_delete_lifecycle() {
    if !helix_available().await || !ollama_available().await {
        eprintln!("SKIP: HelixDB or Ollama not available");
        return;
    }

    let storage = test_storage();
    let embedder = ollama_embedder();

    let memory = test_memory("Lifecycle test memory", MemoryKind::Observation);
    let id = memory.id;
    let embedding = embedder
        .embed(&memory.embedding_text())
        .await
        .expect("embed");

    // Save
    storage
        .save_memory(&memory, Some(&embedding))
        .await
        .expect("save_memory failed");

    // Get and verify fields
    let fetched = storage.get_memory(id).await.expect("get_memory failed");
    assert_eq!(fetched.id, id);
    assert!(fetched.title.contains("Lifecycle test memory"));
    assert_eq!(fetched.kind, MemoryKind::Observation);
    assert_eq!(fetched.created_by, "integration-test");
    assert!((fetched.importance - 0.5).abs() < 0.01);

    // Delete
    storage
        .delete_memory(id)
        .await
        .expect("delete_memory failed");

    // Get after delete should fail
    let result = storage.get_memory(id).await;
    assert!(
        result.is_err(),
        "get_memory after delete should return an error"
    );
}

/// Save 3 memories, get_memories with all 3 IDs, verify all returned.
#[tokio::test]
#[ignore]
async fn test_get_memories_batch() {
    if !helix_available().await || !ollama_available().await {
        eprintln!("SKIP: HelixDB or Ollama not available");
        return;
    }

    let storage = test_storage();
    let embedder = ollama_embedder();

    let m1 = test_memory("Batch memory 1", MemoryKind::Fact);
    let m2 = test_memory("Batch memory 2", MemoryKind::Lesson);
    let m3 = test_memory("Batch memory 3", MemoryKind::Pattern);

    for m in [&m1, &m2, &m3] {
        let emb = embedder.embed(&m.embedding_text()).await.unwrap();
        storage.save_memory(m, Some(&emb)).await.unwrap();
    }

    let ids = [m1.id, m2.id, m3.id];
    let fetched = storage
        .get_memories(&ids)
        .await
        .expect("get_memories failed");

    assert_eq!(
        fetched.len(),
        3,
        "expected 3 memories, got {}",
        fetched.len()
    );

    let fetched_ids: Vec<_> = fetched.iter().map(|m| m.id).collect();
    for id in &ids {
        assert!(fetched_ids.contains(id), "missing memory {id}");
    }

    // Cleanup
    for id in &ids {
        let _ = storage.delete_memory(*id).await;
    }
}

/// Save, update title + importance, verify changes persisted and updated_at advanced.
#[tokio::test]
#[ignore]
async fn test_update_memory() {
    if !helix_available().await || !ollama_available().await {
        eprintln!("SKIP: HelixDB or Ollama not available");
        return;
    }

    let storage = test_storage();
    let embedder = ollama_embedder();

    let memory = test_memory("Update test original", MemoryKind::Decision);
    let id = memory.id;
    let original_updated_at = memory.updated_at;
    let emb = embedder.embed(&memory.embedding_text()).await.unwrap();
    storage.save_memory(&memory, Some(&emb)).await.unwrap();

    // Small delay to ensure updated_at advances
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let update = UpdateMemoryInput {
        title: Some("Update test modified".to_string()),
        importance: Some(0.9),
        ..Default::default()
    };

    let updated = storage
        .update_memory(id, &update)
        .await
        .expect("update_memory failed");

    assert!(updated.title.contains("Update test modified"));
    assert!((updated.importance - 0.9).abs() < 0.01);
    assert!(
        updated.updated_at > original_updated_at,
        "updated_at should advance"
    );

    // Verify persistence
    let fetched = storage.get_memory(id).await.expect("get after update");
    assert!(fetched.title.contains("Update test modified"));
    assert!((fetched.importance - 0.9).abs() < 0.01);

    // Cleanup
    let _ = storage.delete_memory(id).await;
}

/// Save 3 memories, vector search returns results with semantic relevance.
#[tokio::test]
#[ignore]
async fn test_vector_search() {
    if !helix_available().await || !ollama_available().await {
        eprintln!("SKIP: HelixDB or Ollama not available");
        return;
    }

    let storage = test_storage();
    let embedder = ollama_embedder();

    let m1 = test_memory("Vector search alpha", MemoryKind::Observation);
    let m2 = test_memory("Vector search beta", MemoryKind::Fact);
    let m3 = test_memory("Vector search gamma", MemoryKind::Lesson);

    let mut ids = Vec::new();
    for m in [&m1, &m2, &m3] {
        let emb = embedder.embed(&m.embedding_text()).await.unwrap();
        storage.save_memory(m, Some(&emb)).await.unwrap();
        ids.push(m.id);
    }

    // Search using m1's embedding — should find m1 (exact match) at top
    let query_emb = embedder.embed(&m1.embedding_text()).await.unwrap();
    let results = storage
        .vector_search(&query_emb, 10)
        .await
        .expect("vector_search failed");

    assert!(
        !results.is_empty(),
        "vector search should return at least 1 result"
    );

    // The exact-match memory should appear in results
    let result_ids: Vec<_> = results.iter().map(|r| r.0.id).collect();
    assert!(
        result_ids.contains(&m1.id),
        "m1 should appear in search results"
    );

    // Cleanup
    for id in &ids {
        let _ = storage.delete_memory(*id).await;
    }
}

/// Save 2 memories, add_relation between them, get_relations returns the link.
#[tokio::test]
#[ignore]
async fn test_relations() {
    if !helix_available().await || !ollama_available().await {
        eprintln!("SKIP: HelixDB or Ollama not available");
        return;
    }

    let storage = test_storage();
    let embedder = ollama_embedder();

    let m1 = test_memory("Relation source", MemoryKind::Error);
    let m2 = test_memory("Relation target", MemoryKind::Fix);

    for m in [&m1, &m2] {
        let emb = embedder.embed(&m.embedding_text()).await.unwrap();
        storage.save_memory(m, Some(&emb)).await.unwrap();
    }

    let relation = MemoryRelation {
        source_id: m1.id,
        target_id: m2.id,
        relation_type: RelationType::Fixes,
        strength: 0.8,
    };
    storage
        .add_relation(&relation)
        .await
        .expect("add_relation failed");

    let relations = storage
        .get_relations(m1.id)
        .await
        .expect("get_relations failed");

    assert!(
        !relations.is_empty(),
        "should have at least 1 relation from m1"
    );
    let target_ids: Vec<_> = relations.iter().map(|r| r.target_id).collect();
    assert!(
        target_ids.contains(&m2.id),
        "m2 should be in m1's relations"
    );

    // Verify edge properties are returned (not hardcoded defaults)
    let rel = relations.iter().find(|r| r.target_id == m2.id).unwrap();
    assert_eq!(
        rel.relation_type,
        RelationType::Fixes,
        "relation_type should be Fixes, not default Related"
    );
    assert!(
        (rel.strength - 0.8).abs() < 0.01,
        "strength should be ~0.8, got {}",
        rel.strength
    );

    // Cleanup
    let _ = storage.delete_memory(m1.id).await;
    let _ = storage.delete_memory(m2.id).await;
}

/// Save 3 memories with slight delays, timeline returns most-recent-first.
#[tokio::test]
#[ignore]
async fn test_timeline_ordering() {
    if !helix_available().await || !ollama_available().await {
        eprintln!("SKIP: HelixDB or Ollama not available");
        return;
    }

    let storage = test_storage();
    let embedder = ollama_embedder();

    let m1 = test_memory("Timeline first", MemoryKind::Observation);
    let emb1 = embedder.embed(&m1.embedding_text()).await.unwrap();
    storage.save_memory(&m1, Some(&emb1)).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let m2 = test_memory("Timeline second", MemoryKind::Decision);
    let emb2 = embedder.embed(&m2.embedding_text()).await.unwrap();
    storage.save_memory(&m2, Some(&emb2)).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let m3 = test_memory("Timeline third", MemoryKind::Lesson);
    let emb3 = embedder.embed(&m3.embedding_text()).await.unwrap();
    storage.save_memory(&m3, Some(&emb3)).await.unwrap();

    let query = TimelineQuery {
        limit: 100,
        ..Default::default()
    };
    let entries = storage.timeline(&query).await.expect("timeline failed");

    // Find our test entries (there may be other memories in the DB)
    let our_entries: Vec<_> = entries
        .iter()
        .filter(|e| e.id == m1.id || e.id == m2.id || e.id == m3.id)
        .collect();

    assert_eq!(
        our_entries.len(),
        3,
        "expected 3 timeline entries, got {}",
        our_entries.len()
    );

    // Timeline is most-recent-first, so m3 should come before m2, and m2 before m1
    let pos_m1 = our_entries.iter().position(|e| e.id == m1.id).unwrap();
    let pos_m2 = our_entries.iter().position(|e| e.id == m2.id).unwrap();
    let pos_m3 = our_entries.iter().position(|e| e.id == m3.id).unwrap();
    assert!(
        pos_m3 < pos_m2 && pos_m2 < pos_m1,
        "expected most-recent-first: m3(pos={pos_m3}) < m2(pos={pos_m2}) < m1(pos={pos_m1})"
    );

    // Cleanup
    let _ = storage.delete_memory(m1.id).await;
    let _ = storage.delete_memory(m2.id).await;
    let _ = storage.delete_memory(m3.id).await;
}

/// Complete round-trip exercising the same flow as MCP tools:
/// save → search → get → update → relate → timeline → delete
#[tokio::test]
#[ignore]
async fn test_full_mcp_equivalent_lifecycle() {
    if !helix_available().await || !ollama_available().await {
        eprintln!("SKIP: HelixDB or Ollama not available");
        return;
    }

    let storage = test_storage();
    let embedder = ollama_embedder();

    // 1. Save two memories (like save_memory MCP tool)
    let m1 = test_memory("MCP lifecycle: error report", MemoryKind::Error)
        .with_tags(vec!["rust".to_string(), "borrow-checker".to_string()])
        .with_importance(0.8);
    let m2 = test_memory("MCP lifecycle: fix applied", MemoryKind::Fix)
        .with_tags(vec!["rust".to_string()])
        .with_importance(0.7);

    let e1 = embedder.embed(&m1.embedding_text()).await.unwrap();
    let e2 = embedder.embed(&m2.embedding_text()).await.unwrap();

    storage.save_memory(&m1, Some(&e1)).await.expect("save m1");
    storage.save_memory(&m2, Some(&e2)).await.expect("save m2");

    // 2. Search (like search MCP tool) — find m1 by its embedding
    let results = storage.vector_search(&e1, 5).await.expect("vector_search");
    assert!(!results.is_empty(), "search should return results");
    let found_ids: Vec<_> = results.iter().map(|r| r.0.id).collect();
    assert!(found_ids.contains(&m1.id), "m1 should appear in search");

    // 3. Get full details (like get_memories MCP tool)
    let details = storage
        .get_memories(&[m1.id, m2.id])
        .await
        .expect("get_memories");
    assert_eq!(details.len(), 2);

    // 4. Update (like update_memory MCP tool)
    let update = UpdateMemoryInput {
        status: Some(MemoryStatus::Archived),
        title: Some("MCP lifecycle: error report (resolved)".to_string()),
        ..Default::default()
    };
    let updated = storage
        .update_memory(m1.id, &update)
        .await
        .expect("update_memory");
    assert_eq!(updated.status, MemoryStatus::Archived);
    assert!(updated.title.contains("resolved"));

    // 5. Relate (like relate_memories MCP tool)
    let relation = MemoryRelation {
        source_id: m2.id,
        target_id: m1.id,
        relation_type: RelationType::Fixes,
        strength: 0.9,
    };
    storage.add_relation(&relation).await.expect("add_relation");

    let relations = storage.get_relations(m2.id).await.expect("get_relations");
    assert!(
        relations.iter().any(|r| r.target_id == m1.id),
        "m2 should relate to m1"
    );

    // 6. Timeline (like timeline MCP tool)
    let entries = storage
        .timeline(&TimelineQuery {
            limit: 100,
            ..Default::default()
        })
        .await
        .expect("timeline");
    let our_entries: Vec<_> = entries
        .iter()
        .filter(|e| e.id == m1.id || e.id == m2.id)
        .collect();
    assert_eq!(our_entries.len(), 2, "both memories in timeline");

    // 7. Delete (like delete_memory MCP tool)
    storage.delete_memory(m1.id).await.expect("delete m1");
    storage.delete_memory(m2.id).await.expect("delete m2");

    // Verify cleanup
    assert!(storage.get_memory(m1.id).await.is_err());
    assert!(storage.get_memory(m2.id).await.is_err());
}
