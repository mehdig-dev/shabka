use crate::error::{Result, ShabkaError};
use crate::model::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::StorageBackend;

/// HelixDB storage implementation.
///
/// Communicates with a running HelixDB instance via HTTP.
/// All queries are sent as named endpoints that map to pre-defined HelixQL queries.
pub struct HelixStorage {
    base_url: String,
    http: reqwest::Client,
}

impl HelixStorage {
    pub fn new(endpoint: Option<&str>, port: Option<u16>, _api_key: Option<&str>) -> Self {
        let host = endpoint.unwrap_or("http://localhost");
        let p = port.unwrap_or(6969);
        Self {
            base_url: format!("{host}:{p}"),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// Query HelixDB directly via reqwest with proper error context.
    /// Falls back from helix-rs to avoid its opaque deserialization errors.
    /// Wraps `query_once` with retry for transient errors.
    async fn query<T: Serialize + Sync, R: DeserializeOwned>(
        &self,
        endpoint: &str,
        data: &T,
    ) -> Result<R> {
        crate::retry::with_retry(3, 200, || self.query_once(endpoint, data)).await
    }

    /// Single attempt to query HelixDB.
    async fn query_once<T: Serialize, R: DeserializeOwned>(
        &self,
        endpoint: &str,
        data: &T,
    ) -> Result<R> {
        let url = format!("{}/{}", self.base_url, endpoint);
        let resp = self.http.post(&url).json(data).send().await?;
        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(ShabkaError::Storage(format!(
                "HelixDB {endpoint} returned {status}: {body}"
            )));
        }

        serde_json::from_str(&body).map_err(|e| {
            let preview = if body.len() > 300 {
                &body[..300]
            } else {
                &body
            };
            ShabkaError::Storage(format!(
                "Failed to deserialize {endpoint} response: {e}\nBody: {preview}"
            ))
        })
    }
}

// -- Request/Response types for HelixDB queries --

#[derive(Serialize)]
struct SaveMemoryRequest {
    id: String,
    kind: String,
    title: String,
    content: String,
    summary: String,
    tags: String,   // JSON array as string
    source: String, // JSON as string
    scope: String,  // JSON as string
    importance: f32,
    status: String,
    privacy: String,
    project_id: String,
    session_id: String,
    created_by: String,
    created_at: String,
    updated_at: String,
    accessed_at: String,
    verification: String,
    embedding: Vec<f32>,
}

#[derive(Serialize)]
struct SaveMemoryNodeRequest {
    id: String,
    kind: String,
    title: String,
    content: String,
    summary: String,
    tags: String,
    source: String,
    scope: String,
    importance: f32,
    status: String,
    privacy: String,
    project_id: String,
    session_id: String,
    created_by: String,
    created_at: String,
    updated_at: String,
    accessed_at: String,
    verification: String,
}

#[derive(Serialize)]
struct GetMemoryRequest {
    id: String,
}

#[derive(Serialize)]
struct GetMemoriesRequest {
    ids: Vec<String>,
}

#[derive(Serialize)]
struct DeleteMemoryRequest {
    id: String,
}

#[derive(Serialize)]
struct VectorSearchRequest {
    embedding: Vec<f32>,
    limit: usize,
}

#[derive(Serialize)]
struct TimelineRequest {
    limit: usize,
}

#[derive(Serialize)]
struct AddRelationRequest {
    source_id: String,
    target_id: String,
    relation_type: String,
    strength: f32,
}

#[derive(Serialize)]
struct GetRelationsRequest {
    memory_id: String,
}

#[derive(Serialize)]
struct SaveSessionRequest {
    id: String,
    project_id: String,
    started_at: String,
    ended_at: String,
    summary: String,
    memory_count: usize,
}

#[derive(Serialize)]
struct GetSessionRequest {
    id: String,
}

// -- Response wrappers --
// HelixDB returns query results as named fields matching the HelixQL RETURN clause.

#[derive(Deserialize)]
struct MemoryRecord {
    memory_id: String,
    kind: String,
    title: String,
    content: String,
    summary: String,
    tags: String,
    source: String,
    scope: String,
    importance: f32,
    status: String,
    privacy: String,
    project_id: Option<String>,
    session_id: Option<String>,
    created_by: String,
    created_at: String,
    updated_at: String,
    accessed_at: String,
    #[serde(default)]
    verification: Option<String>,
}

#[derive(Deserialize)]
struct MemoryQueryResult {
    #[serde(default)]
    memory: Vec<MemoryRecord>,
}

/// Single-node lookups (`N<Memory>({field: val})`) return an object, not an array.
#[derive(Deserialize)]
struct SingleMemoryResult {
    memory: MemoryRecord,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SearchResultRecord {
    memory_id: String,
    title: String,
    #[serde(default)]
    score: f32,
}

#[derive(Deserialize)]
struct SearchQueryResult {
    #[serde(default)]
    results: Vec<SearchResultRecord>,
}

/// `get_relations` returns source + target nodes (via `Out<RelatesTo>`) and
/// edge objects (via `OutE<RelatesTo>`) with relation_type and strength properties.
/// Edges and targets are paired by index (same traversal order from the source node).
#[derive(Deserialize)]
struct TraversalResult {
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<MemoryRecord>,
    #[serde(default)]
    target: Vec<MemoryRecord>,
    #[serde(default)]
    edges: Vec<EdgeRecord>,
}

/// Edge object returned by `::OutE<RelatesTo>` / `::InE<RelatesTo>`.
#[derive(Deserialize)]
struct EdgeRecord {
    #[serde(default)]
    relation_type: Option<String>,
    #[serde(default)]
    strength: Option<f32>,
}

#[derive(Deserialize)]
struct SessionRecord {
    session_id: String,
    project_id: Option<String>,
    started_at: String,
    ended_at: Option<String>,
    summary: Option<String>,
    memory_count: usize,
}

/// Single-node lookup for sessions.
#[derive(Deserialize)]
struct SingleSessionResult {
    session: SessionRecord,
}

#[derive(Deserialize)]
struct EmptyResult {}

// -- Conversion helpers --

fn record_to_memory(r: &MemoryRecord) -> Result<Memory> {
    use chrono::DateTime;

    Ok(Memory {
        id: Uuid::parse_str(&r.memory_id).map_err(|e| ShabkaError::Storage(e.to_string()))?,
        kind: r.kind.parse().map_err(ShabkaError::Storage)?,
        title: r.title.clone(),
        content: r.content.clone(),
        summary: r.summary.clone(),
        tags: serde_json::from_str(&r.tags).unwrap_or_default(),
        source: serde_json::from_str(&r.source).unwrap_or(MemorySource::Manual),
        scope: serde_json::from_str(&r.scope).unwrap_or(MemoryScope::Global),
        importance: r.importance,
        status: serde_json::from_str(&format!("\"{}\"", r.status)).unwrap_or(MemoryStatus::Active),
        privacy: serde_json::from_str(&format!("\"{}\"", r.privacy))
            .unwrap_or(MemoryPrivacy::Private),
        verification: r
            .verification
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default(),
        project_id: r.project_id.clone(),
        session_id: r.session_id.as_ref().and_then(|s| Uuid::parse_str(s).ok()),
        created_by: r.created_by.clone(),
        created_at: DateTime::parse_from_rfc3339(&r.created_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| ShabkaError::Storage(e.to_string()))?,
        updated_at: DateTime::parse_from_rfc3339(&r.updated_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| ShabkaError::Storage(e.to_string()))?,
        accessed_at: DateTime::parse_from_rfc3339(&r.accessed_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| ShabkaError::Storage(e.to_string()))?,
    })
}

// Vector search returns only MemoryEmbedding fields (memory_id, title, score).
// Full Memory records are fetched separately via get_memories.

fn record_to_session(r: &SessionRecord) -> Result<Session> {
    use chrono::DateTime;

    Ok(Session {
        id: Uuid::parse_str(&r.session_id).map_err(|e| ShabkaError::Storage(e.to_string()))?,
        project_id: r.project_id.clone(),
        started_at: DateTime::parse_from_rfc3339(&r.started_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| ShabkaError::Storage(e.to_string()))?,
        ended_at: r
            .ended_at
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        summary: r.summary.clone(),
        memory_count: r.memory_count,
    })
}

impl StorageBackend for HelixStorage {
    async fn save_memory(&self, memory: &Memory, embedding: Option<&[f32]>) -> Result<()> {
        let req = SaveMemoryRequest {
            id: memory.id.to_string(),
            kind: memory.kind.to_string(),
            title: memory.title.clone(),
            content: memory.content.clone(),
            summary: memory.summary.clone(),
            tags: serde_json::to_string(&memory.tags)?,
            source: serde_json::to_string(&memory.source)?,
            scope: serde_json::to_string(&memory.scope)?,
            importance: memory.importance,
            status: serde_json::to_string(&memory.status)?
                .trim_matches('"')
                .to_string(),
            privacy: serde_json::to_string(&memory.privacy)?
                .trim_matches('"')
                .to_string(),
            project_id: memory.project_id.clone().unwrap_or_default(),
            session_id: memory.session_id.map(|u| u.to_string()).unwrap_or_default(),
            created_by: memory.created_by.clone(),
            created_at: memory.created_at.to_rfc3339(),
            updated_at: memory.updated_at.to_rfc3339(),
            accessed_at: memory.accessed_at.to_rfc3339(),
            verification: memory.verification.to_string().to_lowercase(),
            embedding: embedding.map(|e| e.to_vec()).unwrap_or_default(),
        };

        let _: EmptyResult = self.query("save_memory", &req).await?;
        Ok(())
    }

