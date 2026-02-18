use crate::model::{Memory, MemoryIndex};
use crate::trust::trust_score;
use chrono::{DateTime, Utc};

/// Weights for the fusion ranking formula.
#[derive(Debug, Clone)]
pub struct RankingWeights {
    pub similarity: f32,
    pub keyword: f32,
    pub recency: f32,
    pub importance: f32,
    pub access_freq: f32,
    pub graph_proximity: f32,
    pub trust: f32,
}

impl Default for RankingWeights {
    fn default() -> Self {
        Self {
            similarity: 0.25,
            keyword: 0.15,
            recency: 0.15,
            importance: 0.15,
            access_freq: 0.10,
            graph_proximity: 0.05,
            trust: 0.15,
        }
    }
}

/// Input to the ranking function: a memory with its raw scores.
pub struct RankCandidate {
    pub memory: Memory,
    pub vector_score: f32,
    pub keyword_score: f32,
    pub relation_count: usize,
    pub contradiction_count: usize,
}

/// Breakdown of how each component contributed to the final score.
#[derive(Debug, Clone)]
pub struct ScoreBreakdown {
    pub similarity: f32,
    pub keyword: f32,
    pub recency: f32,
    pub importance: f32,
    pub access_freq: f32,
    pub graph_proximity: f32,
    pub trust: f32,
}

/// Output of the ranking function.
pub struct RankedResult {
    pub memory: Memory,
    pub score: f32,
    pub breakdown: ScoreBreakdown,
}

/// Exponential decay score based on age. Half-life of 7 days.
/// Returns 1.0 for now, 0.5 at 7 days, 0.25 at 14 days, etc.
pub fn recency_score(created_at: DateTime<Utc>, now: DateTime<Utc>) -> f32 {
    let age_secs = (now - created_at).num_seconds().max(0) as f64;
    let half_life_secs = 7.0 * 24.0 * 3600.0; // 7 days
    let decay = (-age_secs * (2.0_f64.ln()) / half_life_secs).exp();
    decay as f32
}

/// Access frequency score: how recently the memory was accessed relative to its age.
/// Returns 1.0 if accessed_at == now, decays toward 0 as accessed_at gets older.
pub fn access_score(
    accessed_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> f32 {
    let age_secs = (now - created_at).num_seconds().max(1) as f64;
    let access_age_secs = (now - accessed_at).num_seconds().max(0) as f64;
    // Ratio: 1.0 means accessed very recently, 0.0 means never accessed since creation
    let ratio = 1.0 - (access_age_secs / age_secs).min(1.0);
    ratio as f32
}

/// Keyword match score: fraction of query terms found in the memory's title + content.
/// Case-insensitive. Returns 0.0 if no terms match, 1.0 if all match.
pub fn keyword_score(query: &str, memory: &Memory) -> f32 {
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return 0.0;
    }

    let haystack = format!(
        "{} {} {}",
        memory.title.to_lowercase(),
        memory.content.to_lowercase(),
        memory.tags.join(" ").to_lowercase(),
    );

    let matched = terms
        .iter()
        .filter(|t| haystack.contains(&t.to_lowercase()))
        .count();

    matched as f32 / terms.len() as f32
}

/// Normalize relation count to 0.0-1.0 range.
/// 0 relations = 0.0, 5+ relations = 1.0, linear interpolation between.
pub fn graph_score(relation_count: usize) -> f32 {
    (relation_count as f32 / 5.0).min(1.0)
}

