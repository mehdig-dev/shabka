use crate::model::{Memory, MemoryIndex};

/// Estimate token count using byte length / 4 heuristic.
/// Note: uses byte length, not character count — overestimates for non-ASCII text.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Estimate tokens for a full Memory (title + content + tags + metadata overhead).
/// Summary is excluded because it is derived from content (first 200 chars).
pub fn estimate_memory_tokens(memory: &Memory) -> usize {
    estimate_tokens(&memory.title)
        + estimate_tokens(&memory.content)
        + estimate_tokens(&memory.tags.join(", "))
        + 20
}

/// Estimate tokens for a compact MemoryIndex (title + tags + metadata overhead).
pub fn estimate_index_tokens(index: &MemoryIndex) -> usize {
    estimate_tokens(&index.title) + estimate_tokens(&index.tags.join(", ")) + 15
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MemoryKind;

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_short() {
        // "hello" = 5 chars → (5+3)/4 = 2
        assert_eq!(estimate_tokens("hello"), 2);
    }

    #[test]
    fn test_estimate_tokens_long() {
        // 400 chars → (400+3)/4 = 100 (integer division: 403/4 = 100)
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    #[test]
    fn test_estimate_tokens_exact_multiple() {
        // "abcdefgh" = 8 chars → (8+3)/4 = 11/4 = 2
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn test_estimate_memory_tokens() {
        // title "Test title" = 10 chars → (10+3)/4 = 3
        // content "Some content here" = 17 chars → (17+3)/4 = 5
        // tags ["rust","testing"] → "rust, testing" = 13 chars → (13+3)/4 = 4
        // overhead = 20
        // total = 3 + 5 + 4 + 20 = 32
        let memory = Memory::new(
            "Test title".to_string(),
            "Some content here".to_string(),
            MemoryKind::Observation,
            "test".to_string(),
        )
        .with_tags(vec!["rust".to_string(), "testing".to_string()]);

        assert_eq!(estimate_memory_tokens(&memory), 32);
    }

    #[test]
    fn test_estimate_index_tokens() {
        // title "Test title" = 10 chars → (10+3)/4 = 3
        // tags ["rust"] → "rust" = 4 chars → (4+3)/4 = 1
        // overhead = 15
        // total = 3 + 1 + 15 = 19
        let memory = Memory::new(
            "Test title".to_string(),
            "content".to_string(),
            MemoryKind::Observation,
            "test".to_string(),
        )
        .with_tags(vec!["rust".to_string()]);

        let index = MemoryIndex::from((&memory, 1.0));
        assert_eq!(estimate_index_tokens(&index), 19);
    }

    #[test]
    fn test_estimate_memory_tokens_no_tags() {
        let memory = Memory::new(
            "Some title".to_string(),
            "Some content".to_string(),
            MemoryKind::Observation,
            "test".to_string(),
        );
        // No tags: tags.join(", ") = "" → 0 tokens
        // But title + content + overhead > 20
        assert!(estimate_memory_tokens(&memory) > 20);
    }
}