    async fn get_memory(&self, id: Uuid) -> Result<Memory> {
        let req = GetMemoryRequest { id: id.to_string() };
        let result: SingleMemoryResult = self.query("get_memory", &req).await?;
        record_to_memory(&result.memory)
    }

    async fn get_memories(&self, ids: &[Uuid]) -> Result<Vec<Memory>> {
        let req = GetMemoriesRequest {
            ids: ids.iter().map(|id| id.to_string()).collect(),
        };
        let result: MemoryQueryResult = self.query("get_memories", &req).await?;
        result.memory.iter().map(record_to_memory).collect()
    }

    async fn update_memory(&self, id: Uuid, input: &UpdateMemoryInput) -> Result<Memory> {
        // Fetch existing, apply updates
        let mut memory = self.get_memory(id).await?;

        if let Some(title) = &input.title {
            memory.title = title.clone();
        }
        if let Some(content) = &input.content {
            memory.content = content.clone();
            if memory.summary == memory.content || memory.summary.ends_with("...") {
                memory.summary = if content.len() > 200 {
                    format!("{}...", &content[..200])
                } else {
                    content.clone()
                };
            }
        }
        if let Some(tags) = &input.tags {
            memory.tags = tags.clone();
        }
        if let Some(importance) = input.importance {
            memory.importance = importance.clamp(0.0, 1.0);
        }
        if let Some(status) = input.status {
            memory.status = status;
        }
        if let Some(kind) = input.kind {
            memory.kind = kind;
        }
        if let Some(privacy) = input.privacy {
            memory.privacy = privacy;
        }
        if let Some(verification) = input.verification {
            memory.verification = verification;
        }
        memory.updated_at = chrono::Utc::now();

        // HelixDB has no UPDATE — delete old node, then create new one (node-only, preserves vector).
        self.delete_memory(id).await?;

        let req = SaveMemoryNodeRequest {
            id: memory.id.to_string(),
            kind: memory.kind.to_string(),
            title: memory.title.clone(),
            content: memory.content.clone(),
            summary: memory.summary.clone(),
            tags: serde_json::to_string(&memory.tags)?,
            source: serde_json::to_string(&memory.source)?,
            scope: serde_json::to_string(&memory.scope)?,
            importance: memory.importance,
            status: serde_json::to_string(&memory.status)?
                .trim_matches('"')
                .to_string(),
            privacy: serde_json::to_string(&memory.privacy)?
                .trim_matches('"')
                .to_string(),
            project_id: memory.project_id.clone().unwrap_or_default(),
            session_id: memory.session_id.map(|u| u.to_string()).unwrap_or_default(),
            created_by: memory.created_by.clone(),
            created_at: memory.created_at.to_rfc3339(),
            updated_at: memory.updated_at.to_rfc3339(),
            accessed_at: memory.accessed_at.to_rfc3339(),
            verification: memory.verification.to_string().to_lowercase(),
        };

        let _: EmptyResult = self.query("save_memory_node", &req).await?;
        Ok(memory)
    }

