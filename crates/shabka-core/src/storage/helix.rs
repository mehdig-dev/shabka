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

/// `get_relations` traverses `Out<RelatesTo>` and returns source node + target nodes.
/// Edge properties (relation_type, strength) are NOT included in the traversal response.
#[derive(Deserialize)]
struct TraversalResult {
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<MemoryRecord>,
    #[serde(default)]
    target: Vec<MemoryRecord>,
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
        verification: VerificationStatus::default(),
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
        // HQL `Out<RelatesTo>` returns source + target nodes; edge properties are not included.
        // We construct relations from the connected nodes with default edge metadata.
        let result: TraversalResult = self.query("get_relations", &req).await?;

        result
            .target
            .iter()
            .map(|t| {
                Ok(MemoryRelation {
                    source_id: memory_id,
                    target_id: Uuid::parse_str(&t.memory_id)
                        .map_err(|e| ShabkaError::Storage(e.to_string()))?,
                    relation_type: RelationType::Related,
                    strength: 0.5,
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
