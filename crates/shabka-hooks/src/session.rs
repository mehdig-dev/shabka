use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use shabka_core::llm::LlmService;
use shabka_core::model::MemoryKind;

/// A single event stored in the session buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferedEvent {
    pub timestamp: String,
    pub kind: MemoryKind,
    pub title: String,
    pub content: String,
    pub importance: f32,
    pub tags: Vec<String>,
    pub file_path: Option<String>,
    /// "tool_use", "tool_failure", or "intent"
    pub event_type: String,
}

/// Manages the JSONL session buffer file for a single session.
pub struct SessionBuffer {
    pub path: PathBuf,
}

impl SessionBuffer {
    /// Create a buffer for the given session ID.
    /// Files are stored at `~/.config/shabka/sessions/{session_id}.jsonl`.
    pub fn new(session_id: &str) -> Self {
        let dir = sessions_dir();
        Self {
            path: dir.join(format!("{session_id}.jsonl")),
        }
    }

    /// Append a single event to the buffer file (creates file + dir if needed).
    /// Skips the write if the last buffered event matches (title, content, event_type),
    /// which deduplicates the double-invocation from Claude Code hooks.
    pub fn append(&self, event: &BufferedEvent) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Dedup: check if last line matches the new event
        if self.path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&self.path) {
                if let Some(last_line) = contents.lines().rev().find(|l| !l.trim().is_empty()) {
                    if let Ok(last) = serde_json::from_str::<BufferedEvent>(last_line) {
                        if last.title == event.title
                            && last.content == event.content
                            && last.event_type == event.event_type
                        {
                            tracing::debug!("skipping duplicate buffer event: {}", event.title);
                            return Ok(());
                        }
                    }
                }
            }
        }

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(event)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// Read all events from the buffer.
    pub fn read_all(&self) -> anyhow::Result<Vec<BufferedEvent>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = std::fs::File::open(&self.path)?;
        let reader = std::io::BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<BufferedEvent>(&line) {
                Ok(event) => events.push(event),
                Err(e) => tracing::debug!("skipping malformed buffer line: {e}"),
            }
        }
        Ok(events)
    }

    /// Delete the buffer file.
    pub fn delete(&self) -> anyhow::Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
        }
        Ok(())
    }

    /// Check if the buffer has any events.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        !self.path.exists()
            || std::fs::metadata(&self.path)
                .map(|m| m.len() == 0)
                .unwrap_or(true)
    }
}

/// Directory where session buffers are stored.
fn sessions_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("shabka")
        .join("sessions")
}

/// A compressed memory ready to be saved.
pub struct CompressedMemory {
    pub kind: MemoryKind,
    pub title: String,
    pub content: String,
    pub importance: f32,
    pub tags: Vec<String>,
}