    async fn delete_memory(&self, id: Uuid) -> Result<()> {
        let req = DeleteMemoryRequest { id: id.to_string() };
        // RETURN NONE yields `null` — use Value to skip typed deserialization.
        let _: serde_json::Value = self.query("delete_memory", &req).await?;
        Ok(())
    }

    async fn vector_search(&self, embedding: &[f32], limit: usize) -> Result<Vec<(Memory, f32)>> {
        let req = VectorSearchRequest {
            embedding: embedding.to_vec(),
            limit,
        };
        let result: SearchQueryResult = self.query("search_memories", &req).await?;

        // Vector search returns only MemoryEmbedding fields; fetch full records
        let ids: Vec<Uuid> = result
            .results
            .iter()
            .filter_map(|r| Uuid::parse_str(&r.memory_id).ok())
            .collect();
        let scores: std::collections::HashMap<Uuid, f32> = result
            .results
            .iter()
            .filter_map(|r| Uuid::parse_str(&r.memory_id).ok().map(|id| (id, r.score)))
            .collect();

        let memories = self.get_memories(&ids).await?;
        Ok(memories
            .into_iter()
            .map(|m| {
                let score = scores.get(&m.id).copied().unwrap_or(0.0);
                (m, score)
            })
            .collect())
    }

    async fn timeline(&self, query: &TimelineQuery) -> Result<Vec<TimelineEntry>> {
        // Fetch all memories; HelixDB RANGE doesn't guarantee chronological order,
        // so we sort and filter in Rust.
        let req = TimelineRequest { limit: 1000 };
        let result: MemoryQueryResult = self.query("timeline", &req).await?;

        let mut memories: Vec<Memory> = result
            .memory
            .iter()
            .map(record_to_memory)
            .collect::<Result<Vec<_>>>()?;

        // Apply date and session filters in Rust
        if let Some(start) = query.start {
            memories.retain(|m| m.created_at >= start);
        }
        if let Some(end) = query.end {
            memories.retain(|m| m.created_at <= end);
        }
        if let Some(sid) = query.session_id {
            memories.retain(|m| m.session_id == Some(sid));
        }
        if let Some(ref pid) = query.project_id {
            memories.retain(|m| m.project_id.as_ref() == Some(pid));
        }
        memories.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        memories.truncate(query.limit);

        // Batch-fetch relation counts
        let ids: Vec<Uuid> = memories.iter().map(|m| m.id).collect();
        let counts = self.count_relations(&ids).await?;
        let count_map: std::collections::HashMap<Uuid, usize> = counts.into_iter().collect();

        Ok(memories
            .iter()
            .map(|memory| {
                let related_count = count_map.get(&memory.id).copied().unwrap_or(0);
                TimelineEntry::from((memory, related_count))
            })
            .collect())
    }

