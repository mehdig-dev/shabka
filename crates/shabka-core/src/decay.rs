//! Decay and pruning — automatic archival of stale memories.
//!
//! Memories that haven't been accessed in a configurable number of days
//! can be automatically archived. Optionally, their importance can also
//! be decayed based on how long since they were last accessed.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::model::{Memory, MemoryStatus};

/// Default number of days of inactivity before a memory is considered stale.
const DEFAULT_INACTIVE_DAYS: u64 = 90;

/// Default half-life for importance decay (in days).
/// After this many days without access, importance is halved.
const DEFAULT_IMPORTANCE_HALF_LIFE_DAYS: f64 = 30.0;

/// Configuration for the prune operation.
#[derive(Debug, Clone)]
pub struct PruneConfig {
    /// Days without access before archiving. Default: 90.
    pub inactive_days: u64,
    /// Whether to also decay importance of stale memories.
    pub decay_importance: bool,
    /// Half-life in days for importance decay. Default: 30.
    pub importance_half_life_days: f64,
}

impl Default for PruneConfig {
    fn default() -> Self {
        Self {
            inactive_days: DEFAULT_INACTIVE_DAYS,
            decay_importance: false,
            importance_half_life_days: DEFAULT_IMPORTANCE_HALF_LIFE_DAYS,
        }
    }
}

/// A recommended action for a stale memory.
#[derive(Debug, Clone)]
pub struct PruneAction {
    pub memory_id: Uuid,
    pub title: String,
    pub days_inactive: u64,
    pub should_archive: bool,
    pub current_importance: f32,
    pub decayed_importance: Option<f32>,
}

/// Summary of a completed prune operation.
#[derive(Debug, Clone, Default)]
pub struct PruneResult {
    pub archived: usize,
    pub importance_decayed: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// Analyze memories and return recommended prune actions.
///
/// Only considers `Active` memories. Already-archived or superseded memories are skipped.
pub fn analyze(memories: &[Memory], config: &PruneConfig, now: DateTime<Utc>) -> Vec<PruneAction> {
    memories
        .iter()
        .filter(|m| m.status == MemoryStatus::Active)
        .filter_map(|m| {
            let days_inactive = (now - m.accessed_at).num_days().max(0) as u64;
            if days_inactive < config.inactive_days {
                return None;
            }

            let decayed = if config.decay_importance {
                Some(decayed_importance(
                    m.importance,
                    days_inactive as f64,
                    config.importance_half_life_days,
                ))
            } else {
                None
            };

            Some(PruneAction {
                memory_id: m.id,
                title: m.title.clone(),
                days_inactive,
                should_archive: true,
                current_importance: m.importance,
                decayed_importance: decayed,
            })
        })
        .collect()
}

/// Calculate decayed importance using exponential decay.
///
/// `importance * 2^(-days_since_access / half_life_days)`
///
/// Returns the decayed value, clamped to \[0.0, 1.0\].
pub fn decayed_importance(importance: f32, days_since_access: f64, half_life_days: f64) -> f32 {
    let decay = (-days_since_access * (2.0_f64.ln()) / half_life_days).exp();
    (importance as f64 * decay).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MemoryKind;
    use chrono::Duration;

    fn test_memory_at(
        now: DateTime<Utc>,
        title: &str,
        importance: f32,
        days_old: i64,
        days_since_access: i64,
    ) -> Memory {
        let created = now - Duration::days(days_old);
        let accessed = now - Duration::days(days_since_access);
        Memory {
            id: uuid::Uuid::now_v7(),
            kind: MemoryKind::Fact,
            title: title.to_string(),
            content: "test content".to_string(),
            summary: "test".to_string(),
            tags: vec![],
            source: crate::model::MemorySource::Manual,
            scope: crate::model::MemoryScope::Global,
            importance,
            status: MemoryStatus::Active,
            privacy: crate::model::MemoryPrivacy::Private,
            verification: crate::model::VerificationStatus::default(),
            project_id: None,
            session_id: None,
            created_by: "test".to_string(),
            created_at: created,
            updated_at: created,
            accessed_at: accessed,
        }
    }

    #[test]
    fn test_decayed_importance_at_half_life() {
        // After exactly one half-life, importance should be halved
        let result = decayed_importance(1.0, 30.0, 30.0);
        assert!((result - 0.5).abs() < 0.01, "got {result}");
    }

    #[test]
    fn test_decayed_importance_at_zero_days() {
        // No time elapsed → no decay
        let result = decayed_importance(0.8, 0.0, 30.0);
        assert!((result - 0.8).abs() < 0.01, "got {result}");
    }

    #[test]
    fn test_decayed_importance_at_two_half_lives() {
        // After two half-lives, importance should be quartered
        let result = decayed_importance(1.0, 60.0, 30.0);
        assert!((result - 0.25).abs() < 0.01, "got {result}");
    }

    #[test]
    fn test_decayed_importance_clamped() {
        // Should never go below 0 or above 1
        let result = decayed_importance(1.0, 1000.0, 30.0);
        assert!(result >= 0.0 && result <= 1.0);
    }

    #[test]
    fn test_analyze_finds_stale_memories() {
        let now = Utc::now();
        let config = PruneConfig {
            inactive_days: 90,
            ..Default::default()
        };

        let memories = vec![
            test_memory_at(now, "recent", 0.8, 10, 1), // accessed 1 day ago
            test_memory_at(now, "stale", 0.5, 200, 100), // accessed 100 days ago
            test_memory_at(now, "borderline", 0.6, 91, 91), // 91 days, past threshold
        ];

        let actions = analyze(&memories, &config, now);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].title, "stale");
        assert_eq!(actions[1].title, "borderline");
    }

    #[test]
    fn test_analyze_skips_archived_memories() {
        let now = Utc::now();
        let config = PruneConfig::default();

        let mut archived = test_memory_at(now, "archived", 0.5, 200, 200);
        archived.status = MemoryStatus::Archived;

        let memories = vec![archived];
        let actions = analyze(&memories, &config, now);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_analyze_with_importance_decay() {
        let now = Utc::now();
        let config = PruneConfig {
            inactive_days: 90,
            decay_importance: true,
            importance_half_life_days: 30.0,
        };

        let memories = vec![test_memory_at(now, "stale", 0.8, 200, 120)];
        let actions = analyze(&memories, &config, now);

        assert_eq!(actions.len(), 1);
        let action = &actions[0];
        assert!(action.decayed_importance.is_some());
        let decayed = action.decayed_importance.unwrap();
        assert!(
            decayed < 0.8,
            "importance should have decayed from 0.8, got {decayed}"
        );
    }

    #[test]
    fn test_analyze_empty_input() {
        let config = PruneConfig::default();
        let actions = analyze(&[], &config, Utc::now());
        assert!(actions.is_empty());
    }

    #[test]
    fn test_prune_config_defaults() {
        let config = PruneConfig::default();
        assert_eq!(config.inactive_days, 90);
        assert!(!config.decay_importance);
        assert!((config.importance_half_life_days - 30.0).abs() < 0.01);
    }
}
