use crate::model::{Memory, MemorySource, VerificationStatus};

/// Compute a trust score (0.0--1.0) for a memory.
///
/// Factors:
/// - Verification status (40%): Verified=1.0, Unverified=0.5, Disputed=0.2, Outdated=0.1
/// - Source reliability (30%): Manual=0.9, Derived=0.7, Import=0.6, AutoCapture=0.5
/// - Contradiction penalty (20%): 0=1.0, 1=0.5, 2+=0.2
/// - Content quality (10%): has_tags + decent content length
pub fn trust_score(memory: &Memory, contradiction_count: usize) -> f32 {
    let verification_weight = match memory.verification {
        VerificationStatus::Verified => 1.0,
        VerificationStatus::Unverified => 0.5,
        VerificationStatus::Disputed => 0.2,
        VerificationStatus::Outdated => 0.1,
    };

    let source_weight = match &memory.source {
        MemorySource::Manual => 0.9,
        MemorySource::Derived { .. } => 0.7,
        MemorySource::Import => 0.6,
        MemorySource::AutoCapture { .. } => 0.5,
    };

    let contradiction_weight = match contradiction_count {
        0 => 1.0,
        1 => 0.5,
        _ => 0.2,
    };

    let has_tags = !memory.tags.is_empty();
    let has_decent_content = memory.content.len() >= 50;
    let quality = match (has_tags, has_decent_content) {
        (true, true) => 1.0,
        (true, false) | (false, true) => 0.6,
        (false, false) => 0.3,
    };

    let score: f32 = 0.40 * verification_weight
        + 0.30 * source_weight
        + 0.20 * contradiction_weight
        + 0.10 * quality;

    score.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MemoryKind;

    fn base_memory() -> Memory {
        Memory::new(
            "Test memory title".to_string(),
            "This is decent content that is long enough to pass the quality check easily"
                .to_string(),
            MemoryKind::Fact,
            "user".to_string(),
        )
        .with_tags(vec!["test".to_string()])
    }

    #[test]
    fn test_verified_manual_no_contradictions() {
        let m = base_memory().with_verification(VerificationStatus::Verified);
        let score = trust_score(&m, 0);
        // 0.40*1.0 + 0.30*0.9 + 0.20*1.0 + 0.10*1.0 = 0.97
        assert!((score - 0.97).abs() < 0.01);
    }

    #[test]
    fn test_unverified_auto_capture_no_contradictions() {
        let m = base_memory().with_source(MemorySource::AutoCapture {
            hook: "test".to_string(),
        });
        let score = trust_score(&m, 0);
        // 0.40*0.5 + 0.30*0.5 + 0.20*1.0 + 0.10*1.0 = 0.65
        assert!((score - 0.65).abs() < 0.01);
    }

    #[test]
    fn test_disputed_with_contradictions() {
        let m = base_memory().with_verification(VerificationStatus::Disputed);
        let score = trust_score(&m, 2);
        // 0.40*0.2 + 0.30*0.9 + 0.20*0.2 + 0.10*1.0 = 0.49
        assert!((score - 0.49).abs() < 0.01);
    }

    #[test]
    fn test_outdated_low_quality() {
        let m = Memory::new(
            "T".to_string(),
            "short".to_string(),
            MemoryKind::Fact,
            "user".to_string(),
        )
        .with_verification(VerificationStatus::Outdated);
        // No tags, short content
        let score = trust_score(&m, 1);
        // 0.40*0.1 + 0.30*0.9 + 0.20*0.5 + 0.10*0.3 = 0.44
        assert!((score - 0.44).abs() < 0.01);
    }

    #[test]
    fn test_score_always_in_range() {
        let best = base_memory().with_verification(VerificationStatus::Verified);
        assert!(trust_score(&best, 0) <= 1.0);
        assert!(trust_score(&best, 0) >= 0.0);

        let worst = Memory::new("T".into(), "s".into(), MemoryKind::Fact, "u".into())
            .with_source(MemorySource::AutoCapture { hook: "h".into() })
            .with_verification(VerificationStatus::Outdated);
        let score = trust_score(&worst, 5);
        assert!(score >= 0.0);
        assert!(score <= 1.0);
    }

    #[test]
    fn test_one_contradiction_vs_zero() {
        let m = base_memory();
        let score_0 = trust_score(&m, 0);
        let score_1 = trust_score(&m, 1);
        assert!(score_0 > score_1);
    }
}