    async fn add_relation(&self, relation: &MemoryRelation) -> Result<()> {
        let req = AddRelationRequest {
            source_id: relation.source_id.to_string(),
            target_id: relation.target_id.to_string(),
            relation_type: relation.relation_type.to_string(),
            strength: relation.strength,
        };
        let _: EmptyResult = self.query("add_relation", &req).await?;
        Ok(())
    }

    async fn get_relations(&self, memory_id: Uuid) -> Result<Vec<MemoryRelation>> {
        let req = GetRelationsRequest {
            memory_id: memory_id.to_string(),
        };
        // `Out<RelatesTo>` returns target nodes, `OutE<RelatesTo>` returns edge objects.
        // Edges and targets are paired by index (same traversal order).
        let result: TraversalResult = self.query("get_relations", &req).await?;

        result
            .target
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let edge = result.edges.get(i);
                Ok(MemoryRelation {
                    source_id: memory_id,
                    target_id: Uuid::parse_str(&t.memory_id)
                        .map_err(|e| ShabkaError::Storage(e.to_string()))?,
                    relation_type: edge
                        .and_then(|e| e.relation_type.as_deref())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(RelationType::Related),
                    strength: edge.and_then(|e| e.strength).unwrap_or(0.5),
                })
            })
            .collect()
    }

