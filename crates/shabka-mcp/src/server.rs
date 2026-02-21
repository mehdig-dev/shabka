use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;
use shabka_core::assess::{self, AssessConfig, IssueCounts};
use shabka_core::config::{self, EmbeddingState, ShabkaConfig};
use shabka_core::context_pack::{build_context_pack, format_context_pack};
use shabka_core::dedup::{self, DedupDecision};
use shabka_core::embedding::EmbeddingService;
use shabka_core::error::ShabkaError;
use shabka_core::graph;
use shabka_core::history::{EventAction, HistoryLogger, MemoryEvent};
use shabka_core::llm::LlmService;
use shabka_core::model::*;
use shabka_core::ranking::{self, RankCandidate, RankingWeights};
use shabka_core::sharing;
use shabka_core::storage::{create_backend, Storage, StorageBackend};
use uuid::Uuid;

#[derive(Clone)]
pub struct ShabkaServer {
    storage: Arc<Storage>,
    embedder: Arc<EmbeddingService>,
    config: Arc<ShabkaConfig>,
    user_id: String,
    tool_router: ToolRouter<Self>,
    migration_checked: Arc<AtomicBool>,
    history: Arc<HistoryLogger>,
    llm: Option<Arc<LlmService>>,
}

// -- Tool parameter types --

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaveMemoryParams {
    #[schemars(description = "Short, searchable title for this memory")]
    pub title: String,

    #[schemars(description = "Full content in markdown format")]
    pub content: String,

    #[schemars(
        description = "Kind of memory: observation, decision, pattern, error, fix, preference, fact, lesson, or todo"
    )]
    pub kind: String,

    #[schemars(description = "Tags for categorization (optional)")]
    #[serde(default)]
    pub tags: Vec<String>,

    #[schemars(description = "Importance score 0.0-1.0 (optional, default 0.5)")]
    #[serde(default = "default_importance")]
    pub importance: f32,

    #[schemars(description = "Scope: 'global' or a project ID (optional)")]
    #[serde(default)]
    pub scope: Option<String>,

    #[schemars(description = "IDs of related memories to link (optional)")]
    #[serde(default)]
    pub related_to: Vec<String>,

    #[schemars(description = "Privacy level: public, team, private (default from config)")]
    #[serde(default)]
    pub privacy: Option<String>,

    #[schemars(description = "Project ID to associate this memory with (optional)")]
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    #[schemars(description = "Search query text for semantic + keyword matching")]
    pub query: String,

    #[schemars(description = "Filter by memory kind (optional)")]
    #[serde(default)]
    pub kind: Option<String>,

    #[schemars(description = "Filter by project ID (optional)")]
    #[serde(default)]
    pub project_id: Option<String>,

    #[schemars(description = "Filter by tags (optional)")]
    #[serde(default)]
    pub tags: Vec<String>,

    #[schemars(description = "Max results (default 10)")]
    #[serde(default = "default_limit")]
    pub limit: usize,

    #[schemars(
        description = "Cap results to fit within a token budget (estimated ~4 chars/token). Omit for no budget limit."
    )]
    #[serde(default)]
    pub token_budget: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetMemoriesParams {
    #[schemars(description = "List of memory IDs to retrieve full details for")]
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateMemoryParams {
    #[schemars(description = "ID of the memory to update")]
    pub id: String,

    #[schemars(description = "New title (optional)")]
    #[serde(default)]
    pub title: Option<String>,

    #[schemars(description = "New content (optional)")]
    #[serde(default)]
    pub content: Option<String>,

    #[schemars(description = "New tags (optional, replaces existing)")]
    #[serde(default)]
    pub tags: Option<Vec<String>>,

    #[schemars(description = "New importance 0.0-1.0 (optional)")]
    #[serde(default)]
    pub importance: Option<f32>,

    #[schemars(description = "New status: active, archived, superseded (optional)")]
    #[serde(default)]
    pub status: Option<String>,

    #[schemars(description = "New privacy level: public, team, private (optional)")]
    #[serde(default)]
    pub privacy: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteMemoryParams {
    #[schemars(description = "ID of the memory to permanently delete")]
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RelateMemoriesParams {
    #[schemars(description = "Source memory ID")]
    pub source_id: String,

    #[schemars(description = "Target memory ID")]
    pub target_id: String,

    #[schemars(
        description = "Relationship type: caused_by, fixes, supersedes, related, contradicts"
    )]
    pub relation_type: String,

    #[schemars(description = "Relationship strength 0.0-1.0 (default 0.5)")]
    #[serde(default = "default_importance")]
    pub strength: f32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TimelineParams {
    #[schemars(description = "Center timeline around this memory ID (optional)")]
    #[serde(default)]
    pub memory_id: Option<String>,

    #[schemars(description = "Start of time range in RFC3339 format (optional)")]
    #[serde(default)]
    pub start: Option<String>,

    #[schemars(description = "End of time range in RFC3339 format (optional)")]
    #[serde(default)]
    pub end: Option<String>,

    #[schemars(description = "Filter by session ID (optional)")]
    #[serde(default)]
    pub session_id: Option<String>,

    #[schemars(description = "Filter by project ID (optional)")]
    #[serde(default)]
    pub project_id: Option<String>,

    #[schemars(description = "Max results (default 10)")]
    #[serde(default = "default_limit")]
    pub limit: usize,

    #[schemars(description = "Sort order: 'created_at' (default) or 'importance'")]
    #[serde(default = "default_order_by")]
    pub order_by: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReembedParams {
    #[schemars(
        description = "Force full re-embed, ignoring incremental skip logic (default false)"
    )]
    #[serde(default)]
    pub force: bool,

    #[schemars(description = "Number of memories to process per batch (default 10)")]
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HistoryParams {
    #[schemars(
        description = "Memory ID to get history for (optional — if omitted, returns recent events across all memories)"
    )]
    #[serde(default)]
    pub memory_id: Option<String>,

    #[schemars(description = "Max events to return (default 20)")]
    #[serde(default = "default_history_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FollowChainParams {
    #[schemars(description = "Starting memory ID to follow chains from")]
    pub memory_id: String,

    #[schemars(
        description = "Relation types to follow: caused_by, fixes, supersedes, related, contradicts. Default: all types."
    )]
    #[serde(default)]
    pub relation_types: Vec<String>,

    #[schemars(description = "Maximum traversal depth (default 5)")]
    #[serde(default)]
    pub max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsolidateParams {
    #[schemars(
        description = "Show what would be consolidated without making changes (default false)"
    )]
    #[serde(default)]
    pub dry_run: bool,

    #[schemars(description = "Minimum cluster size to consolidate (default 3)")]
    #[serde(default)]
    pub min_cluster_size: Option<usize>,

    #[schemars(description = "Minimum age in days before memories are eligible (default 7)")]
    #[serde(default)]
    pub min_age_days: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AssessParams {
    #[schemars(description = "Max memories to analyze (default: all)")]
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct VerifyMemoryParams {
    #[schemars(description = "Memory ID to verify")]
    pub id: String,

    #[schemars(description = "Verification status: verified, disputed, outdated, or unverified")]
    pub status: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetContextParams {
    #[schemars(
        description = "Search query for semantic + keyword matching. Omit or use '*' for top memories by recency/importance."
    )]
    #[serde(default = "default_context_query")]
    pub query: String,

    #[schemars(description = "Filter by project ID (optional)")]
    #[serde(default)]
    pub project_id: Option<String>,

    #[schemars(description = "Filter by memory kind (optional)")]
    #[serde(default)]
    pub kind: Option<String>,

    #[schemars(description = "Comma-separated tag filter (optional)")]
    #[serde(default)]
    pub tags: Option<String>,

    #[schemars(description = "Max tokens in the context pack (default 2000)")]
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionMemoryInput {
    #[schemars(description = "Short, searchable title for this memory")]
    pub title: String,

    #[schemars(description = "Full content in markdown format")]
    pub content: String,

    #[schemars(
        description = "Kind of memory: observation, decision, pattern, error, fix, preference, fact, lesson, or todo"
    )]
    pub kind: String,

    #[schemars(description = "Tags for categorization (optional)")]
    #[serde(default)]
    pub tags: Vec<String>,

    #[schemars(description = "Importance score 0.0-1.0 (optional, default 0.5)")]
    #[serde(default = "default_importance")]
    pub importance: f32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaveSessionSummaryParams {
    #[schemars(description = "Array of memories to save from this session")]
    pub memories: Vec<SessionMemoryInput>,

    #[schemars(description = "Brief description of what the session was about (optional)")]
    #[serde(default)]
    pub session_context: Option<String>,

    #[schemars(description = "Project ID to associate all memories with (optional)")]
    #[serde(default)]
    pub project_id: Option<String>,
}

