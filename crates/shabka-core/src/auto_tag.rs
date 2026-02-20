//! LLM-powered auto-tagging and importance scoring for memories.
//!
//! When enabled (`capture.auto_tag = true` and `llm.enabled = true`),
//! newly captured memories are sent to the LLM for tag and importance suggestions.

use serde::Deserialize;

use crate::llm::LlmService;
use crate::model::Memory;

/// Result of auto-tagging a memory.
#[derive(Debug, Clone)]
pub struct AutoTagResult {
    pub tags: Vec<String>,
    pub importance: f32,
}

/// Raw JSON response from the LLM for auto-tagging.
#[derive(Deserialize, Debug)]
struct AutoTagLlmResponse {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_importance")]
    importance: f64,
}

fn default_importance() -> f64 {
    0.5
}

/// System prompt for LLM auto-tagging.
const AUTO_TAG_SYSTEM_PROMPT: &str = r#"You are a developer knowledge-base tagger. Given a memory's title, content, and kind, suggest appropriate tags and an importance score.

Rules:
- Return 3-8 specific, lowercase tags (e.g. "rust", "helix-db", "config", "wsl2", "bug-fix")
- Do NOT use generic tags like "auto-capture", "memory", "note", "info"
- Tags should reflect the specific technology, concept, file, or pattern described
- Importance is 0.0-1.0 where:
  - 0.1-0.3: trivial observations, routine operations
  - 0.4-0.6: useful patterns, common errors, configuration details
  - 0.7-0.8: important decisions, critical bugs, architectural patterns
  - 0.9-1.0: critical facts, security issues, data-loss scenarios

Return ONLY valid JSON (no markdown fences, no extra text):
{"tags":["tag1","tag2","tag3"],"importance":0.5}"#;

/// Ask the LLM to suggest tags and importance for a memory.
pub async fn auto_tag(memory: &Memory, llm: &LlmService) -> Option<AutoTagResult> {
    let prompt = format!(
        "Title: {}\nKind: {}\nContent: {}",
        memory.title, memory.kind, memory.content,
    );

    let response: AutoTagLlmResponse = llm
        .generate_structured(&prompt, Some(AUTO_TAG_SYSTEM_PROMPT))
        .await
        .ok()?;

    let tags: Vec<String> = response
        .tags
        .into_iter()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    if tags.is_empty() {
        return None;
    }

    let importance = (response.importance as f32).clamp(0.0, 1.0);
    Some(AutoTagResult { tags, importance })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_response(raw: &str) -> Option<AutoTagResult> {
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let response: AutoTagLlmResponse = serde_json::from_str(cleaned).ok()?;
        let tags: Vec<String> = response
            .tags
            .into_iter()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if tags.is_empty() {
            return None;
        }
        let importance = (response.importance as f32).clamp(0.0, 1.0);
        Some(AutoTagResult { tags, importance })
    }

    #[test]
    fn test_parse_auto_tag_valid() {
        let response = r#"{"tags":["rust","helix-db","config"],"importance":0.7}"#;
        let result = parse_response(response).unwrap();
        assert_eq!(result.tags, vec!["rust", "helix-db", "config"]);
        assert!((result.importance - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_auto_tag_with_fences() {
        let response = "```json\n{\"tags\":[\"wsl2\",\"bug-fix\"],\"importance\":0.6}\n```";
        let result = parse_response(response).unwrap();
        assert_eq!(result.tags, vec!["wsl2", "bug-fix"]);
        assert!((result.importance - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_auto_tag_invalid_json() {
        assert!(parse_response("not valid json").is_none());
    }

    #[test]
    fn test_parse_auto_tag_empty_tags() {
        assert!(parse_response(r#"{"tags":[],"importance":0.5}"#).is_none());
    }

    #[test]
    fn test_parse_auto_tag_clamps_importance() {
        let result = parse_response(r#"{"tags":["test"],"importance":1.5}"#).unwrap();
        assert!((result.importance - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_auto_tag_missing_importance_defaults() {
        let result = parse_response(r#"{"tags":["test"]}"#).unwrap();
        assert!((result.importance - 0.5).abs() < f32::EPSILON);
    }
}
