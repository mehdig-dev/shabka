//! LLM-powered auto-tagging and importance scoring for memories.
//!
//! When enabled (`capture.auto_tag = true` and `llm.enabled = true`),
//! newly captured memories are sent to the LLM for tag and importance suggestions.

use crate::llm::LlmService;
use crate::model::Memory;

/// Result of auto-tagging a memory.
#[derive(Debug, Clone)]
pub struct AutoTagResult {
    pub tags: Vec<String>,
    pub importance: f32,
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

    let response = llm
        .generate(&prompt, Some(AUTO_TAG_SYSTEM_PROMPT))
        .await
        .ok()?;
    parse_auto_tag_response(&response)
}

/// Parse the LLM's JSON response into an AutoTagResult.
pub fn parse_auto_tag_response(response: &str) -> Option<AutoTagResult> {
    let cleaned = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let json: serde_json::Value = serde_json::from_str(cleaned).ok()?;

    let tags: Vec<String> = json["tags"]
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
        .filter(|t| !t.is_empty())
        .collect();

    if tags.is_empty() {
        return None;
    }

    let importance = json["importance"]
        .as_f64()
        .map(|v| (v as f32).clamp(0.0, 1.0))
        .unwrap_or(0.5);

    Some(AutoTagResult { tags, importance })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auto_tag_valid() {
        let response = r#"{"tags":["rust","helix-db","config"],"importance":0.7}"#;
        let result = parse_auto_tag_response(response).unwrap();
        assert_eq!(result.tags, vec!["rust", "helix-db", "config"]);
        assert!((result.importance - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_auto_tag_with_fences() {
        let response = "```json\n{\"tags\":[\"wsl2\",\"bug-fix\"],\"importance\":0.6}\n```";
        let result = parse_auto_tag_response(response).unwrap();
        assert_eq!(result.tags, vec!["wsl2", "bug-fix"]);
        assert!((result.importance - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_auto_tag_invalid_json() {
        let response = "not valid json";
        assert!(parse_auto_tag_response(response).is_none());
    }

    #[test]
    fn test_parse_auto_tag_empty_tags() {
        let response = r#"{"tags":[],"importance":0.5}"#;
        assert!(parse_auto_tag_response(response).is_none());
    }

    #[test]
    fn test_parse_auto_tag_clamps_importance() {
        let response = r#"{"tags":["test"],"importance":1.5}"#;
        let result = parse_auto_tag_response(response).unwrap();
        assert!((result.importance - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_auto_tag_missing_importance_defaults() {
        let response = r#"{"tags":["test"]}"#;
        let result = parse_auto_tag_response(response).unwrap();
        assert!((result.importance - 0.5).abs() < f32::EPSILON);
    }
}
