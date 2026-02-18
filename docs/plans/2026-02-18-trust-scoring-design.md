# Trust Scoring & Verification — Design

**Date:** 2026-02-18
**Inspired by:** "Intelligent AI Delegation" (Tomašev, Franklin, Osindero, 2026) — Trust & Reputation framework, Graduated Authority, Attestation Chains

## Problem

All memories are treated equally in ranking. A stale auto-captured snippet gets the same credibility as a manually verified architectural decision. There's no way to mark memories as confirmed, disputed, or outdated — and no signal to push verified knowledge higher in search results.

## Solution

Add a `VerificationStatus` enum and a computed `trust_score` (0.0–1.0) as a 7th ranking signal. Users can verify, dispute, or mark memories outdated via MCP, CLI, or web. Trust is computed from four factors: verification status, source reliability, contradiction count, and content quality.

## Design

### 1. VerificationStatus Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    #[default]
    Unverified,   // Default for all new memories
    Verified,     // User explicitly confirmed accuracy
    Disputed,     // Contradicted or user-flagged
    Outdated,     // No longer accurate
}
```

Added as `pub verification: VerificationStatus` on `Memory` with `#[serde(default)]` for backward compat.

### 2. Trust Score Computation

New module `shabka-core/src/trust.rs`:

```
trust_score(memory, contradiction_count) -> f32

= 0.40 * verification_weight   // Verified=1.0, Unverified=0.5, Disputed=0.2, Outdated=0.1
+ 0.30 * source_weight          // Manual=0.9, Derived=0.7, Import=0.6, AutoCapture=0.5
+ 0.20 * contradiction_weight   // 0 contradictions=1.0, 1=0.5, 2+=0.2
+ 0.10 * content_quality        // has_tags + decent_content length
```

Computed at read time, not stored.

### 3. Ranking Integration

Trust becomes the 7th weight in `RankingWeights`:

| Signal | Old Weight | New Weight |
|--------|-----------|------------|
| similarity | 0.30 | 0.25 |
| keyword | 0.15 | 0.15 |
| recency | 0.20 | 0.15 |
| importance | 0.15 | 0.15 |
| access_freq | 0.10 | 0.10 |
| graph_proximity | 0.10 | 0.05 |
| **trust** | — | **0.15** |

`RankCandidate` gains `contradiction_count: usize`. `ScoreBreakdown` gains `trust: f32`.

### 4. Integration Points

**shabka-core:** New `trust.rs`, `VerificationStatus` on model, updated ranking, `QualityIssue::LowTrust`

**shabka-mcp:** `save_memory`/`update_memory` accept `verification`, search results include `trust_score` + `verification`, new `verify_memory` tool (13th)

**shabka-hooks:** No change — auto-captured memories default to Unverified + AutoCapture source (lower trust)

**shabka-web:** Trust badge on detail page, verify/dispute/outdated buttons, trust indicator on list/search

**shabka-cli:** `shabka get`/`search` show trust, new `shabka verify <id> --status <status>` subcommand

### 5. Backward Compatibility

- `#[serde(default)]` — existing memories deserialize as `Unverified`
- Trust computed at read time — no migration
- `MemoryIndex` gains `verification` + `trust_score` populated at query time
- Old clients omitting `verification` get the default

### 6. Not Doing (YAGNI)

- No provenance graph nodes — source enum sufficient
- No query classifier — trust baked into ranking
- No configurable trust weights — hardcoded, tunable later
- No stored trust_score — computed on read
- No new HelixDB queries — contradiction count from existing `get_relations`
