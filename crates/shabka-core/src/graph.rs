//! Graph intelligence — automatic relationship discovery and traversal.
//!
//! - `semantic_auto_relate`: vector-search for similar memories and create edges.
//! - `follow_chain`: BFS traversal along typed edges for debugging narratives.

use std::collections::{HashSet, VecDeque};

use uuid::Uuid;

use crate::model::{MemoryRelation, RelationType};
use crate::storage::StorageBackend;

/// Default similarity threshold for auto-relating memories (0.0–1.0).
/// Only memories with vector similarity above this value get linked.
const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.6;

/// Maximum number of auto-created relations per memory.
const DEFAULT_MAX_RELATIONS: usize = 3;

/// Find semantically similar memories and create `Related` edges.
///
/// - `storage`: the storage backend for search and relation creation
/// - `memory_id`: the ID of the newly saved memory
/// - `embedding`: the embedding vector for the new memory
/// - `threshold`: minimum similarity score to create a relation (default 0.6)
/// - `max_relations`: maximum number of relations to create (default 3)
///
/// Returns the number of relations created. Errors are logged and swallowed.
pub async fn semantic_auto_relate(
    storage: &impl StorageBackend,
    memory_id: Uuid,
    embedding: &[f32],
    threshold: Option<f32>,
    max_relations: Option<usize>,
) -> usize {
    let threshold = threshold.unwrap_or(DEFAULT_SIMILARITY_THRESHOLD);
    let max_rels = max_relations.unwrap_or(DEFAULT_MAX_RELATIONS);

    // Over-fetch to account for self-match and below-threshold results
    let fetch_limit = max_rels * 3 + 1;
    let results = match storage.vector_search(embedding, fetch_limit).await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("semantic_auto_relate: vector search failed: {e}");
            return 0;
        }
    };

    // Get existing relations so we don't duplicate
    let existing = storage.get_relations(memory_id).await.unwrap_or_default();
    let already_related: std::collections::HashSet<Uuid> = existing
        .iter()
        .map(|r| {
            if r.source_id == memory_id {
                r.target_id
            } else {
                r.source_id
            }
        })
        .collect();

    let mut created = 0usize;
    for (candidate, score) in &results {
        if created >= max_rels {
            break;
        }
        // Skip self
        if candidate.id == memory_id {
            continue;
        }
        // Skip below threshold
        if *score < threshold {
            continue;
        }
        // Skip already related
        if already_related.contains(&candidate.id) {
            continue;
        }

        let relation = MemoryRelation {
            source_id: memory_id,
            target_id: candidate.id,
            relation_type: RelationType::Related,
            strength: *score,
        };
        if let Err(e) = storage.add_relation(&relation).await {
            tracing::debug!("semantic_auto_relate: failed to add relation: {e}");
            continue;
        }
        created += 1;
    }

    created
}

/// Default maximum traversal depth for `follow_chain`.
const DEFAULT_MAX_CHAIN_DEPTH: usize = 5;

/// A single link in a memory chain traversal.
#[derive(Debug, Clone)]
pub struct ChainLink {
    /// The memory at this position in the chain.
    pub memory_id: Uuid,
    /// The memory this link was reached from.
    pub from_id: Uuid,
    /// The type of relation that connects `from_id` → `memory_id`.
    pub relation_type: RelationType,
    /// The strength of the relation edge.
    pub strength: f32,
    /// How many hops from the starting memory (1-based).
    pub depth: usize,
}