/// Compress buffered events into memories using heuristic grouping.
/// Used when LLM is disabled or as fallback on LLM failure.
pub fn compress_heuristic(events: &[BufferedEvent]) -> Vec<CompressedMemory> {
    if events.is_empty() {
        return Vec::new();
    }

    let mut memories = Vec::new();

    // Separate intents, errors, and file edits
    let intents: Vec<&BufferedEvent> = events.iter().filter(|e| e.event_type == "intent").collect();
    let errors: Vec<&BufferedEvent> = events
        .iter()
        .filter(|e| e.kind == MemoryKind::Error)
        .collect();
    let edits: Vec<&BufferedEvent> = events
        .iter()
        .filter(|e| e.event_type != "intent" && e.kind != MemoryKind::Error)
        .collect();

    // Group edits by file path
    let mut file_groups: std::collections::HashMap<String, Vec<&BufferedEvent>> =
        std::collections::HashMap::new();
    for edit in &edits {
        let key = edit.file_path.as_deref().unwrap_or("unknown").to_string();
        file_groups.entry(key).or_default().push(edit);
    }

    // Build a session summary from all file groups
    if !file_groups.is_empty() {
        let mut files_summary: Vec<String> = Vec::new();
        let mut max_importance: f32 = 0.4;
        let mut all_content = String::new();
        let mut file_tags: Vec<String> = Vec::new();

        let mut sorted_groups: Vec<_> = file_groups.into_iter().collect();
        sorted_groups.sort_by_key(|(path, _)| path.clone());

        for (path, group) in &sorted_groups {
            let filename = basename(path);
            let count = group.len();
            files_summary.push(format!(
                "{filename} ({count} edit{})",
                if count > 1 { "s" } else { "" }
            ));

            for event in group {
                max_importance = max_importance.max(event.importance);
            }

            // Extract filename-based tags (e.g. "auth.rs" -> "auth", "Cargo.toml" -> "dependencies")
            let stem = filename
                .rsplit_once('.')
                .map(|(s, _)| s)
                .unwrap_or(filename);
            if !stem.is_empty() {
                let tag = stem.to_lowercase();
                if tag == "cargo" {
                    file_tags.push("dependencies".into());
                } else if !file_tags.contains(&tag) {
                    file_tags.push(tag);
                }
            }

            // Include first and last edit content (truncated)
            if let Some(first) = group.first() {
                all_content.push_str(&format!("### {filename}\n"));
                all_content.push_str(&truncate(&first.content, 300));
                all_content.push('\n');
            }
            if group.len() > 1 {
                if let Some(last) = group.last() {
                    all_content.push_str(&format!("...\n{}\n", truncate(&last.content, 300)));
                }
            }
        }

        // Include intent context if available
        if !intents.is_empty() {
            all_content.push_str("\n### User Intent\n");
            for intent in &intents {
                all_content.push_str(&truncate(&intent.content, 200));
                all_content.push('\n');
            }
        }

        // Build title
        let title = if let Some(first_intent) = intents.first() {
            // Use the intent text directly as title (truncated to 80 chars)
            truncate(first_intent.content.trim(), 80)
        } else {
            // No intent — build from file list
            if files_summary.len() == 1 {
                format!("Update {}", files_summary[0])
            } else if files_summary.len() <= 3 {
                format!("Update {}", files_summary.join(", "))
            } else {
                format!(
                    "Update {} files: {}, and {} more",
                    sorted_groups.len(),
                    files_summary[..2].join(", "),
                    sorted_groups.len() - 2,
                )
            }
        };

        let mut tags = vec!["auto-capture".into(), "session-compressed".into()];
        tags.extend(file_tags);

        memories.push(CompressedMemory {
            kind: MemoryKind::Decision,
            title,
            content: all_content,
            importance: max_importance.min(0.7),
            tags,
        });
    }

    // Errors: keep as separate memories (merged if same type)
    if !errors.is_empty() {
        let mut error_content = String::new();
        let mut max_importance: f32 = 0.6;
        for err in &errors {
            error_content.push_str(&format!("- {}\n", truncate(&err.title, 100)));
            error_content.push_str(&format!("  {}\n", truncate(&err.content, 200)));
            max_importance = max_importance.max(err.importance);
        }
        let title = if errors.len() == 1 {
            errors[0].title.clone()
        } else {
            format!("{} errors encountered", errors.len())
        };
        memories.push(CompressedMemory {
            kind: MemoryKind::Error,
            title,
            content: error_content,
            importance: max_importance,
            tags: vec!["auto-capture".into(), "session-compressed".into()],
        });
    }

    memories
}