fn default_context_query() -> String {
    "*".to_string()
}

fn default_token_budget() -> usize {
    2000
}

fn default_history_limit() -> usize {
    20
}
fn default_batch_size() -> usize {
    10
}

fn default_importance() -> f32 {
    0.5
}
fn default_limit() -> usize {
    10
}
fn default_order_by() -> String {
    "created_at".to_string()
}

fn to_mcp_error(e: ShabkaError) -> ErrorData {
    match &e {
        ShabkaError::NotFound(_) => ErrorData::resource_not_found(
            e.to_string(),
            Some(serde_json::json!({"error_type": "not_found"})),
        ),
        ShabkaError::InvalidInput(_) => ErrorData::invalid_params(
            e.to_string(),
            Some(serde_json::json!({"error_type": "invalid_input"})),
        ),
        ShabkaError::Config(_) => ErrorData::invalid_params(
            e.to_string(),
            Some(serde_json::json!({"error_type": "config_error"})),
        ),
        _ => {
            let variant = match &e {
                ShabkaError::Storage(_) => "storage_error",
                ShabkaError::Helix(_) => "helix_error",
                ShabkaError::Http(_) => "http_error",
                ShabkaError::Embedding(_) => "embedding_error",
                ShabkaError::Serialization(_) => "serialization_error",
                _ => "internal_error",
            };
            ErrorData::internal_error(
                e.to_string(),
                Some(serde_json::json!({"error_type": variant})),
            )
        }
    }
}

#[tool_router]
impl ShabkaServer {
    pub fn new() -> anyhow::Result<Self> {
        let config = ShabkaConfig::load(Some(&std::env::current_dir()?))
            .unwrap_or_else(|_| ShabkaConfig::default_config());

        let storage = create_backend(&config)?;

        let embedder = EmbeddingService::from_config(&config.embedding)?;
        let user_id = config::resolve_user_id(&config.sharing);
        let history = HistoryLogger::new(config.history.enabled);

        let llm = if config.llm.enabled {
            LlmService::from_config(&config.llm).ok().map(Arc::new)
        } else {
            None
        };

        Ok(Self {
            storage: Arc::new(storage),
            embedder: Arc::new(embedder),
            user_id,
            history: Arc::new(history),
            llm,
            config: Arc::new(config),
            tool_router: Self::tool_router(),
            migration_checked: Arc::new(AtomicBool::new(false)),
        })
    }

    // -- Layer 1: Index (compact search results, ~50-100 tokens each) --

