# Trust Scoring Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `VerificationStatus` enum and computed `trust_score` as a 7th ranking signal, with a `verify_memory` MCP tool and CLI subcommand.

**Architecture:** New `VerificationStatus` field on `Memory`, new `trust.rs` module in shabka-core that computes trust from verification + source + contradictions + content quality. Trust plugs into the existing fusion ranking as a 7th weighted signal. A new `count_contradictions` method on `StorageBackend` counts `Contradicts` edges efficiently.

**Tech Stack:** Rust, shabka-core (model, ranking, trust, storage), shabka-mcp (rmcp 0.14), shabka-cli (clap), shabka-web (Axum + Askama)

---

### Task 1: Add VerificationStatus to Model

**Files:**
- Modify: `crates/shabka-core/src/model/memory.rs`
- Modify: `crates/shabka-core/src/model/tests.rs`

**Step 1: Add the enum after `MemoryStatus` (after line 267)**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    #[default]
    Unverified,
    Verified,
    Disputed,
    Outdated,
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unverified => write!(f, "unverified"),
            Self::Verified => write!(f, "verified"),
            Self::Disputed => write!(f, "disputed"),
            Self::Outdated => write!(f, "outdated"),
        }
    }
}

impl std::str::FromStr for VerificationStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "unverified" => Ok(Self::Unverified),
            "verified" => Ok(Self::Verified),
            "disputed" => Ok(Self::Disputed),
            "outdated" => Ok(Self::Outdated),
            _ => Err(format!("unknown verification status: {s}")),
        }
    }
}
```

**Step 2: Add `verification` field to `Memory` struct (after `privacy` field, line 76)**

```rust
    #[serde(default)]
    pub verification: VerificationStatus,
```

**Step 3: Set default in `Memory::new()` (around line 108, after `privacy: MemoryPrivacy::Private,`)**

```rust
            verification: VerificationStatus::default(),
```

**Step 4: Add builder method (after `with_privacy`, around line 154)**

```rust
    pub fn with_verification(mut self, verification: VerificationStatus) -> Self {
        self.verification = verification;
        self
    }
```

**Step 5: Add `verification` to `UpdateMemoryInput` (after `privacy` field, line 357)**

```rust
    pub verification: Option<VerificationStatus>,
```

**Step 6: Add `verification` to `MemoryIndex` (after `tags` field, line 308)**

```rust
    #[serde(default)]
    pub verification: VerificationStatus,
```

Update `MemoryIndex::from` impl (line 312) to include:
```rust
            verification: memory.verification,
```

**Step 7: Add `verification` to `TimelineEntry` (after `status` field, line 424)**

```rust
    #[serde(default)]
    pub verification: VerificationStatus,
```

Update `TimelineEntry::from` impl to include:
```rust
            verification: memory.verification,
```

**Step 8: Write tests in `crates/shabka-core/src/model/tests.rs`**

```rust
#[test]
fn test_verification_status_roundtrip() {
    use std::str::FromStr;

    for (s, expected) in [
        ("unverified", VerificationStatus::Unverified),
        ("verified", VerificationStatus::Verified),
        ("disputed", VerificationStatus::Disputed),
        ("outdated", VerificationStatus::Outdated),
    ] {
        let parsed = VerificationStatus::from_str(s).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(parsed.to_string(), s);
    }

    assert!(VerificationStatus::from_str("invalid").is_err());
}

#[test]
fn test_memory_default_verification() {
    let memory = Memory::new(
        "Test".to_string(),
        "Content".to_string(),
        MemoryKind::Fact,
        "user".to_string(),
    );
    assert_eq!(memory.verification, VerificationStatus::Unverified);
}

#[test]
fn test_memory_serde_without_verification() {
    // Simulate old JSON without verification field — should default to Unverified
    let json = r#"{"id":"019c6311-cefc-7612-b9c4-f7f1eb0d734f","kind":"fact","title":"Test","content":"c","summary":"c","tags":[],"source":{"type":"manual"},"scope":{"type":"global"},"importance":0.5,"status":"active","privacy":"private","created_by":"u","created_at":"2025-01-01T00:00:00Z","updated_at":"2025-01-01T00:00:00Z","accessed_at":"2025-01-01T00:00:00Z"}"#;
    let memory: Memory = serde_json::from_str(json).unwrap();
    assert_eq!(memory.verification, VerificationStatus::Unverified);
}
```

**Step 9: Run tests**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All existing tests pass + 3 new tests pass

**Step 10: Commit**

```
feat(core): add VerificationStatus enum to Memory model
```

---

### Task 2: Add trust.rs Module

**Files:**
- Create: `crates/shabka-core/src/trust.rs`
- Modify: `crates/shabka-core/src/lib.rs` (add `pub mod trust;`)

**Step 1: Write tests first in `trust.rs`**

```rust
use crate::model::{Memory, MemoryKind, MemorySource, VerificationStatus};