    async fn count_relations(&self, memory_ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
        let mut counts = Vec::with_capacity(memory_ids.len());
        for &id in memory_ids {
            let relations = self.get_relations(id).await.unwrap_or_default();
            counts.push((id, relations.len()));
        }
        Ok(counts)
    }

    async fn count_contradictions(&self, memory_ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
        let mut counts = Vec::with_capacity(memory_ids.len());
        for &id in memory_ids {
            let relations = self.get_relations(id).await.unwrap_or_default();
            let contradiction_count = relations
                .iter()
                .filter(|r| r.relation_type == RelationType::Contradicts)
                .count();
            counts.push((id, contradiction_count));
        }
        Ok(counts)
    }

    async fn save_session(&self, session: &Session) -> Result<()> {
        let req = SaveSessionRequest {
            id: session.id.to_string(),
            project_id: session.project_id.clone().unwrap_or_default(),
            started_at: session.started_at.to_rfc3339(),
            ended_at: session
                .ended_at
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
            summary: session.summary.clone().unwrap_or_default(),
            memory_count: session.memory_count,
        };
        let _: EmptyResult = self.query("save_session", &req).await?;
        Ok(())
    }

    async fn get_session(&self, id: Uuid) -> Result<Session> {
        let req = GetSessionRequest { id: id.to_string() };
        let result: SingleSessionResult = self.query("get_session", &req).await?;
        record_to_session(&result.session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- EdgeRecord deserialization --

    #[test]
    fn test_edge_record_full() {
        let json = r#"{"relation_type": "contradicts", "strength": 0.9}"#;
        let edge: EdgeRecord = serde_json::from_str(json).unwrap();
        assert_eq!(edge.relation_type.as_deref(), Some("contradicts"));
        assert_eq!(edge.strength, Some(0.9));
    }

    #[test]
    fn test_edge_record_missing_fields() {
        let json = r#"{}"#;
        let edge: EdgeRecord = serde_json::from_str(json).unwrap();
        assert!(edge.relation_type.is_none());
        assert!(edge.strength.is_none());
    }

    #[test]
    fn test_edge_record_partial() {
        let json = r#"{"relation_type": "fixes"}"#;
        let edge: EdgeRecord = serde_json::from_str(json).unwrap();
        assert_eq!(edge.relation_type.as_deref(), Some("fixes"));
        assert!(edge.strength.is_none());
    }

    // -- TraversalResult deserialization --

    #[test]
    fn test_traversal_result_with_edges() {
        let json = r#"{
            "source": {"memory_id": "00000000-0000-0000-0000-000000000001", "kind": "observation", "title": "src", "content": "", "summary": "", "tags": "[]", "source": "\"manual\"", "scope": "\"global\"", "importance": 0.5, "status": "active", "privacy": "private", "created_by": "test", "created_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-01T00:00:00Z", "accessed_at": "2025-01-01T00:00:00Z"},
            "target": [
                {"memory_id": "00000000-0000-0000-0000-000000000002", "kind": "observation", "title": "tgt", "content": "", "summary": "", "tags": "[]", "source": "\"manual\"", "scope": "\"global\"", "importance": 0.5, "status": "active", "privacy": "private", "created_by": "test", "created_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-01T00:00:00Z", "accessed_at": "2025-01-01T00:00:00Z"}
            ],
            "edges": [
                {"relation_type": "contradicts", "strength": 0.85}
            ]
        }"#;
        let result: TraversalResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.target.len(), 1);
        assert_eq!(result.edges.len(), 1);
        assert_eq!(
            result.edges[0].relation_type.as_deref(),
            Some("contradicts")
        );
        assert_eq!(result.edges[0].strength, Some(0.85));
    }