/// Compress buffered events into memories using an LLM.
pub async fn compress_with_llm(
    events: &[BufferedEvent],
    llm: &LlmService,
) -> anyhow::Result<Vec<CompressedMemory>> {
    // Build context for the LLM
    let mut context = String::new();

    // Collect intents
    let intents: Vec<&BufferedEvent> = events.iter().filter(|e| e.event_type == "intent").collect();
    if !intents.is_empty() {
        context.push_str("## User Requests\n");
        for intent in &intents {
            context.push_str(&format!("- {}\n", truncate(&intent.content, 300)));
        }
        context.push('\n');
    }

    // Collect file changes with actual code context
    let edits: Vec<&BufferedEvent> = events
        .iter()
        .filter(|e| e.event_type != "intent" && e.kind != MemoryKind::Error)
        .collect();
    if !edits.is_empty() {
        context.push_str("## File Changes\n");
        for edit in &edits {
            let path = edit.file_path.as_deref().unwrap_or("unknown");
            context.push_str(&format!(
                "### {}\n{}\n\n",
                basename(path),
                truncate(&edit.content, 400)
            ));
        }
    }

    // Collect errors
    let errors: Vec<&BufferedEvent> = events
        .iter()
        .filter(|e| e.kind == MemoryKind::Error)
        .collect();
    if !errors.is_empty() {
        context.push_str("## Errors\n");
        for err in &errors {
            context.push_str(&format!("- {}\n", truncate(&err.content, 200)));
        }
        context.push('\n');
    }

    let system = "\
You are a developer knowledge extractor. Given a coding session's events, \
extract 1-3 high-value memories that would help a developer in FUTURE sessions.\n\
\n\
For each memory, identify:\n\
- The KEY INSIGHT or LESSON learned (not just what files changed)\n\
- ENTITIES: specific tools, libraries, APIs, patterns, or concepts involved\n\
- WHY this matters for future reference\n\
\n\
Output a JSON array. Each object must have:\n\
- \"title\": descriptive, searchable (include the core concept, e.g. 'Async trait pattern for Axum middleware')\n\
- \"content\": 2-4 sentences explaining the insight, the approach taken, and any gotchas or key decisions. \
Include specific technical details (function names, config keys, error messages) that aid future retrieval.\n\
- \"kind\": one of: observation (noticed something), decision (chose an approach), pattern (reusable technique), \
error (problem encountered), fix (solution to a problem), lesson (learned something new)\n\
- \"importance\": 0.0-1.0 (0.8+ = would save significant time if recalled; 0.3 = minor/routine)\n\
- \"tags\": 3-8 lowercase tags for searchability — include: language, framework/library names, \
specific module/file names, concepts (e.g. 'async', 'auth', 'config'), action type ('refactor', 'bugfix', 'feature')\n\
\n\
Focus on REUSABLE KNOWLEDGE, not session narration. Skip routine changes with no insight.";

    let prompt = format!(
        "Extract reusable developer knowledge from this coding session:\n\n\
        {context}\n\n\
        Rules:\n\
        - Each memory should be independently useful (someone reading it cold should understand it)\n\
        - Titles should be searchable (include the core concept, not just 'Modified X')\n\
        - Content should explain WHY, not just WHAT\n\
        - Skip routine/trivial changes — only extract if there's genuine insight\n\
        Respond ONLY with a JSON array, no markdown fences."
    );

    let response = llm.generate(&prompt, Some(system)).await?;

    // Parse JSON response
    parse_llm_memories(&response)
}

/// Parse the LLM response into compressed memories.
fn parse_llm_memories(response: &str) -> anyhow::Result<Vec<CompressedMemory>> {
    // Try to extract JSON array from response (handle markdown fences)
    let json_str = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let items: Vec<serde_json::Value> = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("failed to parse LLM response as JSON array: {e}"))?;

    let mut memories = Vec::new();
    for item in items {
        let title = item["title"]
            .as_str()
            .unwrap_or("Session activity")
            .to_string();
        let content = item["content"].as_str().unwrap_or("").to_string();
        let kind_str = item["kind"].as_str().unwrap_or("observation");
        let kind = match kind_str {
            "decision" => MemoryKind::Decision,
            "pattern" => MemoryKind::Pattern,
            "error" => MemoryKind::Error,
            "fix" => MemoryKind::Fix,
            "lesson" => MemoryKind::Lesson,
            _ => MemoryKind::Observation,
        };
        let importance = item["importance"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0) as f32;

        // Merge system tags with LLM-generated tags
        let mut tags = vec![
            "auto-capture".to_string(),
            "session-compressed".to_string(),
            "llm-summarized".to_string(),
        ];
        if let Some(llm_tags) = item["tags"].as_array() {
            for tag in llm_tags {
                if let Some(t) = tag.as_str() {
                    let t = t.to_lowercase();
                    if !t.is_empty() && !tags.contains(&t) {
                        tags.push(t);
                    }
                }
            }
        }

        memories.push(CompressedMemory {
            kind,
            title,
            content,
            importance,
            tags,
        });
    }

    if memories.is_empty() {
        anyhow::bail!("LLM returned empty memories array");
    }

    Ok(memories)
}

