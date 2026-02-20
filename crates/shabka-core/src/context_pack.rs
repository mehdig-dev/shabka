use crate::model::Memory;
use crate::tokens::estimate_memory_tokens;
use serde::Serialize;

/// A packed set of memories that fits within a token budget.
#[derive(Debug, Serialize)]
pub struct ContextPack {
    pub memories: Vec<Memory>,
    pub total_tokens: usize,
    pub budget: usize,
    pub project_id: Option<String>,
}

/// Build a context pack by greedily packing ranked memories into a token budget.
/// Memories must already be sorted by relevance (highest first).
pub fn build_context_pack(
    memories: Vec<Memory>,
    token_budget: usize,
    project_id: Option<String>,
) -> ContextPack {
    let mut remaining = token_budget;
    let mut packed = Vec::new();
    let mut total = 0;
    for memory in memories {
        let cost = estimate_memory_tokens(&memory);
        if cost > remaining {
            break;
        }
        remaining -= cost;
        total += cost;
        packed.push(memory);
    }
    ContextPack {
        memories: packed,
        total_tokens: total,
        budget: token_budget,
        project_id,
    }
}

/// Format a context pack as paste-ready markdown.
pub fn format_context_pack(pack: &ContextPack) -> String {
    let mut out = String::new();

    // Header
    let project_label = pack.project_id.as_deref().unwrap_or("all");
    out.push_str(&format!(
        "# Project Context: {} ({} memories, ~{} tokens)\n\n",
        project_label,
        pack.memories.len(),
        pack.total_tokens,
    ));

    // Each memory
    for (i, memory) in pack.memories.iter().enumerate() {
        if i > 0 {
            out.push_str("---\n\n");
        }

        // Title line
        out.push_str(&format!("## [{}] {}\n", memory.kind, memory.title));

        // Metadata line
        let date = memory.created_at.format("%Y-%m-%d");
        let tags_str = if memory.tags.is_empty() {
            String::new()
        } else {
            format!(" | tags: {}", memory.tags.join(", "))
        };
        out.push_str(&format!(
            "*{} | importance: {}{}*\n\n",
            date, memory.importance, tags_str,
        ));

        // Content
        out.push_str(&memory.content);
        out.push_str("\n\n");
    }

    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MemoryKind;

    fn test_memory(title: &str, content: &str) -> Memory {
        Memory::new(
            title.to_string(),
            content.to_string(),
            MemoryKind::Decision,
            "test".to_string(),
        )
        .with_tags(vec!["test".to_string()])
    }

    #[test]
    fn test_build_context_pack_fits_all() {
        let memories = vec![
            test_memory("First", "Short content"),
            test_memory("Second", "Also short"),
        ];
        let pack = build_context_pack(memories, 10000, Some("thesis".to_string()));
        assert_eq!(pack.memories.len(), 2);
        assert_eq!(pack.budget, 10000);
        assert!(pack.total_tokens > 0);
        assert!(pack.total_tokens <= 10000);
        assert_eq!(pack.project_id, Some("thesis".to_string()));
    }

    #[test]
    fn test_build_context_pack_exceeds_budget() {
        let memories = vec![
            test_memory("First", &"a".repeat(200)),
            test_memory("Second", &"b".repeat(200)),
            test_memory("Third", &"c".repeat(200)),
        ];
        // Each memory: ~50 content + ~5 title + ~2 tags + 20 overhead ≈ 77 tokens
        // Budget 100 should fit only 1
        let pack = build_context_pack(memories, 100, None);
        assert_eq!(pack.memories.len(), 1);
        assert_eq!(pack.memories[0].title, "First");
    }

    #[test]
    fn test_build_context_pack_zero_budget() {
        let memories = vec![test_memory("Title", "Content")];
        let pack = build_context_pack(memories, 0, None);
        assert!(pack.memories.is_empty());
        assert_eq!(pack.total_tokens, 0);
    }

    #[test]
    fn test_build_context_pack_single_oversized() {
        let memories = vec![test_memory("Big", &"x".repeat(10000))];
        // Memory is ~2500+ tokens, budget is 100
        let pack = build_context_pack(memories, 100, None);
        assert!(pack.memories.is_empty());
    }

    #[test]
    fn test_format_context_pack_output() {
        let memories = vec![test_memory("Auth flow", "Use JWT tokens for auth.")];
        let pack = build_context_pack(memories, 10000, Some("thesis".to_string()));
        let output = format_context_pack(&pack);

        assert!(output.contains("# Project Context: thesis"));
        assert!(output.contains("## [decision] Auth flow"));
        assert!(output.contains("importance: 0.5"));
        assert!(output.contains("tags: test"));
        assert!(output.contains("Use JWT tokens for auth."));
    }

    #[test]
    fn test_format_context_pack_no_project() {
        let memories = vec![test_memory("Title", "Content")];
        let pack = build_context_pack(memories, 10000, None);
        let output = format_context_pack(&pack);
        assert!(output.contains("Project Context: all"));
    }

    #[test]
    fn test_format_context_pack_multiple_memories() {
        let memories = vec![
            test_memory("First", "Content 1"),
            test_memory("Second", "Content 2"),
        ];
        let pack = build_context_pack(memories, 10000, None);
        let output = format_context_pack(&pack);

        assert!(output.contains("---"));
        assert!(output.contains("## [decision] First"));
        assert!(output.contains("## [decision] Second"));
    }

    #[test]
    fn test_format_context_pack_empty() {
        let pack = build_context_pack(vec![], 1000, Some("empty".to_string()));
        let output = format_context_pack(&pack);
        assert!(output.contains("0 memories"));
        assert!(!output.contains("---"));
    }

    #[test]
    fn test_build_context_pack_exact_budget_boundary() {
        let m1 = test_memory("First", "short");
        let cost1 = crate::tokens::estimate_memory_tokens(&m1);
        let m2 = test_memory("Second", "also short");
        let pack = build_context_pack(vec![m1, m2], cost1, None);
        assert_eq!(pack.memories.len(), 1);
        assert_eq!(pack.total_tokens, cost1);
        assert_eq!(pack.memories[0].title, "First");
    }

    #[test]
    fn test_build_context_pack_preserves_order() {
        let memories = vec![
            test_memory("A", "first"),
            test_memory("B", "second"),
            test_memory("C", "third"),
        ];
        let pack = build_context_pack(memories, 10000, None);
        assert_eq!(pack.memories.len(), 3);
        assert_eq!(pack.memories[0].title, "A");
        assert_eq!(pack.memories[1].title, "B");
        assert_eq!(pack.memories[2].title, "C");
    }

    #[test]
    fn test_format_context_pack_includes_kind_and_tags() {
        let m = Memory::new(
            "Error handling".to_string(),
            "Use Result everywhere".to_string(),
            MemoryKind::Pattern,
            "test".to_string(),
        )
        .with_tags(vec!["rust".to_string(), "error".to_string()]);
        let pack = build_context_pack(vec![m], 10000, None);
        let output = format_context_pack(&pack);
        assert!(output.contains("[pattern]"));
        assert!(output.contains("tags: rust, error"));
    }

    #[test]
    fn test_format_context_pack_no_tags() {
        let mut m = Memory::new(
            "No tags".to_string(),
            "Content".to_string(),
            MemoryKind::Observation,
            "test".to_string(),
        );
        m.tags = vec![];
        let pack = build_context_pack(vec![m], 10000, None);
        let output = format_context_pack(&pack);
        assert!(output.contains("[observation]"));
        assert!(!output.contains("tags:"));
    }
}