    #[test]
    fn test_traversal_result_no_edges_backward_compat() {
        // Simulates old HelixDB responses before edge queries were added
        let json = r#"{
            "source": {"memory_id": "00000000-0000-0000-0000-000000000001", "kind": "observation", "title": "src", "content": "", "summary": "", "tags": "[]", "source": "\"manual\"", "scope": "\"global\"", "importance": 0.5, "status": "active", "privacy": "private", "created_by": "test", "created_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-01T00:00:00Z", "accessed_at": "2025-01-01T00:00:00Z"},
            "target": [
                {"memory_id": "00000000-0000-0000-0000-000000000002", "kind": "observation", "title": "tgt", "content": "", "summary": "", "tags": "[]", "source": "\"manual\"", "scope": "\"global\"", "importance": 0.5, "status": "active", "privacy": "private", "created_by": "test", "created_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-01T00:00:00Z", "accessed_at": "2025-01-01T00:00:00Z"}
            ]
        }"#;
        let result: TraversalResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.target.len(), 1);
        assert!(result.edges.is_empty(), "edges should default to empty vec");
    }

    // -- Edge-target pairing logic --

    /// Replicates the pairing logic from get_relations() for unit testing
    /// without needing an HTTP backend.
    fn pair_edges_with_targets(source_id: Uuid, result: &TraversalResult) -> Vec<MemoryRelation> {
        result
            .target
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                let edge = result.edges.get(i);
                Some(MemoryRelation {
                    source_id,
                    target_id: Uuid::parse_str(&t.memory_id).ok()?,
                    relation_type: edge
                        .and_then(|e| e.relation_type.as_deref())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(RelationType::Related),
                    strength: edge.and_then(|e| e.strength).unwrap_or(0.5),
                })
            })
            .collect()
    }

    #[test]
    fn test_pairing_matches_edge_properties() {
        let source_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let target_id = "00000000-0000-0000-0000-000000000002".to_string();

        let result = TraversalResult {
            source: None,
            target: vec![make_test_record(&target_id)],
            edges: vec![EdgeRecord {
                relation_type: Some("contradicts".to_string()),
                strength: Some(0.9),
            }],
        };

        let relations = pair_edges_with_targets(source_id, &result);
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].relation_type, RelationType::Contradicts);
        assert!((relations[0].strength - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_pairing_defaults_when_edges_missing() {
        let source_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let target_id = "00000000-0000-0000-0000-000000000002".to_string();

        let result = TraversalResult {
            source: None,
            target: vec![make_test_record(&target_id)],
            edges: vec![], // No edge data
        };

        let relations = pair_edges_with_targets(source_id, &result);
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].relation_type, RelationType::Related);
        assert!((relations[0].strength - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_pairing_multiple_edges_mixed_types() {
        let source_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let t1 = "00000000-0000-0000-0000-000000000002".to_string();
        let t2 = "00000000-0000-0000-0000-000000000003".to_string();
        let t3 = "00000000-0000-0000-0000-000000000004".to_string();

        let result = TraversalResult {
            source: None,
            target: vec![
                make_test_record(&t1),
                make_test_record(&t2),
                make_test_record(&t3),
            ],
            edges: vec![
                EdgeRecord {
                    relation_type: Some("fixes".to_string()),
                    strength: Some(0.8),
                },
                EdgeRecord {
                    relation_type: Some("contradicts".to_string()),
                    strength: Some(0.7),
                },
                EdgeRecord {
                    relation_type: Some("caused_by".to_string()),
                    strength: Some(0.6),
                },
            ],
        };

        let relations = pair_edges_with_targets(source_id, &result);
        assert_eq!(relations.len(), 3);
        assert_eq!(relations[0].relation_type, RelationType::Fixes);
        assert_eq!(relations[1].relation_type, RelationType::Contradicts);
        assert_eq!(relations[2].relation_type, RelationType::CausedBy);
        assert!((relations[0].strength - 0.8).abs() < f32::EPSILON);
        assert!((relations[1].strength - 0.7).abs() < f32::EPSILON);
        assert!((relations[2].strength - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn test_pairing_unknown_relation_type_defaults() {
        let source_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let target_id = "00000000-0000-0000-0000-000000000002".to_string();

        let result = TraversalResult {
            source: None,
            target: vec![make_test_record(&target_id)],
            edges: vec![EdgeRecord {
                relation_type: Some("unknown_type".to_string()),
                strength: Some(0.3),
            }],
        };

        let relations = pair_edges_with_targets(source_id, &result);
        assert_eq!(relations.len(), 1);
        // Unknown type falls back to Related
        assert_eq!(relations[0].relation_type, RelationType::Related);
        // Strength is still parsed from edge
        assert!((relations[0].strength - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn test_pairing_fewer_edges_than_targets() {
        let source_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let t1 = "00000000-0000-0000-0000-000000000002".to_string();
        let t2 = "00000000-0000-0000-0000-000000000003".to_string();

        let result = TraversalResult {
            source: None,
            target: vec![make_test_record(&t1), make_test_record(&t2)],
            edges: vec![EdgeRecord {
                relation_type: Some("fixes".to_string()),
                strength: Some(0.8),
            }],
            // Only 1 edge for 2 targets — second target gets defaults
        };

        let relations = pair_edges_with_targets(source_id, &result);
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0].relation_type, RelationType::Fixes);
        assert!((relations[0].strength - 0.8).abs() < f32::EPSILON);
        // Second target falls back to defaults
        assert_eq!(relations[1].relation_type, RelationType::Related);
        assert!((relations[1].strength - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_contradiction_count_from_relations() {
        let source_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let t1 = "00000000-0000-0000-0000-000000000002".to_string();
        let t2 = "00000000-0000-0000-0000-000000000003".to_string();
        let t3 = "00000000-0000-0000-0000-000000000004".to_string();

        let result = TraversalResult {
            source: None,
            target: vec![
                make_test_record(&t1),
                make_test_record(&t2),
                make_test_record(&t3),
            ],
            edges: vec![
                EdgeRecord {
                    relation_type: Some("contradicts".to_string()),
                    strength: Some(0.9),
                },
                EdgeRecord {
                    relation_type: Some("fixes".to_string()),
                    strength: Some(0.8),
                },
                EdgeRecord {
                    relation_type: Some("contradicts".to_string()),
                    strength: Some(0.7),
                },
            ],
        };

        let relations = pair_edges_with_targets(source_id, &result);
        let contradiction_count = relations
            .iter()
            .filter(|r| r.relation_type == RelationType::Contradicts)
            .count();
        assert_eq!(contradiction_count, 2);
    }

    // -- record_to_memory verification parsing --

    #[test]
    fn test_record_to_memory_parses_verification() {
        let record = MemoryRecord {
            memory_id: "00000000-0000-0000-0000-000000000001".to_string(),
            kind: "observation".to_string(),
            title: "test".to_string(),
            content: "content".to_string(),
            summary: "summary".to_string(),
            tags: "[]".to_string(),
            source: "\"manual\"".to_string(),
            scope: "\"global\"".to_string(),
            importance: 0.5,
            status: "active".to_string(),
            privacy: "private".to_string(),
            project_id: None,
            session_id: None,
            created_by: "test".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
            accessed_at: "2025-01-01T00:00:00Z".to_string(),
            verification: Some("verified".to_string()),
        };
        let memory = record_to_memory(&record).unwrap();
        assert_eq!(memory.verification, VerificationStatus::Verified);
    }

    #[test]
    fn test_record_to_memory_defaults_missing_verification() {
        let record = MemoryRecord {
            memory_id: "00000000-0000-0000-0000-000000000001".to_string(),
            kind: "observation".to_string(),
            title: "test".to_string(),
            content: "content".to_string(),
            summary: "summary".to_string(),
            tags: "[]".to_string(),
            source: "\"manual\"".to_string(),
            scope: "\"global\"".to_string(),
            importance: 0.5,
            status: "active".to_string(),
            privacy: "private".to_string(),
            project_id: None,
            session_id: None,
            created_by: "test".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
            accessed_at: "2025-01-01T00:00:00Z".to_string(),
            verification: None,
        };
        let memory = record_to_memory(&record).unwrap();
        assert_eq!(memory.verification, VerificationStatus::Unverified);
    }

    // -- Helper --

    fn make_test_record(memory_id: &str) -> MemoryRecord {
        MemoryRecord {
            memory_id: memory_id.to_string(),
            kind: "observation".to_string(),
            title: "test".to_string(),
            content: "".to_string(),
            summary: "".to_string(),
            tags: "[]".to_string(),
            source: "\"manual\"".to_string(),
            scope: "\"global\"".to_string(),
            importance: 0.5,
            status: "active".to_string(),
            privacy: "private".to_string(),
            project_id: None,
            session_id: None,
            created_by: "test".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
            accessed_at: "2025-01-01T00:00:00Z".to_string(),
            verification: None,
        }
    }
}
