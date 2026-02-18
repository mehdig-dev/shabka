//! Smart dedup — detect near-duplicate memories via embedding similarity.
//!
//! **Threshold-based** (default):
//! - `>= skip_threshold` (0.95): SKIP — don't save, return existing ID
//! - `>= update_threshold` (0.85): SUPERSEDE — save new, mark old as superseded
//! - `< update_threshold`: ADD normally
//!
//! **LLM-powered** (when `graph.dedup_llm = true` and LLM is available):
//! - Vector search for top 5 similar memories
//! - Send new + existing memories to LLM for ADD/UPDATE/SKIP decision
//! - Falls back to threshold-based on LLM failure

use uuid::Uuid;

use crate::config::GraphConfig;
use crate::llm::LlmService;
use crate::model::Memory;
use crate::storage::StorageBackend;

/// The result of a duplicate check.
#[derive(Debug, Clone)]
pub enum DedupDecision {
    /// No duplicate found — save normally.
    Add,
    /// Near-exact duplicate found — skip saving.
    Skip {
        existing_id: Uuid,
        existing_title: String,
        similarity: f32,
    },
    /// Similar memory found — save new and supersede old.
    Supersede {
        existing_id: Uuid,
        existing_title: String,
        similarity: f32,
    },
    /// LLM decided to merge new info into an existing memory.
    Update {
        existing_id: Uuid,
        existing_title: String,
        merged_content: String,
        merged_title: String,
        similarity: f32,
    },
    /// New memory contradicts an existing memory.
    Contradict {
        existing_id: Uuid,
        existing_title: String,
        similarity: f32,
        reason: String,
    },
}

/// System prompt for LLM-powered dedup decisions.
const DEDUP_SYSTEM_PROMPT: &str = r#"You are a technical memory dedup system for a developer knowledge base.
Given a NEW memory and EXISTING similar memories, decide what to do.

Operations:
- ADD: The new memory covers a different topic or aspect. Save as new entry.
- UPDATE: The new memory adds details to an existing memory about the same topic. Provide merged_title and merged_content combining ALL details from BOTH.
- SKIP: An existing memory already covers this adequately. Don't save.
- CONTRADICT: The new memory directly contradicts an existing fact (e.g. "use X" vs "avoid X", different values for the same config). Save new memory and mark the contradiction.

Guidelines:
- UPDATE when new memory adds specific details (error messages, config keys, function names, steps) to an existing memory about the same concept.
- SKIP when the information is essentially restated with no new details.
- ADD when the topic, technology, or problem described is genuinely different.
- CONTRADICT when the new memory directly contradicts an existing memory's factual claims. The new information is assumed to be more current/correct.
- When merging (UPDATE), preserve ALL technical details from both memories.
- Prefer UPDATE over SKIP when the new memory has ANY additional useful info.

Return ONLY valid JSON (no markdown fences, no extra text):
{"decision":"ADD","target_id":null,"merged_title":null,"merged_content":null,"reason":"brief explanation"}

For UPDATE, target_id must be the id of the existing memory to update:
{"decision":"UPDATE","target_id":"0","merged_title":"combined title","merged_content":"merged content with all details","reason":"brief explanation"}

For SKIP, target_id must be the id of the existing memory that already covers this:
{"decision":"SKIP","target_id":"0","merged_title":null,"merged_content":null,"reason":"brief explanation"}

For CONTRADICT, target_id must be the id of the existing memory that is contradicted:
{"decision":"CONTRADICT","target_id":"0","merged_title":null,"merged_content":null,"reason":"brief explanation of contradiction"}"#;