/// Find stale session buffers (older than `max_age`) and return their paths.
pub fn find_stale_buffers(max_age: std::time::Duration) -> Vec<PathBuf> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let now = std::time::SystemTime::now();
    let mut stale = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age > max_age {
                            stale.push(path);
                        }
                    }
                }
            }
        }
    }

    stale
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn temp_buffer(name: &str) -> SessionBuffer {
        let dir = std::env::temp_dir()
            .join("shabka-test-sessions")
            .join(format!("{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        SessionBuffer {
            path: dir.join("test-session.jsonl"),
        }
    }

    fn make_edit_event(file_path: &str, title: &str) -> BufferedEvent {
        BufferedEvent {
            timestamp: Utc::now().to_rfc3339(),
            kind: MemoryKind::Decision,
            title: title.into(),
            content: format!("File modified via Edit: {file_path}"),
            importance: 0.4,
            tags: vec!["auto-capture".into()],
            file_path: Some(file_path.into()),
            event_type: "tool_use".into(),
        }
    }

    fn make_error_event(title: &str) -> BufferedEvent {
        BufferedEvent {
            timestamp: Utc::now().to_rfc3339(),
            kind: MemoryKind::Error,
            title: title.into(),
            content: format!("Error: {title}"),
            importance: 0.6,
            tags: vec!["auto-capture".into()],
            file_path: None,
            event_type: "tool_use".into(),
        }
    }

    fn make_intent_event(prompt: &str) -> BufferedEvent {
        BufferedEvent {
            timestamp: Utc::now().to_rfc3339(),
            kind: MemoryKind::Observation,
            title: "User intent".into(),
            content: prompt.into(),
            importance: 0.3,
            tags: Vec::new(),
            file_path: None,
            event_type: "intent".into(),
        }
    }

    #[test]
    fn test_buffer_append_and_read() {
        let buf = temp_buffer("append-read");
        let event1 = make_edit_event("/src/main.rs", "Edit main.rs: add fn main");
        let event2 = make_edit_event("/src/main.rs", "Edit main.rs: add error handling");

        buf.append(&event1).unwrap();
        buf.append(&event2).unwrap();

        let events = buf.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].title, "Edit main.rs: add fn main");
        assert_eq!(events[1].title, "Edit main.rs: add error handling");

        buf.delete().unwrap();
    }

    #[test]
    fn test_buffer_dedup_skips_duplicate() {
        let buf = temp_buffer("dedup");
        let event = make_edit_event("/src/main.rs", "Edit main.rs: add fn main");

        buf.append(&event).unwrap();
        buf.append(&event).unwrap(); // duplicate — should be skipped

        let events = buf.read_all().unwrap();
        assert_eq!(events.len(), 1, "duplicate event should have been skipped");
        assert_eq!(events[0].title, "Edit main.rs: add fn main");

        buf.delete().unwrap();
    }

    #[test]
    fn test_buffer_empty() {
        let buf = temp_buffer("empty");
        assert!(buf.is_empty());
        let events = buf.read_all().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_buffer_delete() {
        let buf = temp_buffer("delete");
        let event = make_edit_event("/src/lib.rs", "Edit lib.rs");
        buf.append(&event).unwrap();
        assert!(!buf.is_empty());

        buf.delete().unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_compress_heuristic_single_file() {
        let events = vec![
            make_edit_event("/src/main.rs", "Edit main.rs: add fn main"),
            make_edit_event("/src/main.rs", "Edit main.rs: add error handling"),
        ];
        let memories = compress_heuristic(&events);
        assert_eq!(memories.len(), 1);
        assert!(
            memories[0].title.starts_with("Update "),
            "title should start with 'Update': {}",
            memories[0].title
        );
        assert!(memories[0].title.contains("main.rs"));
        assert!(memories[0].title.contains("2 edits"));
        assert!(memories[0].tags.contains(&"session-compressed".to_string()));
    }

    #[test]
    fn test_compress_heuristic_multiple_files() {
        let events = vec![
            make_edit_event("/src/main.rs", "Edit main.rs"),
            make_edit_event("/src/lib.rs", "Edit lib.rs"),
            make_edit_event("/src/config.rs", "Edit config.rs"),
            make_edit_event("/src/utils.rs", "Edit utils.rs"),
        ];
        let memories = compress_heuristic(&events);
        assert_eq!(memories.len(), 1);
        assert!(
            memories[0].title.starts_with("Update "),
            "title should start with 'Update': {}",
            memories[0].title
        );
        assert!(memories[0].title.contains("4 files"));
    }

    #[test]
    fn test_compress_heuristic_intent_in_title() {
        let events = vec![
            make_intent_event("Fix the login bug in auth.rs"),
            make_edit_event("/src/auth.rs", "Edit auth.rs: fix login"),
            make_edit_event("/src/config.rs", "Edit config.rs: update settings"),
        ];
        let memories = compress_heuristic(&events);
        assert_eq!(memories.len(), 1);
        // Title should be the user intent text directly
        assert!(
            memories[0].title.contains("Fix the login bug"),
            "title should contain intent: {}",
            memories[0].title
        );
        // Should NOT contain "modified" — intent text is used directly
        assert!(
            !memories[0].title.contains("modified"),
            "title should not contain 'modified': {}",
            memories[0].title
        );
    }

    #[test]
    fn test_compress_heuristic_file_tags() {
        let events = vec![
            make_edit_event("/src/auth.rs", "Edit auth.rs"),
            make_edit_event("/Cargo.toml", "Edit Cargo.toml"),
        ];
        let memories = compress_heuristic(&events);
        assert_eq!(memories.len(), 1);
        assert!(
            memories[0].tags.contains(&"auth".to_string()),
            "should have 'auth' tag from auth.rs: {:?}",
            memories[0].tags
        );
        assert!(
            memories[0].tags.contains(&"dependencies".to_string()),
            "should have 'dependencies' tag from Cargo.toml: {:?}",
            memories[0].tags
        );
    }

    #[test]
    fn test_compress_heuristic_with_errors() {
        let events = vec![
            make_edit_event("/src/main.rs", "Edit main.rs"),
            make_error_event("cargo build failed: E0308"),
        ];
        let memories = compress_heuristic(&events);
        assert_eq!(memories.len(), 2); // one for edits, one for errors
        assert!(memories.iter().any(|m| m.kind == MemoryKind::Error));
        assert!(memories.iter().any(|m| m.kind == MemoryKind::Decision));
    }

    #[test]
    fn test_compress_heuristic_with_intents() {
        let events = vec![
            make_intent_event("Fix the login bug in auth.rs"),
            make_edit_event("/src/auth.rs", "Edit auth.rs: fix login"),
        ];
        let memories = compress_heuristic(&events);
        assert_eq!(memories.len(), 1);
        // Intent appears in both title and content
        assert!(memories[0].title.contains("Fix the login bug"));
        assert!(memories[0].content.contains("User Intent"));
        assert!(memories[0].content.contains("Fix the login bug"));
    }

    #[test]
    fn test_compress_heuristic_empty() {
        let memories = compress_heuristic(&[]);
        assert!(memories.is_empty());
    }

    #[test]
    fn test_compress_heuristic_only_errors() {
        let events = vec![
            make_error_event("build failed"),
            make_error_event("test failed"),
        ];
        let memories = compress_heuristic(&events);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].kind, MemoryKind::Error);
        assert!(memories[0].title.contains("2 errors"));
    }

    #[test]
    fn test_parse_llm_memories_valid() {
        let response = r#"[
            {"title": "Added auth system", "content": "Implemented JWT-based auth", "kind": "decision", "importance": 0.7, "tags": ["auth", "jwt", "rust"]},
            {"title": "Fixed login bug", "content": "Resolved null pointer in auth flow", "kind": "fix", "importance": 0.6, "tags": ["auth", "bugfix"]}
        ]"#;
        let memories = parse_llm_memories(response).unwrap();
        assert_eq!(memories.len(), 2);
        assert_eq!(memories[0].title, "Added auth system");
        assert_eq!(memories[0].kind, MemoryKind::Decision);
        assert_eq!(memories[1].kind, MemoryKind::Fix);
        // System tags + LLM tags
        assert!(memories[0].tags.contains(&"auto-capture".to_string()));
        assert!(memories[0].tags.contains(&"llm-summarized".to_string()));
        assert!(memories[0].tags.contains(&"auth".to_string()));
        assert!(memories[0].tags.contains(&"jwt".to_string()));
        assert!(memories[0].tags.contains(&"rust".to_string()));
    }

    #[test]
    fn test_parse_llm_memories_without_tags() {
        let response = r#"[
            {"title": "Test", "content": "test content", "kind": "observation", "importance": 0.5}
        ]"#;
        let memories = parse_llm_memories(response).unwrap();
        assert_eq!(memories[0].tags.len(), 3); // only system tags
    }

    #[test]
    fn test_parse_llm_memories_deduplicates_tags() {
        let response = r#"[
            {"title": "Test", "content": "test", "kind": "observation", "importance": 0.5, "tags": ["auto-capture", "new-tag"]}
        ]"#;
        let memories = parse_llm_memories(response).unwrap();
        let auto_count = memories[0]
            .tags
            .iter()
            .filter(|t| *t == "auto-capture")
            .count();
        assert_eq!(auto_count, 1); // no duplicates
        assert!(memories[0].tags.contains(&"new-tag".to_string()));
    }

    #[test]
    fn test_parse_llm_memories_with_fences() {
        let response = "```json\n[{\"title\": \"Test\", \"content\": \"test\", \"kind\": \"observation\", \"importance\": 0.5}]\n```";
        let memories = parse_llm_memories(response).unwrap();
        assert_eq!(memories.len(), 1);
    }

    #[test]
    fn test_parse_llm_memories_invalid() {
        let response = "I'm not sure what to do here";
        let result = parse_llm_memories(response);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_llm_memories_empty_array() {
        let response = "[]";
        let result = parse_llm_memories(response);
        assert!(result.is_err());
    }
}