    #[tool(
        description = "Search memories by semantic similarity and keywords. Returns compact index entries (id, title, kind, date, score). Use get_memories to retrieve full details for specific IDs. Filters: kind (observation/decision/pattern/error/fix/preference/fact/lesson/todo), project_id, tags, limit. Always start here before using get_memories."
    )]
    async fn search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Embed the query text
        let embedding = self
            .embedder
            .embed(&params.query)
            .await
            .map_err(to_mcp_error)?;

        // Over-fetch 3x to have enough candidates after filtering
        let fetch_limit = params.limit * 3;

        let results = self
            .storage
            .vector_search(&embedding, fetch_limit)
            .await
            .map_err(to_mcp_error)?;

        // Filter by privacy, kind, project, tags
        let mut filtered: Vec<(Memory, f32)> = results;
        sharing::filter_search_results(&mut filtered, &self.user_id);
        let filtered: Vec<(Memory, f32)> = filtered
            .into_iter()
            .filter(|(memory, _)| {
                if let Some(ref kind) = params.kind {
                    if memory.kind.to_string() != *kind {
                        return false;
                    }
                }
                if let Some(ref pid) = params.project_id {
                    if memory.project_id.as_ref() != Some(pid) {
                        return false;
                    }
                }
                if !params.tags.is_empty() && !params.tags.iter().any(|t| memory.tags.contains(t)) {
                    return false;
                }
                true
            })
            .collect();

        // Get relation counts for ranking
        let memory_ids: Vec<Uuid> = filtered.iter().map(|(m, _)| m.id).collect();
        let relation_counts = self
            .storage
            .count_relations(&memory_ids)
            .await
            .map_err(to_mcp_error)?;

        let count_map: std::collections::HashMap<Uuid, usize> =
            relation_counts.into_iter().collect();

        let contradiction_counts = self
            .storage
            .count_contradictions(&memory_ids)
            .await
            .map_err(to_mcp_error)?;

        let contradiction_map: std::collections::HashMap<Uuid, usize> =
            contradiction_counts.into_iter().collect();

        // Build rank candidates with keyword scoring
        let candidates: Vec<RankCandidate> = filtered
            .into_iter()
            .map(|(memory, vector_score)| {
                let id = memory.id;
                let relation_count = count_map.get(&id).copied().unwrap_or(0);
                let contradiction_count = contradiction_map.get(&id).copied().unwrap_or(0);
                let kw_score = ranking::keyword_score(&params.query, &memory);
                RankCandidate {
                    memory,
                    vector_score,
                    keyword_score: kw_score,
                    relation_count,
                    contradiction_count,
                }
            })
            .collect();

        // Rank and take top N
        let ranked = ranking::rank(candidates, &RankingWeights::default());
        let top: Vec<MemoryIndex> = ranked
            .into_iter()
            .take(params.limit)
            .map(|r| MemoryIndex::from((&r.memory, r.score)))
            .collect();

        // Apply token budget if set
        let top = match params.token_budget {
            Some(budget) => ranking::budget_truncate(top, budget),
            None => top,
        };

        let json = serde_json::to_string_pretty(&top)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // -- Layer 2: Context (timeline with summaries, ~200-300 tokens each) --

    #[tool(
        description = "Get chronological context around a memory or time range. Returns timeline entries with summaries. Use this for understanding the narrative flow of a session or tracking how ideas evolved over time."
    )]
    async fn timeline(
        &self,
        Parameters(params): Parameters<TimelineParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = TimelineQuery {
            memory_id: params.memory_id.and_then(|s| Uuid::parse_str(&s).ok()),
            start: params
                .start
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            end: params
                .end
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            session_id: params.session_id.and_then(|s| Uuid::parse_str(&s).ok()),
            limit: params.limit,
            project_id: params.project_id,
        };

        let mut entries = self.storage.timeline(&query).await.map_err(to_mcp_error)?;

        // Filter by privacy
        entries.retain(|e| sharing::is_visible(e.privacy, &e.created_by, &self.user_id));

        // Sort by requested order
        if params.order_by == "importance" {
            entries.sort_by(|a, b| {
                b.importance
                    .partial_cmp(&a.importance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        let json = serde_json::to_string_pretty(&entries)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // -- Layer 3: Detail (full content + relationships, ~500-1000 tokens each) --

    #[tool(
        description = "Retrieve full memory details including content and relationships for specific IDs. Use search first to identify relevant memories, then get_memories for the ones you need full details on."
    )]
    async fn get_memories(
        &self,
        Parameters(params): Parameters<GetMemoriesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let ids: Vec<Uuid> = params
            .ids
            .iter()
            .map(|s| Uuid::parse_str(s))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| ErrorData::invalid_params(format!("invalid UUID: {e}"), None))?;

        let mut memories = self
            .storage
            .get_memories(&ids)
            .await
            .map_err(to_mcp_error)?;

        // Filter by privacy
        sharing::filter_memories(&mut memories, &self.user_id);

        let mut results = Vec::new();
        for memory in &memories {
            let relations = self
                .storage
                .get_relations(memory.id)
                .await
                .unwrap_or_default();
            results.push(MemoryWithRelations {
                memory: memory.clone(),
                relations,
            });
        }

        let json = serde_json::to_string_pretty(&results)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // -- Write operations --

    #[tool(
        description = "Save a new memory. Provide a title, content (markdown), and kind. Optionally add tags, importance (0.0-1.0), scope, and related memory IDs."
    )]
    async fn save_memory(
        &self,
        Parameters(params): Parameters<SaveMemoryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let kind: MemoryKind = params
            .kind
            .parse()
            .map_err(|e: String| ErrorData::invalid_params(e, None))?;

        shabka_core::model::validate_create_input(
            &params.title,
            &params.content,
            params.importance,
        )
        .map_err(to_mcp_error)?;

        let privacy = params
            .privacy
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| sharing::parse_default_privacy(&self.config.privacy));

        // Check if user provided meaningful tags before moving them
        let user_tags_empty = params.tags.is_empty()
            || params
                .tags
                .iter()
                .all(|t| t == "auto-capture" || t == "hook");

        let mut memory = Memory::new(params.title, params.content, kind, self.user_id.clone())
            .with_tags(params.tags)
            .with_importance(params.importance)
            .with_privacy(privacy);

        if let Some(scope) = params.scope {
            if scope != "global" {
                memory = memory.with_scope(MemoryScope::Project { id: scope });
            }
        }

        if let Some(project_id) = params.project_id {
            memory = memory.with_project(project_id);
        }

        // Auto-tag with LLM if user provided no meaningful tags
        if user_tags_empty {
            if let Some(ref llm) = self.llm {
                if let Some(result) = shabka_core::auto_tag::auto_tag(&memory, llm.as_ref()).await {
                    let mut tags = memory.tags.clone();
                    for tag in result.tags {
                        if !tags.contains(&tag) {
                            tags.push(tag);
                        }
                    }
                    memory.tags = tags;
                    memory.importance = result.importance;
                }
            }
        }

        // Generate embedding from the memory's text representation
        let embedding = self
            .embedder
            .embed(&memory.embedding_text())
            .await
            .map_err(to_mcp_error)?;

        // Check for embedding migration once per session
        if !self.migration_checked.swap(true, Ordering::Relaxed) {
            if let Some(warning) = EmbeddingState::migration_warning(
                &self.config.embedding,
                self.embedder.dimensions(),
            ) {
                eprintln!("{warning}");
            }
        }

        // Smart dedup check
        let llm_ref = self.llm.as_deref();
        let dedup_decision = dedup::check_duplicate(
            self.storage.as_ref(),
            &embedding,
            &self.config.graph,
            None,
            llm_ref,
            &memory.title,
            &memory.content,
        )
        .await;

        match dedup_decision {
            DedupDecision::Skip {
                existing_id,
                existing_title,
                similarity,
            } => {
                let response = serde_json::json!({
                    "action": "skipped",
                    "existing_id": existing_id.to_string(),
                    "existing_title": existing_title,
                    "similarity": similarity,
                    "message": "Near-duplicate found — memory not saved.",
                });
                return Ok(CallToolResult::success(vec![Content::text(
                    response.to_string(),
                )]));
            }
            DedupDecision::Supersede {
                existing_id,
                existing_title,
                similarity,
            } => {
                // Save the new memory
                self.storage
                    .save_memory(&memory, Some(&embedding))
                    .await
                    .map_err(to_mcp_error)?;

                // Mark old as superseded
                let _ = self
                    .storage
                    .update_memory(
                        existing_id,
                        &UpdateMemoryInput {
                            status: Some(MemoryStatus::Superseded),
                            ..Default::default()
                        },
                    )
                    .await;

                // Add Supersedes relation
                let relation = MemoryRelation {
                    source_id: memory.id,
                    target_id: existing_id,
                    relation_type: RelationType::Supersedes,
                    strength: similarity,
                };
                let _ = self.storage.add_relation(&relation).await;

                // Log history events
                self.history.log(
                    &MemoryEvent::new(memory.id, EventAction::Created, self.user_id.clone())
                        .with_title(&memory.title),
                );
                self.history.log(
                    &MemoryEvent::new(existing_id, EventAction::Superseded, self.user_id.clone())
                        .with_title(&existing_title),
                );

                let response = serde_json::json!({
                    "action": "superseded",
                    "id": memory.id.to_string(),
                    "title": memory.title,
                    "superseded_id": existing_id.to_string(),
                    "superseded_title": existing_title,
                    "similarity": similarity,
                });
                return Ok(CallToolResult::success(vec![Content::text(
                    response.to_string(),
                )]));
            }
            DedupDecision::Update {
                existing_id,
                existing_title,
                merged_content,
                merged_title,
                similarity,
            } => {
                // Update existing memory with LLM-merged content
                let _ = self
                    .storage
                    .update_memory(
                        existing_id,
                        &UpdateMemoryInput {
                            title: Some(merged_title.clone()),
                            content: Some(merged_content.clone()),
                            ..Default::default()
                        },
                    )
                    .await;

                self.history.log(
                    &MemoryEvent::new(existing_id, EventAction::Updated, self.user_id.clone())
                        .with_title(&merged_title),
                );

                let response = serde_json::json!({
                    "action": "merged",
                    "existing_id": existing_id.to_string(),
                    "existing_title": existing_title,
                    "merged_title": merged_title,
                    "similarity": similarity,
                    "message": "New info merged into existing memory.",
                });
                return Ok(CallToolResult::success(vec![Content::text(
                    response.to_string(),
                )]));
            }
            DedupDecision::Contradict {
                existing_id,
                existing_title,
                similarity,
                reason,
            } => {
                // Save the new (corrected) memory
                self.storage
                    .save_memory(&memory, Some(&embedding))
                    .await
                    .map_err(to_mcp_error)?;

                // Add Contradicts relation
                let relation = MemoryRelation {
                    source_id: memory.id,
                    target_id: existing_id,
                    relation_type: RelationType::Contradicts,
                    strength: similarity,
                };
                let _ = self.storage.add_relation(&relation).await;

                // Log history events
                self.history.log(
                    &MemoryEvent::new(memory.id, EventAction::Created, self.user_id.clone())
                        .with_title(&memory.title),
                );

                let response = serde_json::json!({
                    "action": "contradicted",
                    "id": memory.id.to_string(),
                    "title": memory.title,
                    "contradicted_id": existing_id.to_string(),
                    "contradicted_title": existing_title,
                    "similarity": similarity,
                    "reason": reason,
                    "message": "New memory saved. Contradicts existing memory — review recommended.",
                });
                return Ok(CallToolResult::success(vec![Content::text(
                    response.to_string(),
                )]));
            }
            DedupDecision::Add => {}
        }

        self.storage
            .save_memory(&memory, Some(&embedding))
            .await
            .map_err(to_mcp_error)?;

        // Update embedding state after successful save
        let state = EmbeddingState::from_config(&self.config.embedding, self.embedder.dimensions());
        let _ = state.save();

        for related_id in &params.related_to {
            if let Ok(target_id) = Uuid::parse_str(related_id) {
                let relation = MemoryRelation {
                    source_id: memory.id,
                    target_id,
                    relation_type: RelationType::Related,
                    strength: 0.5,
                };
                let _ = self.storage.add_relation(&relation).await;
            }
        }

        // Log history event
        self.history.log(
            &MemoryEvent::new(memory.id, EventAction::Created, self.user_id.clone())
                .with_title(&memory.title),
        );

        // Semantic auto-relate: find similar memories and link them
        let auto_related = graph::semantic_auto_relate(
            self.storage.as_ref(),
            memory.id,
            &embedding,
            Some(self.config.graph.similarity_threshold),
            Some(self.config.graph.max_relations),
        )
        .await;

        let response = serde_json::json!({
            "action": "added",
            "id": memory.id.to_string(),
            "title": memory.title,
            "kind": memory.kind.to_string(),
            "created_at": memory.created_at.to_rfc3339(),
            "auto_related": auto_related,
        });

        Ok(CallToolResult::success(vec![Content::text(
            response.to_string(),
        )]))
    }

    #[tool(
        description = "Update an existing memory. Provide the memory ID and any fields to change: title, content, tags, importance, status."
    )]
    async fn update_memory(
        &self,
        Parameters(params): Parameters<UpdateMemoryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = Uuid::parse_str(&params.id)
            .map_err(|e| ErrorData::invalid_params(format!("invalid UUID: {e}"), None))?;

        let needs_reembed = params.title.is_some() || params.content.is_some();

        // Fetch old memory for diff
        let old_memory = self.storage.get_memory(id).await.map_err(to_mcp_error)?;

        let status = params
            .status
            .map(|s| {
                serde_json::from_str::<MemoryStatus>(&format!("\"{s}\""))
                    .map_err(|_| ErrorData::invalid_params(format!("invalid status: {s}"), None))
            })
            .transpose()?;

        let privacy = params.privacy.and_then(|s| s.parse().ok());

        let input = UpdateMemoryInput {
            title: params.title,
            content: params.content,
            tags: params.tags,
            importance: params.importance,
            status,
            kind: None,
            privacy,
            verification: None,
        };

        shabka_core::model::validate_update_input(&input).map_err(to_mcp_error)?;

        let memory = self
            .storage
            .update_memory(id, &input)
            .await
            .map_err(to_mcp_error)?;

        // Log history event
        let changes = shabka_core::history::diff_update(&old_memory, &input);
        self.history.log(
            &MemoryEvent::new(memory.id, EventAction::Updated, self.user_id.clone())
                .with_title(&memory.title)
                .with_changes(changes),
        );

        // Re-embed if title or content changed
        if needs_reembed {
            let embedding = self
                .embedder
                .embed(&memory.embedding_text())
                .await
                .map_err(to_mcp_error)?;
            // Re-save with the new embedding
            self.storage
                .save_memory(&memory, Some(&embedding))
                .await
                .map_err(to_mcp_error)?;
        }

        let response = serde_json::json!({
            "id": memory.id.to_string(),
            "title": memory.title,
            "updated_at": memory.updated_at.to_rfc3339(),
        });

        Ok(CallToolResult::success(vec![Content::text(
            response.to_string(),
        )]))
    }

    #[tool(description = "Permanently delete a memory by ID. This cannot be undone.")]
    async fn delete_memory(
        &self,
        Parameters(params): Parameters<DeleteMemoryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = Uuid::parse_str(&params.id)
            .map_err(|e| ErrorData::invalid_params(format!("invalid UUID: {e}"), None))?;

        // Fetch title before deleting for audit trail
        let title = self
            .storage
            .get_memory(id)
            .await
            .ok()
            .map(|m| m.title.clone());

        self.storage.delete_memory(id).await.map_err(to_mcp_error)?;

        let mut event = MemoryEvent::new(id, EventAction::Deleted, self.user_id.clone());
        if let Some(t) = title {
            event = event.with_title(t);
        }
        self.history.log(&event);

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Memory {id} deleted."
        ))]))
    }

    #[tool(
        description = "Re-embed all (or changed) memories with the current embedding provider. Use after changing provider config or to refresh embeddings. Returns counts of processed/skipped/errored memories."
    )]
    async fn reembed(
        &self,
        Parameters(params): Parameters<ReembedParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let saved_state = EmbeddingState::load();
        let provider_changed = !saved_state.provider.is_empty()
            && !saved_state.matches_config(&self.config.embedding, self.embedder.dimensions());
        let full_reembed =
            params.force || provider_changed || saved_state.last_reembed_at.is_empty();

        // Fetch all memories
        let entries = self
            .storage
            .timeline(&TimelineQuery {
                limit: 10000,
                ..Default::default()
            })
            .await
            .map_err(to_mcp_error)?;

        if entries.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No memories to re-embed.",
            )]));
        }

        let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
        let all_memories = self
            .storage
            .get_memories(&ids)
            .await
            .map_err(to_mcp_error)?;

        // Filter to memories needing re-embed
        let (memories, skipped) = if full_reembed {
            (all_memories, 0usize)
        } else {
            let cutoff = chrono::DateTime::parse_from_rfc3339(&saved_state.last_reembed_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::DateTime::<chrono::Utc>::MIN_UTC);
            let mut to_embed = Vec::new();
            let mut skip_count = 0usize;
            for m in all_memories {
                if m.updated_at > cutoff {
                    to_embed.push(m);
                } else {
                    skip_count += 1;
                }
            }
            (to_embed, skip_count)
        };

        if memories.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "All {} memories are up to date. Nothing to re-embed.",
                skipped
            ))]));
        }

        let mut processed = 0usize;
        let mut errors = 0usize;

        for chunk in memories.chunks(params.batch_size) {
            let texts: Vec<String> = chunk.iter().map(|m| m.embedding_text()).collect();
            let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

            let embeddings = match self.embedder.embed_batch(&text_refs).await {
                Ok(embs) => embs,
                Err(_) => {
                    let mut single_embs = Vec::with_capacity(chunk.len());
                    for text in &text_refs {
                        match self.embedder.embed(text).await {
                            Ok(emb) => single_embs.push(emb),
                            Err(_) => {
                                errors += 1;
                                single_embs.push(Vec::new());
                            }
                        }
                    }
                    single_embs
                }
            };

            for (memory, embedding) in chunk.iter().zip(embeddings.iter()) {
                if embedding.is_empty() {
                    continue;
                }
                match self.storage.save_memory(memory, Some(embedding)).await {
                    Ok(()) => processed += 1,
                    Err(_) => errors += 1,
                }
            }
        }

        // Update state
        let mut state =
            EmbeddingState::from_config(&self.config.embedding, self.embedder.dimensions());
        state.last_reembed_at = chrono::Utc::now().to_rfc3339();
        let _ = state.save();

        let response = serde_json::json!({
            "processed": processed,
            "skipped": skipped,
            "errors": errors,
            "provider": self.embedder.provider_name(),
            "model": self.embedder.model_id(),
            "mode": if full_reembed { "full" } else { "incremental" },
        });

        Ok(CallToolResult::success(vec![Content::text(
            response.to_string(),
        )]))
    }

    #[tool(
        description = "Create a relationship between two memories. Types: caused_by, fixes, supersedes, related, contradicts. Strength 0.0-1.0."
    )]
    async fn relate_memories(
        &self,
        Parameters(params): Parameters<RelateMemoriesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let source_id = Uuid::parse_str(&params.source_id)
            .map_err(|e| ErrorData::invalid_params(format!("invalid source UUID: {e}"), None))?;
        let target_id = Uuid::parse_str(&params.target_id)
            .map_err(|e| ErrorData::invalid_params(format!("invalid target UUID: {e}"), None))?;
        let relation_type: RelationType = params
            .relation_type
            .parse()
            .map_err(|e: String| ErrorData::invalid_params(e, None))?;

        let relation = MemoryRelation {
            source_id,
            target_id,
            relation_type,
            strength: params.strength.clamp(0.0, 1.0),
        };

        self.storage
            .add_relation(&relation)
            .await
            .map_err(to_mcp_error)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Linked {} -[{}]-> {}",
            source_id, relation.relation_type, target_id
        ))]))
    }

    #[tool(
        description = "Get the audit history of memory mutations. Returns chronological events (created, updated, deleted, archived, superseded). Optionally filter by a specific memory ID."
    )]
    async fn history(
        &self,
        Parameters(params): Parameters<HistoryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let events = if let Some(ref id_str) = params.memory_id {
            let memory_id = Uuid::parse_str(id_str)
                .map_err(|e| ErrorData::invalid_params(format!("invalid UUID: {e}"), None))?;
            self.history.history_for(memory_id)
        } else {
            self.history.recent(params.limit)
        };

        let events: Vec<&shabka_core::history::MemoryEvent> =
            events.iter().take(params.limit).collect();

        let json = serde_json::to_string_pretty(&events)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Follow a chain of relations from a starting memory. BFS traversal for debugging narratives (fixes/caused_by), knowledge exploration (related), or version history (supersedes). Returns linked memories with relation types and depth."
    )]
    async fn follow_chain(
        &self,
        Parameters(params): Parameters<FollowChainParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let start_id = Uuid::parse_str(&params.memory_id)
            .map_err(|e| ErrorData::invalid_params(format!("invalid UUID: {e}"), None))?;

        // Parse relation type filters (default: all types)
        let relation_types: Vec<RelationType> = if params.relation_types.is_empty() {
            vec![
                RelationType::CausedBy,
                RelationType::Fixes,
                RelationType::Supersedes,
                RelationType::Related,
                RelationType::Contradicts,
            ]
        } else {
            params
                .relation_types
                .iter()
                .map(|s| {
                    s.parse::<RelationType>()
                        .map_err(|e| ErrorData::invalid_params(e, None))
                })
                .collect::<Result<Vec<_>, _>>()?
        };

        let max_depth = params
            .max_depth
            .unwrap_or(self.config.graph.max_chain_depth);
        let chain = graph::follow_chain(
            self.storage.as_ref(),
            start_id,
            &relation_types,
            Some(max_depth),
        )
        .await;

        if chain.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No connected memories found.",
            )]));
        }

        // Fetch full memory details for each chain link
        let chain_ids: Vec<Uuid> = chain.iter().map(|l| l.memory_id).collect();
        let memories = self
            .storage
            .get_memories(&chain_ids)
            .await
            .map_err(to_mcp_error)?;

        let memory_map: std::collections::HashMap<Uuid, &Memory> =
            memories.iter().map(|m| (m.id, m)).collect();

        let results: Vec<serde_json::Value> = chain
            .iter()
            .filter_map(|link| {
                memory_map.get(&link.memory_id).map(|memory| {
                    serde_json::json!({
                        "id": memory.id.to_string(),
                        "title": memory.title,
                        "kind": memory.kind.to_string(),
                        "summary": memory.summary,
                        "relation_type": link.relation_type.to_string(),
                        "from_id": link.from_id.to_string(),
                        "strength": link.strength,
                        "depth": link.depth,
                    })
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&results)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Consolidate clusters of similar memories into comprehensive summaries. Requires LLM to be enabled. Finds groups of related memories via vector similarity, merges each cluster into a single comprehensive memory, and supersedes the originals."
    )]
    async fn consolidate(
        &self,
        Parameters(params): Parameters<ConsolidateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let llm = self.llm.as_ref().ok_or_else(|| {
            ErrorData::invalid_params(
                "Consolidation requires LLM. Enable it in config.toml under [llm].".to_string(),
                None,
            )
        })?;

        let mut config = self.config.consolidate.clone();
        if let Some(min) = params.min_cluster_size {
            config.min_cluster_size = min;
        }
        if let Some(age) = params.min_age_days {
            config.min_age_days = age;
        }

        let result = shabka_core::consolidate::consolidate(
            self.storage.as_ref(),
            self.embedder.as_ref(),
            llm.as_ref(),
            &config,
            &self.user_id,
            &self.history,
            params.dry_run,
        )
        .await
        .map_err(to_mcp_error)?;

        let response = serde_json::json!({
            "clusters_found": result.clusters_found,
            "clusters_consolidated": result.clusters_consolidated,
            "memories_superseded": result.memories_superseded,
            "memories_created": result.memories_created,
            "mode": if params.dry_run { "dry_run" } else { "applied" },
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Assess memory quality. Returns a scorecard with issue counts, overall score (0-100), and top issues. Use this to identify memories that need improvement (generic titles, missing tags, short content, stale, orphaned)."
    )]
    async fn assess(
        &self,
        Parameters(params): Parameters<AssessParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let fetch_limit = params.limit.unwrap_or(10000);

        // Fetch all memories via timeline
        let entries = self
            .storage
            .timeline(&TimelineQuery {
                limit: fetch_limit,
                ..Default::default()
            })
            .await
            .map_err(to_mcp_error)?;

        if entries.is_empty() {
            let response = serde_json::json!({
                "total_memories": 0,
                "memories_with_issues": 0,
                "score": 100,
                "counts": IssueCounts::default(),
                "top_issues": [],
            });
            return Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&response)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            )]));
        }

        let ids: Vec<Uuid> = entries.iter().map(|e| e.id).collect();
        let memories = self
            .storage
            .get_memories(&ids)
            .await
            .map_err(to_mcp_error)?;

        let total = memories.len();

        // Get relation counts
        let all_ids: Vec<Uuid> = memories.iter().map(|m| m.id).collect();
        let relation_counts = self
            .storage
            .count_relations(&all_ids)
            .await
            .map_err(to_mcp_error)?;
        let count_map: std::collections::HashMap<Uuid, usize> =
            relation_counts.into_iter().collect();

        let assess_config = AssessConfig {
            stale_days: self.config.graph.stale_days,
            ..AssessConfig::default()
        };

        // Analyze each memory
        let results: Vec<assess::AssessmentResult> = memories
            .iter()
            .filter_map(|m| {
                let rel_count = count_map.get(&m.id).copied().unwrap_or(0);
                let issues = assess::analyze_memory(m, &assess_config, rel_count);
                if issues.is_empty() {
                    None
                } else {
                    Some(assess::AssessmentResult {
                        memory_id: m.id,
                        title: m.title.clone(),
                        issues,
                    })
                }
            })
            .collect();

        let score = assess::quality_score(&results, total);
        let counts = IssueCounts::from_results(&results);

        // Top 10 issues
        let mut sorted = results.clone();
        sorted.sort_by(|a, b| b.issues.len().cmp(&a.issues.len()));
        let top_issues: Vec<serde_json::Value> = sorted
            .iter()
            .take(10)
            .map(|r| {
                let labels: Vec<&str> = r.issues.iter().map(|i| i.label()).collect();
                serde_json::json!({
                    "id": r.memory_id.to_string(),
                    "title": r.title,
                    "issues": labels,
                })
            })
            .collect();

        let response = serde_json::json!({
            "total_memories": total,
            "memories_with_issues": results.len(),
            "score": score,
            "counts": counts,
            "top_issues": top_issues,
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        name = "verify_memory",
        description = "Set verification status on a memory (verified, disputed, outdated, unverified). Verified memories rank higher in search results."
    )]
    async fn verify_memory(
        &self,
        Parameters(params): Parameters<VerifyMemoryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = Uuid::parse_str(&params.id)
            .map_err(|e| ErrorData::invalid_params(format!("invalid memory ID: {e}"), None))?;

        let verification: VerificationStatus = params
            .status
            .parse()
            .map_err(|e: String| ErrorData::invalid_params(e, None))?;

        // Fetch old state for audit trail before updating
        let old_memory = self.storage.get_memory(id).await.map_err(to_mcp_error)?;

        let input = UpdateMemoryInput {
            verification: Some(verification),
            ..Default::default()
        };

        let memory = self
            .storage
            .update_memory(id, &input)
            .await
            .map_err(to_mcp_error)?;

        self.history.log(
            &MemoryEvent::new(id, EventAction::Updated, self.user_id.clone())
                .with_title(&memory.title)
                .with_changes(vec![shabka_core::history::FieldChange {
                    field: "verification".to_string(),
                    old_value: old_memory.verification.to_string(),
                    new_value: verification.to_string(),
                }]),
        );

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Memory '{}' marked as {verification}",
            memory.title
        ))]))
    }

    #[tool(
        name = "get_context",
        description = "Get a token-budgeted context pack of relevant memories, formatted as markdown ready for injection into prompts. Supports filtering by query, project, kind, and tags. Use this when you need rich context rather than individual search results."
    )]
    async fn get_context(
        &self,
        Parameters(params): Parameters<GetContextParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = if params.query.is_empty() {
            "*"
        } else {
            &params.query
        };

        let embedding = self.embedder.embed(query).await.map_err(to_mcp_error)?;

        let mut results = self
            .storage
            .vector_search(&embedding, 50)
            .await
            .map_err(to_mcp_error)?;

        sharing::filter_search_results(&mut results, &self.user_id);

        let tag_filter: Vec<String> = params
            .tags
            .map(|t| {
                t.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let filtered: Vec<(Memory, f32)> = results
            .into_iter()
            .filter(|(memory, _)| {
                if let Some(ref kind) = params.kind {
                    if memory.kind.to_string() != *kind {
                        return false;
                    }
                }
                if let Some(ref pid) = params.project_id {
                    if memory.project_id.as_ref() != Some(pid) {
                        return false;
                    }
                }
                if !tag_filter.is_empty() && !tag_filter.iter().any(|t| memory.tags.contains(t)) {
                    return false;
                }
                true
            })
            .collect();

        let memory_ids: Vec<Uuid> = filtered.iter().map(|(m, _)| m.id).collect();
        let relation_counts = self
            .storage
            .count_relations(&memory_ids)
            .await
            .map_err(to_mcp_error)?;
        let count_map: std::collections::HashMap<Uuid, usize> =
            relation_counts.into_iter().collect();
        let contradiction_counts = self
            .storage
            .count_contradictions(&memory_ids)
            .await
            .map_err(to_mcp_error)?;
        let contradiction_map: std::collections::HashMap<Uuid, usize> =
            contradiction_counts.into_iter().collect();

        let candidates: Vec<RankCandidate> = filtered
            .into_iter()
            .map(|(memory, vector_score)| {
                let kw_score = ranking::keyword_score(query, &memory);
                RankCandidate {
                    relation_count: count_map.get(&memory.id).copied().unwrap_or(0),
                    contradiction_count: contradiction_map.get(&memory.id).copied().unwrap_or(0),
                    keyword_score: kw_score,
                    memory,
                    vector_score,
                }
            })
            .collect();

        let ranked = ranking::rank(candidates, &RankingWeights::default());
        let memories: Vec<Memory> = ranked.into_iter().map(|r| r.memory).collect();

        let pack = build_context_pack(memories, params.token_budget, params.project_id);

        if pack.memories.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No memories found matching the query and filters within the token budget.",
            )]));
        }

        let formatted = format_context_pack(&pack);
        Ok(CallToolResult::success(vec![Content::text(formatted)]))
    }

    #[tool(
        name = "save_session_summary",
        description = "Save multiple memories from a session at once. Use this at the end of a conversation to persist what was learned — decisions, patterns, fixes, observations. Each memory goes through the same pipeline as save_memory (embed, dedup, auto-relate). Returns a summary of what was saved, skipped, or merged."
    )]
    async fn save_session_summary(
        &self,
        Parameters(params): Parameters<SaveSessionSummaryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = Uuid::now_v7();
        let mut saved = 0usize;
        let mut skipped = 0usize;
        let mut superseded = 0usize;
        let mut errors = Vec::new();

        for (i, input) in params.memories.iter().enumerate() {
            let kind: MemoryKind = match input.kind.parse() {
                Ok(k) => k,
                Err(e) => {
                    errors.push(format!("memory[{i}]: invalid kind — {e}"));
                    continue;
                }
            };

            if let Err(e) = shabka_core::model::validate_create_input(
                &input.title,
                &input.content,
                input.importance,
            ) {
                errors.push(format!("memory[{i}]: {e}"));
                continue;
            }

            let privacy = sharing::parse_default_privacy(&self.config.privacy);

            let mut memory = Memory::new(
                input.title.clone(),
                input.content.clone(),
                kind,
                self.user_id.clone(),
            )
            .with_tags(input.tags.clone())
            .with_importance(input.importance)
            .with_privacy(privacy)
            .with_session(session_id);

            if let Some(ref pid) = params.project_id {
                memory = memory.with_project(pid.clone());
            }

            // Auto-tag if no meaningful tags provided
            let user_tags_empty = input.tags.is_empty()
                || input
                    .tags
                    .iter()
                    .all(|t| t == "auto-capture" || t == "hook");
            if user_tags_empty {
                if let Some(ref llm) = self.llm {
                    if let Some(result) =
                        shabka_core::auto_tag::auto_tag(&memory, llm.as_ref()).await
                    {
                        let mut tags = memory.tags.clone();
                        for tag in result.tags {
                            if !tags.contains(&tag) {
                                tags.push(tag);
                            }
                        }
                        memory.tags = tags;
                        memory.importance = result.importance;
                    }
                }
            }

            // Embed
            let embedding = match self.embedder.embed(&memory.embedding_text()).await {
                Ok(e) => e,
                Err(e) => {
                    errors.push(format!("memory[{i}]: embed failed — {e}"));
                    continue;
                }
            };

            // Dedup check
            let llm_ref = self.llm.as_deref();
            let dedup_decision = dedup::check_duplicate(
                self.storage.as_ref(),
                &embedding,
                &self.config.graph,
                None,
                llm_ref,
                &memory.title,
                &memory.content,
            )
            .await;

            match dedup_decision {
                DedupDecision::Skip { .. } => {
                    skipped += 1;
                    continue;
                }
                DedupDecision::Supersede {
                    existing_id,
                    existing_title,
                    similarity,
                    ..
                } => {
                    if let Err(e) = self.storage.save_memory(&memory, Some(&embedding)).await {
                        errors.push(format!("memory[{i}]: save failed — {e}"));
                        continue;
                    }
                    let _ = self
                        .storage
                        .update_memory(
                            existing_id,
                            &UpdateMemoryInput {
                                status: Some(MemoryStatus::Superseded),
                                ..Default::default()
                            },
                        )
                        .await;
                    let relation = MemoryRelation {
                        source_id: memory.id,
                        target_id: existing_id,
                        relation_type: RelationType::Supersedes,
                        strength: similarity,
                    };
                    let _ = self.storage.add_relation(&relation).await;
                    self.history.log(
                        &MemoryEvent::new(memory.id, EventAction::Created, self.user_id.clone())
                            .with_title(&memory.title),
                    );
                    self.history.log(
                        &MemoryEvent::new(
                            existing_id,
                            EventAction::Superseded,
                            self.user_id.clone(),
                        )
                        .with_title(&existing_title),
                    );
                    superseded += 1;
                    saved += 1;
                }
                DedupDecision::Update {
                    existing_id,
                    merged_content,
                    merged_title,
                    ..
                } => {
                    let _ = self
                        .storage
                        .update_memory(
                            existing_id,
                            &UpdateMemoryInput {
                                title: Some(merged_title.clone()),
                                content: Some(merged_content),
                                ..Default::default()
                            },
                        )
                        .await;
                    self.history.log(
                        &MemoryEvent::new(existing_id, EventAction::Updated, self.user_id.clone())
                            .with_title(&merged_title),
                    );
                    saved += 1;
                }
                DedupDecision::Contradict {
                    existing_id,
                    similarity,
                    ..
                } => {
                    if let Err(e) = self.storage.save_memory(&memory, Some(&embedding)).await {
                        errors.push(format!("memory[{i}]: save failed — {e}"));
                        continue;
                    }
                    let relation = MemoryRelation {
                        source_id: memory.id,
                        target_id: existing_id,
                        relation_type: RelationType::Contradicts,
                        strength: similarity,
                    };
                    let _ = self.storage.add_relation(&relation).await;
                    self.history.log(
                        &MemoryEvent::new(memory.id, EventAction::Created, self.user_id.clone())
                            .with_title(&memory.title),
                    );
                    saved += 1;
                }
                DedupDecision::Add => {
                    if let Err(e) = self.storage.save_memory(&memory, Some(&embedding)).await {
                        errors.push(format!("memory[{i}]: save failed — {e}"));
                        continue;
                    }
                    self.history.log(
                        &MemoryEvent::new(memory.id, EventAction::Created, self.user_id.clone())
                            .with_title(&memory.title),
                    );

                    // Auto-relate
                    let _ = graph::semantic_auto_relate(
                        self.storage.as_ref(),
                        memory.id,
                        &embedding,
                        Some(self.config.graph.similarity_threshold),
                        Some(self.config.graph.max_relations),
                    )
                    .await;

                    saved += 1;
                }
            }
        }

        let response = serde_json::json!({
            "session_id": session_id.to_string(),
            "session_context": params.session_context,
            "total_submitted": params.memories.len(),
            "saved": saved,
            "skipped_duplicates": skipped,
            "superseded_existing": superseded,
            "errors": errors,
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }
}

#[tool_handler]
impl ServerHandler for ShabkaServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Shabka is a shared LLM memory system. Use progressive disclosure for efficient retrieval:\n\n\
                 1. **search** (Layer 1 - Index): Start here. Returns compact entries (~50-100 tokens each).\n\n\
                 2. **timeline** (Layer 2 - Context): Chronological context around a memory or time range.\n\n\
                 3. **get_memories** (Layer 3 - Detail): Full content + relationships for specific IDs.\n\n\
                 Write operations: save_memory, update_memory, delete_memory, relate_memories.\n\n\
                 Graph traversal: follow_chain (BFS along typed edges for debugging narratives).\n\n\
                 Audit trail: history (chronological mutation events).\n\n\
                 Maintenance: reembed (re-embed memories after provider change).\n\n\
                 Quality: assess (scorecard with issue counts and overall score).\n\n\
                 Trust: verify_memory (set verified/disputed/outdated status — verified memories rank higher).\n\n\
                 Context: get_context (token-budgeted context pack of relevant memories for prompt injection).\n\n\
                 Session capture: save_session_summary (batch-save multiple memories at end of conversation).\n\n\
                 Always start with search, then drill down as needed."
                    .to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_found_maps_to_resource_not_found() {
        let err = ShabkaError::NotFound("memory xyz".into());
        let data = to_mcp_error(err);
        assert_eq!(data.code.0, -32002); // RESOURCE_NOT_FOUND
        assert!(data.message.contains("Not found"));
        let error_type = data.data.unwrap()["error_type"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(error_type, "not_found");
    }

    #[test]
    fn test_invalid_input_maps_to_invalid_params() {
        let err = ShabkaError::InvalidInput("title cannot be empty".into());
        let data = to_mcp_error(err);
        assert_eq!(data.code.0, -32602); // INVALID_PARAMS
        assert!(data.message.contains("Invalid input"));
        let error_type = data.data.unwrap()["error_type"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(error_type, "invalid_input");
    }

    #[test]
    fn test_config_maps_to_invalid_params() {
        let err = ShabkaError::Config("bad config".into());
        let data = to_mcp_error(err);
        assert_eq!(data.code.0, -32602); // INVALID_PARAMS
        let error_type = data.data.unwrap()["error_type"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(error_type, "config_error");
    }

    #[test]
    fn test_storage_maps_to_internal_error() {
        let err = ShabkaError::Storage("db failed".into());
        let data = to_mcp_error(err);
        assert_eq!(data.code.0, -32603); // INTERNAL_ERROR
        let error_type = data.data.unwrap()["error_type"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(error_type, "storage_error");
    }

    #[test]
    fn test_embedding_maps_to_internal_error() {
        let err = ShabkaError::Embedding("embed failed".into());
        let data = to_mcp_error(err);
        assert_eq!(data.code.0, -32603); // INTERNAL_ERROR
        let error_type = data.data.unwrap()["error_type"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(error_type, "embedding_error");
    }
}