/// Check whether a new memory is a duplicate of an existing one.
///
/// When `llm` is `Some` and `config.dedup_llm` is true, uses LLM-powered
/// decision logic (mem0-style ADD/UPDATE/SKIP). Falls back to threshold-based
/// logic on LLM failure or when LLM is not available.
pub async fn check_duplicate(
    storage: &impl StorageBackend,
    embedding: &[f32],
    config: &GraphConfig,
    exclude_id: Option<Uuid>,
    llm: Option<&LlmService>,
    new_title: &str,
    new_content: &str,
) -> DedupDecision {
    if !config.dedup_enabled {
        return DedupDecision::Add;
    }

    let results = match storage.vector_search(embedding, 5).await {
        Ok(r) => r,
        Err(_) => return DedupDecision::Add,
    };

    // Filter out the memory being updated (if any)
    let candidates: Vec<&(Memory, f32)> = results
        .iter()
        .filter(|(m, _)| Some(m.id) != exclude_id)
        .collect();

    // Try LLM-powered dedup if enabled and available
    if config.dedup_llm {
        if let Some(llm_service) = llm {
            // Only call LLM if there are candidates above minimum threshold
            let llm_candidates: Vec<&(Memory, f32)> = candidates
                .iter()
                .filter(|(_, score)| *score >= 0.5)
                .copied()
                .collect();

            if !llm_candidates.is_empty() {
                match check_duplicate_with_llm(llm_service, new_title, new_content, &llm_candidates)
                    .await
                {
                    Ok(decision) => return decision,
                    Err(e) => {
                        tracing::warn!("LLM dedup failed, falling back to thresholds: {e}");
                    }
                }
            } else {
                // No candidates above 0.5 — definitely a new memory
                return DedupDecision::Add;
            }
        }
    }

    // Threshold-based fallback
    threshold_decision(&candidates, config)
}

/// Pure threshold-based dedup decision (the original logic).
fn threshold_decision(candidates: &[&(Memory, f32)], config: &GraphConfig) -> DedupDecision {
    // Only check the top (highest-scoring) candidate
    if let Some((candidate, score)) = candidates.first() {
        if *score >= config.dedup_skip_threshold {
            return DedupDecision::Skip {
                existing_id: candidate.id,
                existing_title: candidate.title.clone(),
                similarity: *score,
            };
        }

        if *score >= config.dedup_update_threshold {
            return DedupDecision::Supersede {
                existing_id: candidate.id,
                existing_title: candidate.title.clone(),
                similarity: *score,
            };
        }
    }

    DedupDecision::Add
}

/// Ask the LLM to decide ADD/UPDATE/SKIP given the new memory and candidates.
async fn check_duplicate_with_llm(
    llm: &LlmService,
    new_title: &str,
    new_content: &str,
    candidates: &[&(Memory, f32)],
) -> std::result::Result<DedupDecision, String> {
    let (prompt, id_mapping) = build_dedup_prompt(new_title, new_content, candidates);

    let response = llm
        .generate(&prompt, Some(DEDUP_SYSTEM_PROMPT))
        .await
        .map_err(|e| format!("LLM call failed: {e}"))?;

    parse_llm_response(&response, &id_mapping)
}

/// Build the user prompt with temp ID mapping (like mem0).
///
/// Returns `(prompt_text, mapping)` where mapping maps "0" -> (Uuid, title, similarity).
pub(crate) fn build_dedup_prompt(
    new_title: &str,
    new_content: &str,
    candidates: &[&(Memory, f32)],
) -> (String, Vec<(Uuid, String, f32)>) {
    let mut id_mapping = Vec::new();
    let mut existing_section = String::new();

    for (idx, (memory, score)) in candidates.iter().enumerate() {
        id_mapping.push((memory.id, memory.title.clone(), *score));
        existing_section.push_str(&format!(
            "  {{\"id\": \"{idx}\", \"title\": \"{}\", \"content\": \"{}\"}}\n",
            memory.title.replace('"', "\\\""),
            memory.content.replace('"', "\\\"").replace('\n', "\\n"),
        ));
    }

    let prompt = format!(
        "NEW MEMORY:\n  Title: {new_title}\n  Content: {new_content}\n\n\
         EXISTING MEMORIES:\n{existing_section}\n\
         Decide: ADD, UPDATE, SKIP, or CONTRADICT?"
    );

    (prompt, id_mapping)
}