/// Follow a chain of relations from a starting memory via BFS.
///
/// Traverses edges of the given types up to `max_depth` hops.
/// Returns links in order of discovery. Avoids cycles.
///
/// Use cases:
/// - Follow `Fixes` → `CausedBy` chains for debugging narratives
/// - Follow `Related` chains for knowledge exploration
/// - Follow `Supersedes` chains for version history
pub async fn follow_chain(
    storage: &impl StorageBackend,
    start_id: Uuid,
    relation_types: &[RelationType],
    max_depth: Option<usize>,
) -> Vec<ChainLink> {
    let max_depth = max_depth.unwrap_or(DEFAULT_MAX_CHAIN_DEPTH);

    let mut visited = HashSet::new();
    visited.insert(start_id);
    let mut queue = VecDeque::new();
    queue.push_back((start_id, 0usize));
    let mut chain = Vec::new();

    while let Some((current_id, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        let relations = match storage.get_relations(current_id).await {
            Ok(r) => r,
            Err(_) => continue,
        };

        for rel in &relations {
            if !relation_types.contains(&rel.relation_type) {
                continue;
            }

            let next_id = if rel.source_id == current_id {
                rel.target_id
            } else {
                rel.source_id
            };

            if visited.contains(&next_id) {
                continue;
            }
            visited.insert(next_id);

            chain.push(ChainLink {
                memory_id: next_id,
                from_id: current_id,
                relation_type: rel.relation_type,
                strength: rel.strength,
                depth: depth + 1,
            });

            queue.push_back((next_id, depth + 1));
        }
    }

    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::model::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Mock storage for graph tests. Supports configurable relations per node.
    struct MockGraphStorage {
        relations: Mutex<HashMap<Uuid, Vec<MemoryRelation>>>,
        added_relations: Mutex<Vec<MemoryRelation>>,
        search_results: Mutex<Vec<(Memory, f32)>>,
    }

    impl MockGraphStorage {
        fn new() -> Self {
            Self {
                relations: Mutex::new(HashMap::new()),
                added_relations: Mutex::new(Vec::new()),
                search_results: Mutex::new(Vec::new()),
            }
        }

        fn with_search_results(results: Vec<(Memory, f32)>) -> Self {
            let s = Self::new();
            *s.search_results.lock().unwrap() = results;
            s
        }

        fn add_mock_relation(&self, from: Uuid, to: Uuid, rtype: RelationType, strength: f32) {
            let rel = MemoryRelation {
                source_id: from,
                target_id: to,
                relation_type: rtype,
                strength,
            };
            self.relations
                .lock()
                .unwrap()
                .entry(from)
                .or_default()
                .push(rel);
        }

        fn added_count(&self) -> usize {
            self.added_relations.lock().unwrap().len()
        }
    }

    impl crate::storage::StorageBackend for MockGraphStorage {
        async fn save_memory(&self, _: &Memory, _: Option<&[f32]>) -> Result<()> {
            Ok(())
        }
        async fn get_memory(&self, _: Uuid) -> Result<Memory> {
            Err(crate::error::ShabkaError::NotFound("mock".into()))
        }
        async fn get_memories(&self, _: &[Uuid]) -> Result<Vec<Memory>> {
            Ok(Vec::new())
        }
        async fn update_memory(&self, _: Uuid, _: &UpdateMemoryInput) -> Result<Memory> {
            Err(crate::error::ShabkaError::NotFound("mock".into()))
        }
        async fn delete_memory(&self, _: Uuid) -> Result<()> {
            Ok(())
        }
        async fn vector_search(&self, _: &[f32], _: usize) -> Result<Vec<(Memory, f32)>> {
            Ok(self.search_results.lock().unwrap().clone())
        }
        async fn timeline(&self, _: &TimelineQuery) -> Result<Vec<TimelineEntry>> {
            Ok(Vec::new())
        }
        async fn add_relation(&self, rel: &MemoryRelation) -> Result<()> {
            self.added_relations.lock().unwrap().push(rel.clone());
            Ok(())
        }
        async fn get_relations(&self, memory_id: Uuid) -> Result<Vec<MemoryRelation>> {
            Ok(self
                .relations
                .lock()
                .unwrap()
                .get(&memory_id)
                .cloned()
                .unwrap_or_default())
        }
        async fn count_relations(&self, ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
            let rels = self.relations.lock().unwrap();
            Ok(ids
                .iter()
                .map(|id| (*id, rels.get(id).map(|v| v.len()).unwrap_or(0)))
                .collect())
        }
        async fn save_session(&self, _: &Session) -> Result<()> {
            Ok(())
        }
        async fn get_session(&self, _: Uuid) -> Result<Session> {
            Err(crate::error::ShabkaError::NotFound("mock".into()))
        }
    }

    fn make_memory(title: &str) -> Memory {
        Memory::new(
            title.to_string(),
            "test content".to_string(),
            MemoryKind::Observation,
            "test".to_string(),
        )
    }

    #[test]
    fn test_default_constants() {
        assert!(DEFAULT_SIMILARITY_THRESHOLD > 0.0 && DEFAULT_SIMILARITY_THRESHOLD < 1.0);
        assert!(DEFAULT_MAX_RELATIONS > 0);
        assert!(DEFAULT_MAX_CHAIN_DEPTH > 0);
    }

    #[test]
    fn test_chain_link_struct() {
        let link = ChainLink {
            memory_id: Uuid::nil(),
            from_id: Uuid::nil(),
            relation_type: RelationType::Fixes,
            strength: 0.9,
            depth: 1,
        };
        assert_eq!(link.depth, 1);
        assert_eq!(link.relation_type, RelationType::Fixes);
    }

    // -- follow_chain tests --

    #[tokio::test]
    async fn test_follow_chain_empty_graph() {
        let storage = MockGraphStorage::new();
        let start = Uuid::now_v7();

        let chain = follow_chain(&storage, start, &[RelationType::Related], None).await;
        assert!(chain.is_empty(), "empty graph should return no links");
    }

    #[tokio::test]
    async fn test_follow_chain_single_hop() {
        let storage = MockGraphStorage::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();

        storage.add_mock_relation(a, b, RelationType::Fixes, 0.9);

        let chain = follow_chain(&storage, a, &[RelationType::Fixes], None).await;
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].memory_id, b);
        assert_eq!(chain[0].from_id, a);
        assert_eq!(chain[0].depth, 1);
        assert_eq!(chain[0].relation_type, RelationType::Fixes);
    }

    #[tokio::test]
    async fn test_follow_chain_multi_hop() {
        let storage = MockGraphStorage::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let c = Uuid::now_v7();

        storage.add_mock_relation(a, b, RelationType::Related, 0.8);
        storage.add_mock_relation(b, c, RelationType::Related, 0.7);

        let chain = follow_chain(&storage, a, &[RelationType::Related], None).await;
        assert_eq!(chain.len(), 2);

        let ids: Vec<Uuid> = chain.iter().map(|l| l.memory_id).collect();
        assert!(ids.contains(&b));
        assert!(ids.contains(&c));

        // b should be depth 1, c should be depth 2
        let link_b = chain.iter().find(|l| l.memory_id == b).unwrap();
        let link_c = chain.iter().find(|l| l.memory_id == c).unwrap();
        assert_eq!(link_b.depth, 1);
        assert_eq!(link_c.depth, 2);
    }

    #[tokio::test]
    async fn test_follow_chain_cycle_detection() {
        let storage = MockGraphStorage::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();

        // a -> b -> a (cycle)
        storage.add_mock_relation(a, b, RelationType::Related, 0.8);
        storage.add_mock_relation(b, a, RelationType::Related, 0.8);

        let chain = follow_chain(&storage, a, &[RelationType::Related], None).await;
        // Should only visit b once, not loop
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].memory_id, b);
    }

    #[tokio::test]
    async fn test_follow_chain_depth_limit() {
        let storage = MockGraphStorage::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let c = Uuid::now_v7();
        let d = Uuid::now_v7();

        storage.add_mock_relation(a, b, RelationType::Related, 0.8);
        storage.add_mock_relation(b, c, RelationType::Related, 0.7);
        storage.add_mock_relation(c, d, RelationType::Related, 0.6);

        // Limit to depth 2 — should only reach b and c, not d
        let chain = follow_chain(&storage, a, &[RelationType::Related], Some(2)).await;
        let ids: Vec<Uuid> = chain.iter().map(|l| l.memory_id).collect();
        assert!(ids.contains(&b));
        assert!(ids.contains(&c));
        assert!(!ids.contains(&d), "d should be beyond depth limit");
    }

    #[tokio::test]
    async fn test_follow_chain_filters_by_relation_type() {
        let storage = MockGraphStorage::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let c = Uuid::now_v7();

        storage.add_mock_relation(a, b, RelationType::Fixes, 0.9);
        storage.add_mock_relation(a, c, RelationType::Related, 0.7);

        // Only follow Fixes relations
        let chain = follow_chain(&storage, a, &[RelationType::Fixes], None).await;
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].memory_id, b);
    }

    #[tokio::test]
    async fn test_follow_chain_multiple_relation_types() {
        let storage = MockGraphStorage::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let c = Uuid::now_v7();

        storage.add_mock_relation(a, b, RelationType::CausedBy, 0.9);
        storage.add_mock_relation(a, c, RelationType::Fixes, 0.8);

        let chain = follow_chain(
            &storage,
            a,
            &[RelationType::CausedBy, RelationType::Fixes],
            None,
        )
        .await;
        assert_eq!(chain.len(), 2);
    }

    #[tokio::test]
    async fn test_follow_chain_depth_zero() {
        let storage = MockGraphStorage::new();
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();

        storage.add_mock_relation(a, b, RelationType::Related, 0.8);

        // Depth 0 means don't traverse at all
        let chain = follow_chain(&storage, a, &[RelationType::Related], Some(0)).await;
        assert!(chain.is_empty());
    }

    // -- semantic_auto_relate tests --

    #[tokio::test]
    async fn test_auto_relate_no_results() {
        let storage = MockGraphStorage::new();
        let id = Uuid::now_v7();

        let created = semantic_auto_relate(&storage, id, &[0.0; 128], None, None).await;
        assert_eq!(created, 0);
    }

    #[tokio::test]
    async fn test_auto_relate_skips_self() {
        let m = make_memory("self");
        let storage = MockGraphStorage::with_search_results(vec![(m.clone(), 0.99)]);

        let created = semantic_auto_relate(&storage, m.id, &[0.0; 128], None, None).await;
        assert_eq!(created, 0, "should not relate to self");
    }

    #[tokio::test]
    async fn test_auto_relate_creates_relations() {
        let m1 = make_memory("source");
        let m2 = make_memory("similar");
        let m3 = make_memory("also similar");

        let storage = MockGraphStorage::with_search_results(vec![(m2, 0.8), (m3, 0.7)]);

        let created = semantic_auto_relate(&storage, m1.id, &[0.0; 128], Some(0.6), None).await;
        assert_eq!(created, 2);
        assert_eq!(storage.added_count(), 2);
    }

    #[tokio::test]
    async fn test_auto_relate_respects_threshold() {
        let m1 = make_memory("source");
        let m2 = make_memory("above threshold");
        let m3 = make_memory("below threshold");

        let storage = MockGraphStorage::with_search_results(vec![(m2, 0.8), (m3, 0.4)]);

        let created = semantic_auto_relate(&storage, m1.id, &[0.0; 128], Some(0.6), None).await;
        assert_eq!(created, 1, "should only relate the above-threshold match");
    }

    #[tokio::test]
    async fn test_auto_relate_respects_max_relations() {
        let m1 = make_memory("source");
        let results: Vec<(Memory, f32)> = (0..10)
            .map(|i| (make_memory(&format!("match {i}")), 0.8))
            .collect();
        let storage = MockGraphStorage::with_search_results(results);

        let created = semantic_auto_relate(&storage, m1.id, &[0.0; 128], Some(0.5), Some(2)).await;
        assert_eq!(created, 2, "should cap at max_relations=2");
    }

    #[tokio::test]
    async fn test_auto_relate_skips_existing_relations() {
        let m1 = make_memory("source");
        let m2 = make_memory("already related");

        let storage = MockGraphStorage::with_search_results(vec![(m2.clone(), 0.9)]);
        // Pre-add an existing relation
        storage.add_mock_relation(m1.id, m2.id, RelationType::Related, 0.8);

        let created = semantic_auto_relate(&storage, m1.id, &[0.0; 128], Some(0.5), None).await;
        assert_eq!(created, 0, "should skip already-related memory");
    }
}
