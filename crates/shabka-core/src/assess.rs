use chrono::Utc;
use uuid::Uuid;

use crate::model::Memory;

/// A quality issue found in a memory.
#[derive(Debug, Clone)]
pub enum QualityIssue {
    GenericTitle {
        title: String,
    },
    ShortContent {
        length: usize,
    },
    NoTags,
    LowImportance {
        importance: f32,
    },
    Stale {
        days_inactive: i64,
    },
    Orphaned,
    PossibleDuplicate {
        other_id: Uuid,
        other_title: String,
        similarity: f32,
    },
    LowTrust {
        trust_score: f32,
    },
}

impl QualityIssue {
    /// Penalty score for this issue type, used in overall quality scoring.
    pub fn penalty(&self) -> f32 {
        match self {
            QualityIssue::GenericTitle { .. } => 15.0,
            QualityIssue::ShortContent { .. } => 10.0,
            QualityIssue::NoTags => 10.0,
            QualityIssue::LowImportance { .. } => 5.0,
            QualityIssue::Stale { .. } => 5.0,
            QualityIssue::Orphaned => 5.0,
            QualityIssue::PossibleDuplicate { .. } => 15.0,
            QualityIssue::LowTrust { .. } => 10.0,
        }
    }

    /// Human-readable label for this issue type.
    pub fn label(&self) -> &'static str {
        match self {
            QualityIssue::GenericTitle { .. } => "generic title",
            QualityIssue::ShortContent { .. } => "short content",
            QualityIssue::NoTags => "no tags",
            QualityIssue::LowImportance { .. } => "low importance",
            QualityIssue::Stale { .. } => "stale",
            QualityIssue::Orphaned => "orphaned",
            QualityIssue::PossibleDuplicate { .. } => "possible duplicate",
            QualityIssue::LowTrust { .. } => "low trust",
        }
    }
}

/// Assessment result for a single memory.
#[derive(Debug, Clone)]
pub struct AssessmentResult {
    pub memory_id: Uuid,
    pub title: String,
    pub issues: Vec<QualityIssue>,
}

/// Configuration for the assessment engine.
pub struct AssessConfig {
    pub generic_prefixes: Vec<&'static str>,
    pub min_content_length: usize,
    pub stale_days: u64,
    pub min_importance: f32,
}

impl Default for AssessConfig {
    fn default() -> Self {
        Self {
            generic_prefixes: vec![
                "Modified ",
                "Edit ",
                "Write ",
                "Session activity",
                "Tool failure",
            ],
            min_content_length: 50,
            stale_days: 90,
            min_importance: 0.3,
        }
    }
}

/// Analyze a single memory for quality issues.
pub fn analyze_memory(
    memory: &Memory,
    config: &AssessConfig,
    relation_count: usize,
) -> Vec<QualityIssue> {
    let mut issues = Vec::new();

    // Generic title check
    if config
        .generic_prefixes
        .iter()
        .any(|p| memory.title.starts_with(p))
    {
        issues.push(QualityIssue::GenericTitle {
            title: memory.title.clone(),
        });
    }

    // Short content
    if memory.content.len() < config.min_content_length {
        issues.push(QualityIssue::ShortContent {
            length: memory.content.len(),
        });
    }

    // No tags
    if memory.tags.is_empty() {
        issues.push(QualityIssue::NoTags);
    }

    // Low importance
    if memory.importance < config.min_importance {
        issues.push(QualityIssue::LowImportance {
            importance: memory.importance,
        });
    }

    // Stale
    let days_inactive = (Utc::now() - memory.accessed_at).num_days();
    if days_inactive >= config.stale_days as i64 {
        issues.push(QualityIssue::Stale { days_inactive });
    }

    // Orphaned (no relations)
    if relation_count == 0 {
        issues.push(QualityIssue::Orphaned);
    }

    // Low trust — uses contradiction_count=0 because analyze_memory doesn't have
    // access to the storage layer. Verification status alone (40% weight) is sufficient
    // to flag Disputed/Outdated memories.
    let trust = crate::trust::trust_score(memory, 0);
    if trust < 0.5 {
        issues.push(QualityIssue::LowTrust { trust_score: trust });
    }

    issues
}

/// Check a newly-created memory for quality issues before saving.
///
/// Like `analyze_memory()` but skips Stale and Orphaned checks which are
/// always false-positive on brand-new memories (just created, no relations yet).
pub fn check_new_memory(memory: &Memory, config: &AssessConfig) -> Vec<QualityIssue> {
    let mut issues = Vec::new();

    // Generic title check
    if config
        .generic_prefixes
        .iter()
        .any(|p| memory.title.starts_with(p))
    {
        issues.push(QualityIssue::GenericTitle {
            title: memory.title.clone(),
        });
    }

    // Short content
    if memory.content.len() < config.min_content_length {
        issues.push(QualityIssue::ShortContent {
            length: memory.content.len(),
        });
    }

    // No tags
    if memory.tags.is_empty() {
        issues.push(QualityIssue::NoTags);
    }

    // Low importance
    if memory.importance < config.min_importance {
        issues.push(QualityIssue::LowImportance {
            importance: memory.importance,
        });
    }

    issues
}