/// Parse the LLM's JSON response and map temp IDs back to real UUIDs.
pub(crate) fn parse_llm_response(
    response: &str,
    id_mapping: &[(Uuid, String, f32)],
) -> std::result::Result<DedupDecision, String> {
    // Strip markdown fences if present
    let cleaned = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let json: serde_json::Value =
        serde_json::from_str(cleaned).map_err(|e| format!("invalid JSON from LLM: {e}"))?;

    let decision = json["decision"]
        .as_str()
        .ok_or("missing 'decision' field")?
        .to_uppercase();

    match decision.as_str() {
        "ADD" => Ok(DedupDecision::Add),
        "SKIP" => {
            let target_idx = resolve_target_id(&json, id_mapping)?;
            let (id, title, sim) = &id_mapping[target_idx];
            Ok(DedupDecision::Skip {
                existing_id: *id,
                existing_title: title.clone(),
                similarity: *sim,
            })
        }
        "UPDATE" => {
            let target_idx = resolve_target_id(&json, id_mapping)?;
            let (id, title, sim) = &id_mapping[target_idx];
            let merged_title = json["merged_title"].as_str().unwrap_or(title).to_string();
            let merged_content = json["merged_content"]
                .as_str()
                .ok_or("UPDATE decision missing 'merged_content'")?
                .to_string();
            Ok(DedupDecision::Update {
                existing_id: *id,
                existing_title: title.clone(),
                merged_content,
                merged_title,
                similarity: *sim,
            })
        }
        "CONTRADICT" => {
            let target_idx = resolve_target_id(&json, id_mapping)?;
            let (id, title, sim) = &id_mapping[target_idx];
            let reason = json["reason"]
                .as_str()
                .unwrap_or("contradicts existing memory")
                .to_string();
            Ok(DedupDecision::Contradict {
                existing_id: *id,
                existing_title: title.clone(),
                similarity: *sim,
                reason,
            })
        }
        other => Err(format!("unknown decision: '{other}'")),
    }
}

