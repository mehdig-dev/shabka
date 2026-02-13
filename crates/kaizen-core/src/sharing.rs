use crate::config::PrivacyConfig;
use crate::model::{Memory, MemoryPrivacy};

/// Legacy `created_by` values that should be treated as owned by the current user.
const LEGACY_CREATORS: &[&str] = &["default", "kaizen-hooks"];

/// Check if a memory is visible to the current user.
///
/// - Public: always visible
/// - Team: always visible (real team membership deferred to sync server)
/// - Private: only if created by current user or a legacy value
pub fn is_visible(privacy: MemoryPrivacy, created_by: &str, current_user: &str) -> bool {
    match privacy {
        MemoryPrivacy::Public | MemoryPrivacy::Team => true,
        MemoryPrivacy::Private => {
            created_by == current_user || LEGACY_CREATORS.contains(&created_by)
        }
    }
}

/// Filter a vec of memories in-place, retaining only those visible to the current user.
pub fn filter_memories(memories: &mut Vec<Memory>, current_user: &str) {
    memories.retain(|m| is_visible(m.privacy, &m.created_by, current_user));
}

/// Filter search results in-place, retaining only those visible to the current user.
pub fn filter_search_results(results: &mut Vec<(Memory, f32)>, current_user: &str) {
    results.retain(|(m, _)| is_visible(m.privacy, &m.created_by, current_user));
}

/// Parse the default privacy level from config, falling back to Private.
pub fn parse_default_privacy(config: &PrivacyConfig) -> MemoryPrivacy {
    config
        .default_level
        .parse()
        .unwrap_or(MemoryPrivacy::Private)
}

/// Openness ordering for export filtering.
///
/// Returns true if a memory's privacy is at least as open as the threshold.
/// Ordering: Public (most open) > Team > Private (least open).
pub fn should_export(memory_privacy: MemoryPrivacy, threshold: MemoryPrivacy) -> bool {
    openness(memory_privacy) <= openness(threshold)
}

fn openness(privacy: MemoryPrivacy) -> u8 {
    match privacy {
        MemoryPrivacy::Public => 0,
        MemoryPrivacy::Team => 1,
        MemoryPrivacy::Private => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_public_always_visible() {
        assert!(is_visible(MemoryPrivacy::Public, "other-user", "me"));
    }

    #[test]
    fn test_team_always_visible() {
        assert!(is_visible(MemoryPrivacy::Team, "other-user", "me"));
    }

    #[test]
    fn test_private_visible_to_owner() {
        assert!(is_visible(MemoryPrivacy::Private, "alice", "alice"));
    }

    #[test]
    fn test_private_hidden_from_others() {
        assert!(!is_visible(MemoryPrivacy::Private, "alice", "bob"));
    }

    #[test]
    fn test_private_visible_for_legacy_default() {
        assert!(is_visible(MemoryPrivacy::Private, "default", "anyone"));
    }

    #[test]
    fn test_private_visible_for_legacy_hooks() {
        assert!(is_visible(MemoryPrivacy::Private, "kaizen-hooks", "anyone"));
    }

    #[test]
    fn test_filter_memories() {
        use crate::model::MemoryKind;

        let mut memories = vec![
            Memory::new(
                "Public".into(),
                "c".into(),
                MemoryKind::Fact,
                "alice".into(),
            )
            .with_privacy(MemoryPrivacy::Public),
            Memory::new(
                "Private alice".into(),
                "c".into(),
                MemoryKind::Fact,
                "alice".into(),
            )
            .with_privacy(MemoryPrivacy::Private),
            Memory::new(
                "Private bob".into(),
                "c".into(),
                MemoryKind::Fact,
                "bob".into(),
            )
            .with_privacy(MemoryPrivacy::Private),
        ];

        filter_memories(&mut memories, "alice");
        assert_eq!(memories.len(), 2);
        assert_eq!(memories[0].title, "Public");
        assert_eq!(memories[1].title, "Private alice");
    }

    #[test]
    fn test_filter_search_results() {
        use crate::model::MemoryKind;

        let mut results: Vec<(Memory, f32)> = vec![
            (
                Memory::new("Visible".into(), "c".into(), MemoryKind::Fact, "me".into())
                    .with_privacy(MemoryPrivacy::Private),
                0.9,
            ),
            (
                Memory::new(
                    "Hidden".into(),
                    "c".into(),
                    MemoryKind::Fact,
                    "other".into(),
                )
                .with_privacy(MemoryPrivacy::Private),
                0.8,
            ),
        ];

        filter_search_results(&mut results, "me");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.title, "Visible");
    }

    #[test]
    fn test_should_export_public_threshold() {
        assert!(should_export(MemoryPrivacy::Public, MemoryPrivacy::Public));
        assert!(!should_export(MemoryPrivacy::Team, MemoryPrivacy::Public));
        assert!(!should_export(
            MemoryPrivacy::Private,
            MemoryPrivacy::Public
        ));
    }

    #[test]
    fn test_should_export_team_threshold() {
        assert!(should_export(MemoryPrivacy::Public, MemoryPrivacy::Team));
        assert!(should_export(MemoryPrivacy::Team, MemoryPrivacy::Team));
        assert!(!should_export(MemoryPrivacy::Private, MemoryPrivacy::Team));
    }

    #[test]
    fn test_should_export_private_threshold() {
        assert!(should_export(MemoryPrivacy::Public, MemoryPrivacy::Private));
        assert!(should_export(MemoryPrivacy::Team, MemoryPrivacy::Private));
        assert!(should_export(
            MemoryPrivacy::Private,
            MemoryPrivacy::Private
        ));
    }

    #[test]
    fn test_parse_default_privacy() {
        let config = PrivacyConfig {
            default_level: "team".to_string(),
            redaction_patterns: vec![],
        };
        assert_eq!(parse_default_privacy(&config), MemoryPrivacy::Team);
    }

    #[test]
    fn test_parse_default_privacy_invalid_fallback() {
        let config = PrivacyConfig {
            default_level: "invalid".to_string(),
            redaction_patterns: vec![],
        };
        assert_eq!(parse_default_privacy(&config), MemoryPrivacy::Private);
    }
}