/// Compute a trust score (0.0–1.0) for a memory.
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

    let score = 0.40 * verification_weight
        + 0.30 * source_weight
        + 0.20 * contradiction_weight
        + 0.10 * quality;

    score.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn base_memory() -> Memory {
        Memory::new(
            "Test memory title".to_string(),
            "This is decent content that is long enough to pass the quality check easily".to_string(),
            MemoryKind::Fact,
            "user".to_string(),
        )
        .with_tags(vec!["test".to_string()])
    }

    #[test]
    fn test_verified_manual_no_contradictions() {
        let m = base_memory().with_verification(VerificationStatus::Verified);
        let score = trust_score(&m, 0);
        // 0.40*1.0 + 0.30*0.9 + 0.20*1.0 + 0.10*1.0 = 0.40 + 0.27 + 0.20 + 0.10 = 0.97
        assert!((score - 0.97).abs() < 0.01);
    }

    #[test]
    fn test_unverified_auto_capture_no_contradictions() {
        let m = base_memory()
            .with_source(MemorySource::AutoCapture { hook: "test".to_string() });
        let score = trust_score(&m, 0);
        // 0.40*0.5 + 0.30*0.5 + 0.20*1.0 + 0.10*1.0 = 0.20 + 0.15 + 0.20 + 0.10 = 0.65
        assert!((score - 0.65).abs() < 0.01);
    }

    #[test]
    fn test_disputed_with_contradictions() {
        let m = base_memory().with_verification(VerificationStatus::Disputed);
        let score = trust_score(&m, 2);
        // 0.40*0.2 + 0.30*0.9 + 0.20*0.2 + 0.10*1.0 = 0.08 + 0.27 + 0.04 + 0.10 = 0.49
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
        // 0.40*0.1 + 0.30*0.9 + 0.20*0.5 + 0.10*0.3 = 0.04 + 0.27 + 0.10 + 0.03 = 0.44
        assert!((score - 0.44).abs() < 0.01);
    }

    #[test]
    fn test_score_always_in_range() {
        // Best case: verified, manual, no contradictions, good quality
        let best = base_memory().with_verification(VerificationStatus::Verified);
        assert!(trust_score(&best, 0) <= 1.0);
        assert!(trust_score(&best, 0) >= 0.0);

        // Worst case: outdated, auto-capture, many contradictions, no tags, short content
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
```

**Step 2: Add module to lib.rs**

Add `pub mod trust;` after `pub mod tokens;` in `crates/shabka-core/src/lib.rs`.

**Step 3: Run tests**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All tests pass including 6 new trust tests

**Step 4: Commit**

```
feat(core): add trust score computation module
```

---

### Task 3: Add count_contradictions to StorageBackend

**Files:**
- Modify: `crates/shabka-core/src/storage/backend.rs`
- Modify: `crates/shabka-core/src/storage/helix.rs`
- Modify: `crates/shabka-core/src/graph.rs` (mock impl)
- Modify: `crates/shabka-core/src/dedup.rs` (mock impl)

**Step 1: Add trait method to `StorageBackend` (after `count_relations`, line 64)**

```rust
    /// Count Contradicts relations for a batch of memory IDs.
    /// Returns (id, count) pairs — only counts edges of type Contradicts.
    fn count_contradictions(
        &self,
        memory_ids: &[Uuid],
    ) -> impl std::future::Future<Output = Result<Vec<(Uuid, usize)>>> + Send;
```

**Step 2: Implement in `HelixStorage`**

In `crates/shabka-core/src/storage/helix.rs`, add after the `count_relations` impl. The implementation calls `get_relations` per ID and filters for `Contradicts`. This is O(n) calls but acceptable since ranking only happens on search result sets (≤50 items typically).

```rust
    async fn count_contradictions(&self, memory_ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
        let mut counts = Vec::with_capacity(memory_ids.len());
        for &id in memory_ids {
            let relations = self.get_relations(id).await.unwrap_or_default();
            let contradiction_count = relations
                .iter()
                .filter(|r| r.relation_type == RelationType::Contradicts)
                .count();
            counts.push((id, contradiction_count));
        }
        Ok(counts)
    }
```

**Step 3: Add to mock impls in `graph.rs` and `dedup.rs`**

In `crates/shabka-core/src/graph.rs` MockStorage (around line 263), add:
```rust
        async fn count_contradictions(&self, ids: &[Uuid]) -> Result<Vec<(Uuid, usize)>> {
            Ok(ids.iter().map(|id| (*id, 0)).collect())
        }
```

In `crates/shabka-core/src/dedup.rs` MockStorage (around line 372), add the same.

**Step 4: Run tests**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All tests pass (compile check — no new behavioral tests needed for mock returning 0)

**Step 5: Commit**

```
feat(core): add count_contradictions to StorageBackend trait
```

---

### Task 4: Integrate Trust into Ranking

**Files:**
- Modify: `crates/shabka-core/src/ranking.rs`

**Step 1: Add `trust` to `RankingWeights`**

Update `RankingWeights` struct and `Default` impl:

```rust
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
```

**Step 2: Add `contradiction_count` to `RankCandidate`**

```rust
pub struct RankCandidate {
    pub memory: Memory,
    pub vector_score: f32,
    pub keyword_score: f32,
    pub relation_count: usize,
    pub contradiction_count: usize,
}
```

**Step 3: Add `trust` to `ScoreBreakdown`**

```rust
pub struct ScoreBreakdown {
    pub similarity: f32,
    pub keyword: f32,
    pub recency: f32,
    pub importance: f32,
    pub access_freq: f32,
    pub graph_proximity: f32,
    pub trust: f32,
}
```

**Step 4: Update `rank()` function to compute trust**

Add `use crate::trust::trust_score;` at the top.

In the `rank()` closure, after `let graph = graph_score(c.relation_count);`:

```rust
            let tru = trust_score(&c.memory, c.contradiction_count);
```

Update the score computation:
```rust
            let score = weights.similarity * sim
                + weights.keyword * kw
                + weights.recency * rec
                + weights.importance * imp
                + weights.access_freq * acc
                + weights.graph_proximity * graph
                + weights.trust * tru;
```

Update `ScoreBreakdown` construction to include `trust: tru`.

**Step 5: Fix existing tests**

- `test_weights_sum_to_one`: Will pass since new weights sum to 1.0.
- `test_rank_ordering`: Add `contradiction_count: 0` to both `RankCandidate` instances.
- The `test_memory` helper needs no change.
- `test_budget_truncate_*`: These use `MemoryIndex` not `RankCandidate`, but `MemoryIndex` now has a `verification` field. The test constructors need `verification: VerificationStatus::default()` added.

**Step 6: Add trust-specific ranking test**

```rust
#[test]
fn test_trust_affects_ranking() {
    let weights = RankingWeights::default();

    // Same vector/keyword/recency, but one is verified and one is disputed
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
```

**Step 7: Run tests**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All tests pass

**Step 8: Commit**

```
feat(core): add trust as 7th ranking signal
```

---

### Task 5: Wire Trust into MCP Server Search

**Files:**
- Modify: `crates/shabka-mcp/src/server.rs`

**Step 1: Add `count_contradictions` call alongside `count_relations`**

In the search handler (around line 392-399), after the `count_relations` call, add:

```rust
        let contradiction_counts = self
            .storage
            .count_contradictions(&memory_ids)
            .await
            .map_err(to_mcp_error)?;

        let contradiction_map: std::collections::HashMap<Uuid, usize> =
            contradiction_counts.into_iter().collect();
```

**Step 2: Add `contradiction_count` when constructing `RankCandidate`**

Around line 407, in the `RankCandidate` struct literal:
```rust
                RankCandidate {
                    memory,
                    vector_score,
                    keyword_score: kw_score,
                    relation_count,
                    contradiction_count: contradiction_map.get(&memory.id).copied().unwrap_or(0),
                }
```

Note: you'll need to capture `memory.id` before moving `memory` into the struct, or reference the id first. Use a let binding:
```rust
            .map(|(memory, vector_score)| {
                let id = memory.id;
                let relation_count = count_map.get(&id).copied().unwrap_or(0);
                let kw_score = ranking::keyword_score(&params.query, &memory);
                let contradiction_count = contradiction_map.get(&id).copied().unwrap_or(0);
                RankCandidate {
                    memory,
                    vector_score,
                    keyword_score: kw_score,
                    relation_count,
                    contradiction_count,
                }
```

**Step 3: Include `trust_score` and `verification` in search response output**

The MCP search response serializes `MemoryIndex`. Since we added `verification` to `MemoryIndex` in Task 1, it's already included. Trust score isn't stored on `MemoryIndex` — it's in `ScoreBreakdown`. For the MCP response, add `trust_score` as a formatted field alongside the existing score when building the text response.

**Step 4: Run clippy**

Run: `cargo clippy --workspace --no-default-features -- -D warnings`
Expected: Clean

**Step 5: Commit**

```
feat(mcp): wire trust scoring into search ranking
```

---

### Task 6: Add verify_memory MCP Tool

**Files:**
- Modify: `crates/shabka-mcp/src/server.rs`

**Step 1: Add `VerifyMemoryParams` struct**

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct VerifyMemoryParams {
    #[schemars(description = "Memory ID to verify")]
    pub id: String,

    #[schemars(
        description = "Verification status: verified, disputed, outdated, or unverified"
    )]
    pub status: String,
}
```

**Step 2: Add the tool handler**

```rust
    #[tool(
        name = "verify_memory",
        description = "Set verification status on a memory (verified, disputed, outdated, unverified). Verified memories rank higher in search results."
    )]
    async fn verify_memory(
        &self,
        Parameters(params): Parameters<VerifyMemoryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = Uuid::parse_str(&params.id).map_err(|e| {
            ErrorData::invalid_params(format!("invalid memory ID: {e}"), None)
        })?;

        let verification: VerificationStatus = params
            .status
            .parse()
            .map_err(|e: String| ErrorData::invalid_params(e, None))?;

        let input = UpdateMemoryInput {
            verification: Some(verification),
            ..Default::default()
        };

        let memory = self
            .storage
            .update_memory(id, &input)
            .await
            .map_err(to_mcp_error)?;

        self.history.log(MemoryEvent {
            memory_id: id,
            action: EventAction::Update,
            timestamp: chrono::Utc::now(),
            details: Some(format!("verification → {verification}")),
        });

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Memory '{}' marked as {verification}",
            memory.title
        ))]))
    }
```

**Step 3: Add `verify_memory` to the `tool_router!` macro call**

Find the `tool_router!` invocation and add `verify_memory` to the list.

**Step 4: Run clippy + tests**

Run: `cargo clippy --workspace --no-default-features -- -D warnings`
Expected: Clean

**Step 5: Commit**

```
feat(mcp): add verify_memory tool (13th MCP tool)
```

---

### Task 7: Wire Trust into CLI Search + Add Verify Subcommand

**Files:**
- Modify: `crates/shabka-cli/src/main.rs`

**Step 1: Add `contradiction_count` to CLI search's `RankCandidate` construction**

In `cmd_search` (around line 640-647), add `count_contradictions` call and wire it in, same pattern as Task 5.

Also add it in `cmd_context_pack` (around line 773-780), same pattern.

**Step 2: Add `Verify` variant to `Cli` enum**

```rust
    /// Set verification status on a memory (verified, disputed, outdated)
    Verify {
        /// Memory ID (full UUID or short 8-char prefix)
        id: String,
        /// Verification status: verified, disputed, outdated, unverified
        #[arg(long)]
        status: String,
    },
```

**Step 3: Add match arm in `run()`**

```rust
        Cli::Verify { id, status } => cmd_verify(&storage, &history, &id, &status).await?,
```

**Step 4: Implement `cmd_verify`**

```rust
async fn cmd_verify(
    storage: &HelixStorage,
    history: &HistoryLogger,
    id_str: &str,
    status_str: &str,
) -> Result<()> {
    let id = resolve_memory_id(storage, id_str).await?;
    let verification: VerificationStatus = status_str
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;

    let input = UpdateMemoryInput {
        verification: Some(verification),
        ..Default::default()
    };

    let memory = storage.update_memory(id, &input).await?;

    history.log(shabka_core::history::MemoryEvent {
        memory_id: id,
        action: shabka_core::history::EventAction::Update,
        timestamp: chrono::Utc::now(),
        details: Some(format!("verification → {verification}")),
    });

    println!(
        "{} Memory '{}' marked as {}",
        "✓".green(),
        memory.title.bold(),
        verification.to_string().cyan()
    );

    Ok(())
}
```

**Step 5: Update `cmd_get` to display verification status and trust score**

In the `cmd_get` output section, after printing importance, add:

```rust
    // Compute trust score
    let relations = storage.get_relations(memory.id).await.unwrap_or_default();
    let contradiction_count = relations
        .iter()
        .filter(|r| r.relation_type == RelationType::Contradicts)
        .count();
    let trust = shabka_core::trust::trust_score(&memory, contradiction_count);

    println!("  {} {}", "Verification:".dimmed(), memory.verification);
    println!("  {} {:.0}%", "Trust:".dimmed(), trust * 100.0);
```

**Step 6: Run clippy + tests**

Run: `cargo clippy --workspace --no-default-features -- -D warnings`
Expected: Clean

**Step 7: Commit**

```
feat(cli): add verify subcommand and trust display
```

---

### Task 8: Add LowTrust Quality Issue to assess.rs

**Files:**
- Modify: `crates/shabka-core/src/assess.rs`

**Step 1: Add `LowTrust` variant to `QualityIssue`**

```rust
    LowTrust {
        trust_score: f32,
    },
```

Add `penalty` and `label` for it:
```rust
            QualityIssue::LowTrust { .. } => 10.0,
```
```rust
            QualityIssue::LowTrust { .. } => "low trust",
```

**Step 2: Add trust check to `analyze_memory`**

After existing checks, add:
```rust
    let trust = crate::trust::trust_score(memory, 0); // contradiction_count not available here, use 0
    if trust < 0.3 {
        issues.push(QualityIssue::LowTrust { trust_score: trust });
    }
```

Note: `analyze_memory` doesn't have access to contradiction count. Using 0 as floor — the trust score will still be low for disputed/outdated memories since verification status has 40% weight.

**Step 3: Add test**

```rust
#[test]
fn test_low_trust_flagged() {
    let m = test_memory_with_tags(0.5, 30, &["tag"])
        .with_verification(VerificationStatus::Outdated);
    let issues = analyze_memory(&m, &AssessConfig::default(), 2);
    assert!(issues.iter().any(|i| matches!(i, QualityIssue::LowTrust { .. })));
}
```

**Step 4: Run tests**

Run: `cargo test -p shabka-core --no-default-features`
Expected: All pass

**Step 5: Commit**

```
feat(core): add LowTrust quality issue to assessment
```

---

### Task 9: Wire Trust into Web Dashboard

**Files:**
- Modify: `crates/shabka-web/src/routes/search.rs` (add contradiction_count to RankCandidate)
- Modify: `crates/shabka-web/src/routes/api.rs` (add contradiction_count to RankCandidate)
- Modify: `crates/shabka-web/src/routes/memories.rs` (show trust badge on detail page)
- Modify: web templates as needed for trust badge display

**Step 1: Wire `count_contradictions` into web search routes**

Same pattern as Tasks 5/7: call `count_contradictions`, build `contradiction_map`, add to `RankCandidate`. Do this in both `routes/search.rs` and `routes/api.rs`.

**Step 2: Add verify buttons to detail page template**

Add a button group under the memory metadata section:
- "Verify" (green) → POST to `/api/v1/memories/{id}` with `verification: "verified"`
- "Dispute" (yellow) → POST with `verification: "disputed"`
- "Mark Outdated" (red) → POST with `verification: "outdated"`

Use the same `showConfirm` modal pattern already used for delete.

**Step 3: Show trust badge on detail page**

Color-coded badge next to the title:
- Verified → green badge
- Unverified → gray badge
- Disputed → yellow badge
- Outdated → red badge

Plus trust score percentage.

**Step 4: Show verification indicator on list page**

Small icon/badge in each memory card showing verification status.

**Step 5: Run clippy**

Run: `cargo clippy --workspace --no-default-features -- -D warnings`
Expected: Clean

**Step 6: Commit**

```
feat(web): add trust badges and verify buttons to dashboard
```

---

### Task 10: Full Validation + Update HelixDB update_memory Query

**Files:**
- Modify: `helix/queries.hx` (if update_memory query needs `verification` field)

**Step 1: Check if HelixDB `update_memory` query handles unknown fields**

HelixDB stores arbitrary JSON fields on nodes. The `update_memory` HQL query likely sets fields individually. Check if `verification` needs to be added to the query's SET clause.

**Step 2: Run full validation**

Run: `just check` (clippy + all unit tests)
Expected: All 230+ tests pass, clippy clean

**Step 3: Run hooks tests**

Run: `cargo test -p shabka-hooks --no-default-features`
Expected: All 37 tests pass

**Step 4: Commit any final fixes**

```
chore: fix any remaining compilation or test issues
```

**Step 5: Push**

```
git push origin main
```