/// Resolve the target_id from the LLM response to an index in the id_mapping.
fn resolve_target_id(
    json: &serde_json::Value,
    id_mapping: &[(Uuid, String, f32)],
) -> std::result::Result<usize, String> {
    let target = json["target_id"]
        .as_str()
        .or_else(|| json["target_id"].as_u64().map(|_| ""))
        .ok_or("missing 'target_id' field")?;

    // Try parsing as integer string first, then as the raw u64
    let idx: usize = if target.is_empty() {
        json["target_id"]
            .as_u64()
            .ok_or("target_id is not a valid integer")? as usize
    } else {
        target
            .parse()
            .map_err(|_| format!("target_id '{target}' is not a valid integer"))?
    };

    if idx >= id_mapping.len() {
        return Err(format!(
            "target_id {idx} out of range (0..{})",
            id_mapping.len()
        ));
    }

    Ok(idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::model::*;
    use std::sync::Mutex;

    /// Mock storage backend for dedup tests. Returns configurable vector_search results.
    struct MockStorage {
        search_results: Mutex<Vec<(Memory, f32)>>,
    }

    impl MockStorage {
        fn new(results: Vec<(Memory, f32)>) -> Self {
            Self {
                search_results: Mutex::new(results),
            }
        }

        fn empty() -> Self {
            Self::new(Vec::new())
        }

        fn with_match(title: &str, score: f32) -> Self {
            let memory = Memory::new(
                title.to_string(),
                "test content".to_string(),
                MemoryKind::Observation,
                "test".to_string(),
            );
            Self::new(vec![(memory, score)])
        }
    }

    impl crate::storage::StorageBackend for MockStorage {
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
        async fn add_relation(&self, _: &MemoryRelation) -> Result<()> {
            Ok(())
        }
        async fn get_relations(&self, _: Uuid) -> Result<Vec<MemoryRelation>> {
            Ok(Vec::new())
        }
        async fn count_relations(&self, ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
            Ok(ids.iter().map(|id| (*id, 0)).collect())
        }
        async fn count_contradictions(&self, ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
            Ok(ids.iter().map(|id| (*id, 0)).collect())
        }
        async fn save_session(&self, _: &Session) -> Result<()> {
            Ok(())
        }
        async fn get_session(&self, _: Uuid) -> Result<Session> {
            Err(crate::error::ShabkaError::NotFound("mock".into()))
        }
    }

    #[test]
    fn test_dedup_decision_variants() {
        let add = DedupDecision::Add;
        assert!(matches!(add, DedupDecision::Add));

        let skip = DedupDecision::Skip {
            existing_id: Uuid::nil(),
            existing_title: "test".to_string(),
            similarity: 0.99,
        };
        assert!(matches!(skip, DedupDecision::Skip { .. }));

        let supersede = DedupDecision::Supersede {
            existing_id: Uuid::nil(),
            existing_title: "test".to_string(),
            similarity: 0.90,
        };
        assert!(matches!(supersede, DedupDecision::Supersede { .. }));

        let update = DedupDecision::Update {
            existing_id: Uuid::nil(),
            existing_title: "old".to_string(),
            merged_content: "merged".to_string(),
            merged_title: "merged title".to_string(),
            similarity: 0.80,
        };
        assert!(matches!(update, DedupDecision::Update { .. }));
    }

    #[test]
    fn test_graph_config_defaults_for_dedup() {
        let config = GraphConfig::default();
        assert!(config.dedup_enabled);
        assert!((config.dedup_skip_threshold - 0.95).abs() < f32::EPSILON);
        assert!((config.dedup_update_threshold - 0.85).abs() < f32::EPSILON);
        assert!(!config.dedup_llm);
    }

    #[tokio::test]
    async fn test_dedup_disabled_always_add() {
        let config = GraphConfig {
            dedup_enabled: false,
            ..Default::default()
        };
        let storage = MockStorage::with_match("exact dup", 0.99);

        let decision = check_duplicate(&storage, &[0.0; 128], &config, None, None, "t", "c").await;
        assert!(matches!(decision, DedupDecision::Add));
    }

    #[tokio::test]
    async fn test_dedup_no_results_returns_add() {
        let config = GraphConfig::default();
        let storage = MockStorage::empty();

        let decision = check_duplicate(&storage, &[0.0; 128], &config, None, None, "t", "c").await;
        assert!(matches!(decision, DedupDecision::Add));
    }

    #[tokio::test]
    async fn test_dedup_exact_match_returns_skip() {
        let config = GraphConfig::default();
        let storage = MockStorage::with_match("existing memory", 0.97);

        let decision = check_duplicate(&storage, &[0.0; 128], &config, None, None, "t", "c").await;
        match decision {
            DedupDecision::Skip {
                existing_title,
                similarity,
                ..
            } => {
                assert_eq!(existing_title, "existing memory");
                assert!(similarity >= 0.95);
            }
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_dedup_high_similarity_returns_supersede() {
        let config = GraphConfig::default();
        // Score between update_threshold (0.85) and skip_threshold (0.95)
        let storage = MockStorage::with_match("similar memory", 0.90);

        let decision = check_duplicate(&storage, &[0.0; 128], &config, None, None, "t", "c").await;
        match decision {
            DedupDecision::Supersede {
                existing_title,
                similarity,
                ..
            } => {
                assert_eq!(existing_title, "similar memory");
                assert!(similarity >= 0.85);
                assert!(similarity < 0.95);
            }
            other => panic!("expected Supersede, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_dedup_low_similarity_returns_add() {
        let config = GraphConfig::default();
        // Score below update_threshold (0.85)
        let storage = MockStorage::with_match("unrelated memory", 0.50);

        let decision = check_duplicate(&storage, &[0.0; 128], &config, None, None, "t", "c").await;
        assert!(matches!(decision, DedupDecision::Add));
    }

    #[tokio::test]
    async fn test_dedup_at_skip_boundary() {
        let config = GraphConfig::default();
        // Exactly at skip threshold
        let storage = MockStorage::with_match("boundary", 0.95);

        let decision = check_duplicate(&storage, &[0.0; 128], &config, None, None, "t", "c").await;
        assert!(matches!(decision, DedupDecision::Skip { .. }));
    }

    #[tokio::test]
    async fn test_dedup_at_update_boundary() {
        let config = GraphConfig::default();
        // Exactly at update threshold
        let storage = MockStorage::with_match("boundary", 0.85);

        let decision = check_duplicate(&storage, &[0.0; 128], &config, None, None, "t", "c").await;
        assert!(matches!(decision, DedupDecision::Supersede { .. }));
    }

    #[tokio::test]
    async fn test_dedup_excludes_self() {
        let config = GraphConfig::default();
        let memory = Memory::new(
            "self match".to_string(),
            "content".to_string(),
            MemoryKind::Fact,
            "test".to_string(),
        );
        let self_id = memory.id;
        let storage = MockStorage::new(vec![(memory, 0.99)]);

        // When exclude_id matches the result, it should be skipped → Add
        let decision = check_duplicate(
            &storage,
            &[0.0; 128],
            &config,
            Some(self_id),
            None,
            "t",
            "c",
        )
        .await;
        assert!(matches!(decision, DedupDecision::Add));
    }

    #[tokio::test]
    async fn test_dedup_custom_thresholds() {
        // Lower thresholds: skip at 0.80, update at 0.60
        let config = GraphConfig {
            dedup_skip_threshold: 0.80,
            dedup_update_threshold: 0.60,
            ..Default::default()
        };
        let storage = MockStorage::with_match("custom", 0.75);

        let decision = check_duplicate(&storage, &[0.0; 128], &config, None, None, "t", "c").await;
        assert!(
            matches!(decision, DedupDecision::Supersede { .. }),
            "0.75 should supersede with update_threshold=0.60"
        );
    }

    #[tokio::test]
    async fn test_dedup_picks_first_result_above_threshold() {
        let config = GraphConfig::default();
        let m1 = Memory::new(
            "highest match".to_string(),
            "c1".to_string(),
            MemoryKind::Fact,
            "test".to_string(),
        );
        let m2 = Memory::new(
            "second match".to_string(),
            "c2".to_string(),
            MemoryKind::Fact,
            "test".to_string(),
        );
        // Results are ordered by score descending (as from vector search)
        let storage = MockStorage::new(vec![(m1, 0.97), (m2, 0.91)]);

        let decision = check_duplicate(&storage, &[0.0; 128], &config, None, None, "t", "c").await;
        match decision {
            DedupDecision::Skip { existing_title, .. } => {
                assert_eq!(existing_title, "highest match");
            }
            other => panic!("expected Skip for highest match, got {other:?}"),
        }
    }

    // -- LLM dedup parsing tests --

    #[test]
    fn test_parse_llm_response_add() {
        let id = Uuid::nil();
        let mapping = vec![(id, "existing".to_string(), 0.80)];
        let response = r#"{"decision":"ADD","target_id":null,"merged_title":null,"merged_content":null,"reason":"different topic"}"#;
        let result = parse_llm_response(response, &mapping).unwrap();
        assert!(matches!(result, DedupDecision::Add));
    }

    #[test]
    fn test_parse_llm_response_skip() {
        let id = Uuid::nil();
        let mapping = vec![(id, "existing memory".to_string(), 0.85)];
        let response = r#"{"decision":"SKIP","target_id":"0","merged_title":null,"merged_content":null,"reason":"already covered"}"#;
        let result = parse_llm_response(response, &mapping).unwrap();
        match result {
            DedupDecision::Skip {
                existing_id,
                existing_title,
                ..
            } => {
                assert_eq!(existing_id, id);
                assert_eq!(existing_title, "existing memory");
            }
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_llm_response_update() {
        let id = Uuid::nil();
        let mapping = vec![(id, "old title".to_string(), 0.82)];
        let response = r#"{"decision":"UPDATE","target_id":"0","merged_title":"combined title","merged_content":"merged details from both","reason":"adds details"}"#;
        let result = parse_llm_response(response, &mapping).unwrap();
        match result {
            DedupDecision::Update {
                existing_id,
                merged_title,
                merged_content,
                ..
            } => {
                assert_eq!(existing_id, id);
                assert_eq!(merged_title, "combined title");
                assert_eq!(merged_content, "merged details from both");
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_llm_response_strips_markdown_fences() {
        let id = Uuid::nil();
        let mapping = vec![(id, "existing".to_string(), 0.80)];
        let response = "```json\n{\"decision\":\"ADD\",\"target_id\":null,\"merged_title\":null,\"merged_content\":null,\"reason\":\"new\"}\n```";
        let result = parse_llm_response(response, &mapping).unwrap();
        assert!(matches!(result, DedupDecision::Add));
    }

    #[test]
    fn test_parse_llm_response_invalid_json() {
        let mapping = vec![(Uuid::nil(), "x".to_string(), 0.8)];
        let result = parse_llm_response("not json", &mapping);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn test_parse_llm_response_target_id_out_of_range() {
        let mapping = vec![(Uuid::nil(), "x".to_string(), 0.8)];
        let response = r#"{"decision":"SKIP","target_id":"5","reason":"covered"}"#;
        let result = parse_llm_response(response, &mapping);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of range"));
    }

    #[test]
    fn test_parse_llm_response_numeric_target_id() {
        let id = Uuid::nil();
        let mapping = vec![(id, "existing".to_string(), 0.80)];
        // Some LLMs return target_id as number instead of string
        let response = r#"{"decision":"SKIP","target_id":0,"reason":"covered"}"#;
        let result = parse_llm_response(response, &mapping).unwrap();
        assert!(matches!(result, DedupDecision::Skip { .. }));
    }

    #[test]
    fn test_parse_llm_response_unknown_decision() {
        let mapping = vec![(Uuid::nil(), "x".to_string(), 0.8)];
        let response = r#"{"decision":"DELETE","target_id":"0","reason":"remove"}"#;
        let result = parse_llm_response(response, &mapping);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown decision"));
    }

    #[test]
    fn test_contradict_variant() {
        let contradict = DedupDecision::Contradict {
            existing_id: Uuid::nil(),
            existing_title: "old fact".to_string(),
            similarity: 0.82,
            reason: "directly contradicts".to_string(),
        };
        match contradict {
            DedupDecision::Contradict {
                existing_title,
                similarity,
                reason,
                ..
            } => {
                assert_eq!(existing_title, "old fact");
                assert!((similarity - 0.82).abs() < f32::EPSILON);
                assert_eq!(reason, "directly contradicts");
            }
            other => panic!("expected Contradict, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_llm_response_contradict() {
        let id = Uuid::nil();
        let mapping = vec![(id, "use library X".to_string(), 0.80)];
        let response = r#"{"decision":"CONTRADICT","target_id":"0","merged_title":null,"merged_content":null,"reason":"new memory says avoid library X"}"#;
        let result = parse_llm_response(response, &mapping).unwrap();
        match result {
            DedupDecision::Contradict {
                existing_id,
                existing_title,
                reason,
                ..
            } => {
                assert_eq!(existing_id, id);
                assert_eq!(existing_title, "use library X");
                assert_eq!(reason, "new memory says avoid library X");
            }
            other => panic!("expected Contradict, got {other:?}"),
        }
    }

    #[test]
    fn test_build_dedup_prompt_structure() {
        let m1 = Memory::new(
            "HelixDB v2 syntax".to_string(),
            "Uses DROP instead of DELETE".to_string(),
            MemoryKind::Fact,
            "test".to_string(),
        );
        let m2 = Memory::new(
            "HelixDB port".to_string(),
            "Runs on port 6969".to_string(),
            MemoryKind::Fact,
            "test".to_string(),
        );
        let candidates: Vec<(Memory, f32)> = vec![(m1.clone(), 0.85), (m2.clone(), 0.70)];
        let candidate_refs: Vec<&(Memory, f32)> = candidates.iter().collect();

        let (prompt, mapping) =
            build_dedup_prompt("HelixDB query changes", "HQL v2 uses DROP", &candidate_refs);

        assert!(prompt.contains("HelixDB query changes"));
        assert!(prompt.contains("HQL v2 uses DROP"));
        assert!(prompt.contains("\"id\": \"0\""));
        assert!(prompt.contains("\"id\": \"1\""));
        assert_eq!(mapping.len(), 2);
        assert_eq!(mapping[0].0, m1.id);
        assert_eq!(mapping[1].0, m2.id);
    }
}