/// Issue category counts for the scorecard.
#[derive(Debug, Default, serde::Serialize)]
pub struct IssueCounts {
    pub generic_titles: usize,
    pub short_content: usize,
    pub no_tags: usize,
    pub low_importance: usize,
    pub stale: usize,
    pub orphaned: usize,
    pub duplicates: usize,
    pub low_trust: usize,
}

impl IssueCounts {
    /// Build counts from a list of assessment results.
    pub fn from_results(results: &[AssessmentResult]) -> Self {
        let mut counts = Self::default();
        for result in results {
            for issue in &result.issues {
                match issue {
                    QualityIssue::GenericTitle { .. } => counts.generic_titles += 1,
                    QualityIssue::ShortContent { .. } => counts.short_content += 1,
                    QualityIssue::NoTags => counts.no_tags += 1,
                    QualityIssue::LowImportance { .. } => counts.low_importance += 1,
                    QualityIssue::Stale { .. } => counts.stale += 1,
                    QualityIssue::Orphaned => counts.orphaned += 1,
                    QualityIssue::PossibleDuplicate { .. } => counts.duplicates += 1,
                    QualityIssue::LowTrust { .. } => counts.low_trust += 1,
                }
            }
        }
        counts
    }
}

/// Compute an overall quality score (0–100) from assessment results.
///
/// Each issue deducts a weighted penalty from 100. The score is clamped to [0, 100].
pub fn quality_score(results: &[AssessmentResult], total_memories: usize) -> u32 {
    if total_memories == 0 {
        return 100;
    }

    let total_penalty: f32 = results
        .iter()
        .flat_map(|r| r.issues.iter())
        .map(|issue| issue.penalty())
        .sum();

    let avg_penalty = total_penalty / total_memories as f32;
    let score = (100.0 - avg_penalty).clamp(0.0, 100.0);
    score as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Memory, MemoryKind};

    fn make_memory(title: &str, content: &str, importance: f32, tags: Vec<String>) -> Memory {
        let mut m = Memory::new(
            title.to_string(),
            content.to_string(),
            MemoryKind::Observation,
            "test-user".to_string(),
        )
        .with_importance(importance)
        .with_tags(tags);
        // Ensure it's recently accessed so not stale
        m.accessed_at = Utc::now();
        m
    }

    #[test]
    fn test_generic_title_detected() {
        let m = make_memory(
            "Modified main.rs",
            "some content that is long enough to pass the check easily",
            0.5,
            vec!["rust".into()],
        );
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::GenericTitle { .. })));
    }

    #[test]
    fn test_edit_prefix_detected() {
        let m = make_memory(
            "Edit config.toml",
            "some content that is long enough to pass the check easily",
            0.5,
            vec!["config".into()],
        );
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::GenericTitle { .. })));
    }

    #[test]
    fn test_non_generic_title_passes() {
        let m = make_memory(
            "Implement retry logic for API calls",
            "some content that is long enough to pass the check easily",
            0.5,
            vec!["rust".into()],
        );
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(!issues
            .iter()
            .any(|i| matches!(i, QualityIssue::GenericTitle { .. })));
    }

    #[test]
    fn test_short_content_flagged() {
        let m = make_memory("Good title", "short", 0.5, vec!["tag".into()]);
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::ShortContent { length } if *length == 5)));
    }

    #[test]
    fn test_no_tags_flagged() {
        let m = make_memory(
            "Good title",
            "content that is long enough to pass the minimum length check",
            0.5,
            vec![],
        );
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(issues.iter().any(|i| matches!(i, QualityIssue::NoTags)));
    }

    #[test]
    fn test_low_importance_flagged() {
        let m = make_memory(
            "Good title",
            "content that is long enough to pass the minimum length check",
            0.1,
            vec!["tag".into()],
        );
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::LowImportance { importance } if *importance < 0.3)));
    }

    #[test]
    fn test_stale_memory_flagged() {
        let mut m = make_memory(
            "Good title",
            "content that is long enough to pass the minimum length check",
            0.5,
            vec!["tag".into()],
        );
        m.accessed_at = Utc::now() - chrono::Duration::days(100);
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::Stale { .. })));
    }

    #[test]
    fn test_orphaned_flagged() {
        let m = make_memory(
            "Good title",
            "content that is long enough to pass the minimum length check",
            0.5,
            vec!["tag".into()],
        );
        let issues = analyze_memory(&m, &AssessConfig::default(), 0);
        assert!(issues.iter().any(|i| matches!(i, QualityIssue::Orphaned)));
    }

    #[test]
    fn test_clean_memory_no_issues() {
        let m = make_memory("Implement retry logic for API calls", "content that is long enough to pass the minimum length check easily and with room to spare", 0.5, vec!["rust".into()]);
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(
            issues.is_empty(),
            "expected no issues but got: {:?}",
            issues
        );
    }

    #[test]
    fn test_check_new_memory_skips_stale_and_orphaned() {
        // A new memory with no relations and just created should NOT get Stale or Orphaned
        let m = make_memory(
            "Good title",
            "content that is long enough to pass the minimum length check easily",
            0.5,
            vec!["tag".into()],
        );
        let issues = check_new_memory(&m, &AssessConfig::default());
        assert!(!issues
            .iter()
            .any(|i| matches!(i, QualityIssue::Stale { .. })));
        assert!(!issues.iter().any(|i| matches!(i, QualityIssue::Orphaned)));
        assert!(
            issues.is_empty(),
            "clean new memory should have no issues: {:?}",
            issues
        );
    }

    #[test]
    fn test_check_new_memory_detects_generic_title() {
        let m = make_memory(
            "Modified main.rs",
            "content that is long enough to pass the check easily and more",
            0.5,
            vec!["rust".into()],
        );
        let issues = check_new_memory(&m, &AssessConfig::default());
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::GenericTitle { .. })));
    }

    #[test]
    fn test_check_new_memory_detects_multiple_issues() {
        let m = make_memory("Modified x", "short", 0.1, vec![]);
        let issues = check_new_memory(&m, &AssessConfig::default());
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::GenericTitle { .. })));
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::ShortContent { .. })));
        assert!(issues.iter().any(|i| matches!(i, QualityIssue::NoTags)));
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::LowImportance { .. })));
        assert_eq!(issues.len(), 4);
    }

    #[test]
    fn test_low_trust_flagged() {
        use crate::model::{MemorySource, VerificationStatus};
        // Outdated + AutoCapture + no tags + short content = trust ~0.42
        let m = make_memory("Good title", "short", 0.5, vec![])
            .with_verification(VerificationStatus::Outdated)
            .with_source(MemorySource::AutoCapture {
                hook: "test".to_string(),
            });
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(issues
            .iter()
            .any(|i| matches!(i, QualityIssue::LowTrust { .. })));
    }

    #[test]
    fn test_low_trust_not_flagged_for_verified() {
        let m = make_memory(
            "Good title",
            "content that is long enough to pass the minimum length check",
            0.5,
            vec!["tag".into()],
        )
        .with_verification(crate::model::VerificationStatus::Verified);
        let issues = analyze_memory(&m, &AssessConfig::default(), 1);
        assert!(!issues
            .iter()
            .any(|i| matches!(i, QualityIssue::LowTrust { .. })));
    }

    #[test]
    fn test_quality_score_perfect() {
        let score = quality_score(&[], 10);
        assert_eq!(score, 100);
    }

    #[test]
    fn test_quality_score_with_issues() {
        let results = vec![AssessmentResult {
            memory_id: Uuid::now_v7(),
            title: "test".into(),
            issues: vec![QualityIssue::NoTags, QualityIssue::Orphaned],
        }];
        // 1 memory with 15 penalty points → 100 - 15 = 85
        let score = quality_score(&results, 1);
        assert_eq!(score, 85);
    }

    #[test]
    fn test_quality_score_clamped_at_zero() {
        let results = vec![AssessmentResult {
            memory_id: Uuid::now_v7(),
            title: "test".into(),
            issues: vec![
                QualityIssue::GenericTitle { title: "t".into() },
                QualityIssue::ShortContent { length: 5 },
                QualityIssue::NoTags,
                QualityIssue::LowImportance { importance: 0.1 },
                QualityIssue::Stale { days_inactive: 100 },
                QualityIssue::Orphaned,
                QualityIssue::PossibleDuplicate {
                    other_id: Uuid::now_v7(),
                    other_title: "x".into(),
                    similarity: 0.9,
                },
            ],
        }];
        // Total: 15+10+10+5+5+5+15 = 65 penalty for 1 memory → 100-65 = 35
        let score = quality_score(&results, 1);
        assert_eq!(score, 35);
    }

    #[test]
    fn test_quality_score_empty_store() {
        let score = quality_score(&[], 0);
        assert_eq!(score, 100);
    }

    #[test]
    fn test_issue_counts() {
        let results = vec![
            AssessmentResult {
                memory_id: Uuid::now_v7(),
                title: "a".into(),
                issues: vec![QualityIssue::NoTags, QualityIssue::Orphaned],
            },
            AssessmentResult {
                memory_id: Uuid::now_v7(),
                title: "b".into(),
                issues: vec![
                    QualityIssue::NoTags,
                    QualityIssue::GenericTitle {
                        title: "Modified x".into(),
                    },
                ],
            },
        ];
        let counts = IssueCounts::from_results(&results);
        assert_eq!(counts.no_tags, 2);
        assert_eq!(counts.orphaned, 1);
        assert_eq!(counts.generic_titles, 1);
    }
}