/// Rank candidates using weighted fusion of multiple signals.
/// Returns results sorted by fused score, descending.
pub fn rank(candidates: Vec<RankCandidate>, weights: &RankingWeights) -> Vec<RankedResult> {
    let now = Utc::now();

    let mut results: Vec<RankedResult> = candidates
        .into_iter()
        .map(|c| {
            let sim = c.vector_score;
            let kw = c.keyword_score;
            let rec = recency_score(c.memory.created_at, now);
            let imp = c.memory.importance;
            let acc = access_score(c.memory.accessed_at, c.memory.created_at, now);
            let graph = graph_score(c.relation_count);
            let tru = trust_score(&c.memory, c.contradiction_count);

            let score = weights.similarity * sim
                + weights.keyword * kw
                + weights.recency * rec
                + weights.importance * imp
                + weights.access_freq * acc
                + weights.graph_proximity * graph
                + weights.trust * tru;

            RankedResult {
                memory: c.memory,
                score,
                breakdown: ScoreBreakdown {
                    similarity: sim,
                    keyword: kw,
                    recency: rec,
                    importance: imp,
                    access_freq: acc,
                    graph_proximity: graph,
                    trust: tru,
                },
            }
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}

/// Greedily pack ranked results into a token budget.
/// Results must already be sorted by score (descending).
/// Stops as soon as the next result would exceed the remaining budget.
pub fn budget_truncate(results: Vec<MemoryIndex>, token_budget: usize) -> Vec<MemoryIndex> {
    use crate::tokens::estimate_index_tokens;

    let mut remaining = token_budget;
    let mut packed = Vec::new();
    for result in results {
        let cost = estimate_index_tokens(&result);
        if cost > remaining {
            break;
        }
        remaining -= cost;
        packed.push(result);
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Memory, MemoryIndex, MemoryKind, VerificationStatus};
    use chrono::Duration;

    fn test_memory(title: &str, importance: f32, days_old: i64) -> Memory {
        let now = Utc::now();
        let created = now - Duration::days(days_old);
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
            status: crate::model::MemoryStatus::Active,
            privacy: crate::model::MemoryPrivacy::Private,
            verification: crate::model::VerificationStatus::default(),
            project_id: None,
            session_id: None,
            created_by: "test".to_string(),
            created_at: created,
            updated_at: created,
            accessed_at: created,
        }
    }

    #[test]
    fn test_recency_decay_curve() {
        let now = Utc::now();

        // Just created → ~1.0
        let score_now = recency_score(now, now);
        assert!((score_now - 1.0).abs() < 0.01);

        // 7 days → ~0.5
        let score_7d = recency_score(now - Duration::days(7), now);
        assert!((score_7d - 0.5).abs() < 0.01);

        // 14 days → ~0.25
        let score_14d = recency_score(now - Duration::days(14), now);
        assert!((score_14d - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_access_score_range() {
        let now = Utc::now();
        let created = now - Duration::days(10);

        // Accessed just now → 1.0
        let score = access_score(now, created, now);
        assert!((score - 1.0).abs() < 0.01);

        // Accessed at creation time → 0.0
        let score = access_score(created, created, now);
        assert!((score - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_graph_score_normalization() {
        assert_eq!(graph_score(0), 0.0);
        assert!((graph_score(1) - 0.2).abs() < 0.01);
        assert!((graph_score(5) - 1.0).abs() < 0.01);
        assert!((graph_score(10) - 1.0).abs() < 0.01); // clamped
    }

    #[test]
    fn test_rank_ordering() {
        let weights = RankingWeights::default();

        // High vector score + recent should beat low vector score + old
        let candidates = vec![
            RankCandidate {
                memory: test_memory("recent-high", 0.8, 0),
                vector_score: 0.95,
                keyword_score: 0.8,
                relation_count: 3,
                contradiction_count: 0,
            },
            RankCandidate {
                memory: test_memory("old-low", 0.3, 30),
                vector_score: 0.4,
                keyword_score: 0.2,
                relation_count: 0,
                contradiction_count: 0,
            },
        ];

        let results = rank(candidates, &weights);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].memory.title, "recent-high");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_rank_empty_input() {
        let weights = RankingWeights::default();
        let results = rank(vec![], &weights);
        assert!(results.is_empty());
    }

    #[test]
    fn test_keyword_score() {
        let mem = test_memory("Authentication flow with JWT tokens", 0.5, 1);
        // All terms match
        assert!((keyword_score("authentication JWT", &mem) - 1.0).abs() < 0.01);
        // Partial match (1 of 2 terms)
        assert!((keyword_score("authentication foobar", &mem) - 0.5).abs() < 0.01);
        // No match
        assert!((keyword_score("database migration", &mem) - 0.0).abs() < 0.01);
        // Empty query
        assert_eq!(keyword_score("", &mem), 0.0);
    }

    #[test]
    fn test_weights_sum_to_one() {
        let w = RankingWeights::default();
        let sum = w.similarity
            + w.keyword
            + w.recency
            + w.importance
            + w.access_freq
            + w.graph_proximity
            + w.trust;
        assert!((sum - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_budget_truncate_fits_all() {
        let results = vec![
            MemoryIndex {
                id: uuid::Uuid::now_v7(),
                title: "Short".to_string(),
                kind: MemoryKind::Fact,
                created_at: Utc::now(),
                score: 0.9,
                tags: vec![],
                verification: VerificationStatus::default(),
            },
            MemoryIndex {
                id: uuid::Uuid::now_v7(),
                title: "Also short".to_string(),
                kind: MemoryKind::Fact,
                created_at: Utc::now(),
                score: 0.8,
                tags: vec![],
                verification: VerificationStatus::default(),
            },
        ];
        let packed = budget_truncate(results, 10000);
        assert_eq!(packed.len(), 2);
    }

    #[test]
    fn test_budget_truncate_exceeds_budget() {
        let results = vec![
            MemoryIndex {
                id: uuid::Uuid::now_v7(),
                title: "a".repeat(100),
                kind: MemoryKind::Fact,
                created_at: Utc::now(),
                score: 0.9,
                tags: vec![],
                verification: VerificationStatus::default(),
            },
            MemoryIndex {
                id: uuid::Uuid::now_v7(),
                title: "b".repeat(100),
                kind: MemoryKind::Fact,
                created_at: Utc::now(),
                score: 0.8,
                tags: vec![],
                verification: VerificationStatus::default(),
            },
        ];
        // Each index: ~25 title tokens + 15 overhead = ~40 tokens
        // Budget of 45 should fit only the first one
        let packed = budget_truncate(results, 45);
        assert_eq!(packed.len(), 1);
        assert!(packed[0].title.starts_with('a'));
    }

    #[test]
    fn test_budget_truncate_zero_budget() {
        let results = vec![MemoryIndex {
            id: uuid::Uuid::now_v7(),
            title: "Something".to_string(),
            kind: MemoryKind::Fact,
            created_at: Utc::now(),
            score: 0.9,
            tags: vec![],
            verification: VerificationStatus::default(),
        }];
        let packed = budget_truncate(results, 0);
        assert!(packed.is_empty());
    }

    #[test]
    fn test_budget_truncate_empty_input() {
        let packed = budget_truncate(vec![], 1000);
        assert!(packed.is_empty());
    }

    #[test]
    fn test_trust_affects_ranking() {
        let weights = RankingWeights::default();

        let mut verified_mem = test_memory("verified", 0.5, 0);
        verified_mem.verification = VerificationStatus::Verified;

        let mut disputed_mem = test_memory("disputed", 0.5, 0);
        disputed_mem.verification = VerificationStatus::Disputed;

        let candidates = vec![
            RankCandidate {
                memory: disputed_mem,
                vector_score: 0.9,
                keyword_score: 0.8,
                relation_count: 2,
                contradiction_count: 2,
            },
            RankCandidate {
                memory: verified_mem,
                vector_score: 0.9,
                keyword_score: 0.8,
                relation_count: 2,
                contradiction_count: 0,
            },
        ];

        let results = rank(candidates, &weights);
        assert_eq!(results[0].memory.title, "verified");
        assert!(results[0].breakdown.trust > results[1].breakdown.trust);
    }
}
