# Rig Leverage + Edge Property Fix Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix HelixDB edge property retrieval, replace manual LLM JSON parsing with Rig structured extraction, and add 5 new LLM providers.

**Architecture:** Three independent work streams: (1) Patch HelixDB generated handler to call `.out_e()` / `.in_e()` and include edge properties in responses, (2) Add `extractor()` method to LlmService and convert auto_tag/consolidate/dedup to use typed response structs with `schemars::JsonSchema`, (3) Add match arms for deepseek/groq/xai/cohere providers.

**Tech Stack:** Rust, rig-core 0.31, schemars 1.x, HelixDB

---

### Task 1: Fix HelixDB `get_relations` handler

**Files:**
- Modify: `helix/.helix/dev/helix-container/src/queries.rs:702-759`

**Step 1: Add edge return type struct**

Add this struct right before the `get_relations` handler (after line 700):

```rust
#[derive(Serialize, Default)]
pub struct Get_relationsEdgesReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub from_node: &'a str,
    pub to_node: &'a str,
    pub relation_type: Option<&'a Value>,
    pub strength: Option<&'a Value>,
}
```

**Step 2: Patch the handler to add `.out_e()` traversal**

Replace the `get_relations` handler body (lines 703-759) with:

```rust
#[handler]
pub fn get_relations (input: HandlerInput) -> Result<Response, GraphError> {
let db = Arc::clone(&input.graph.storage);
let data = input.request.in_fmt.deserialize::<get_relationsInput>(&input.request.body)?;
let arena = Bump::new();
let txn = db.graph_env.read_txn().map_err(|e| GraphError::New(format!("Failed to start read transaction: {:?}", e)))?;
    let source = G::new(&db, &txn, &arena)
.n_from_index("Memory", "memory_id", &data.memory_id).collect_to_obj()?;
    let target = G::from_iter(&db, &txn, std::iter::once(source.clone()), &arena)
.out_node("RelatesTo").collect::<Result<Vec<_>, _>>()?;
    let edges = G::from_iter(&db, &txn, std::iter::once(source.clone()), &arena)
.out_e("RelatesTo").collect::<Result<Vec<_>, _>>()?;
let response = json!({
    "source": Get_relationsSourceReturnType {
        id: uuid_str(source.id(), &arena),
        label: source.label(),
        memory_id: source.get_property("memory_id"),
        kind: source.get_property("kind"),
        title: source.get_property("title"),
        content: source.get_property("content"),
        summary: source.get_property("summary"),
        tags: source.get_property("tags"),
        source: source.get_property("source"),
        scope: source.get_property("scope"),
        importance: source.get_property("importance"),
        status: source.get_property("status"),
        privacy: source.get_property("privacy"),
        project_id: source.get_property("project_id"),
        session_id: source.get_property("session_id"),
        created_by: source.get_property("created_by"),
        created_at: source.get_property("created_at"),
        updated_at: source.get_property("updated_at"),
        accessed_at: source.get_property("accessed_at"),
    },
    "target": target.iter().map(|target| Get_relationsTargetReturnType {
        id: uuid_str(target.id(), &arena),
        label: target.label(),
        memory_id: target.get_property("memory_id"),
        kind: target.get_property("kind"),
        title: target.get_property("title"),
        content: target.get_property("content"),
        summary: target.get_property("summary"),
        tags: target.get_property("tags"),
        source: target.get_property("source"),
        scope: target.get_property("scope"),
        importance: target.get_property("importance"),
        status: target.get_property("status"),
        privacy: target.get_property("privacy"),
        project_id: target.get_property("project_id"),
        session_id: target.get_property("session_id"),
        created_by: target.get_property("created_by"),
        created_at: target.get_property("created_at"),
        updated_at: target.get_property("updated_at"),
        accessed_at: target.get_property("accessed_at"),
    }).collect::<Vec<_>>(),
    "edges": edges.iter().map(|edge| Get_relationsEdgesReturnType {
        id: uuid_str(edge.id(), &arena),
        label: edge.label(),
        from_node: uuid_str(edge.from_node(), &arena),
        to_node: uuid_str(edge.to_node(), &arena),
        relation_type: edge.get_property("relation_type"),
        strength: edge.get_property("strength"),
    }).collect::<Vec<_>>()
});
txn.commit().map_err(|e| GraphError::New(format!("Failed to commit transaction: {:?}", e)))?;
Ok(input.request.out_fmt.create_response(&response))
}
```

**Step 3: Do the same for `get_incoming_relations`**

Add struct:
```rust
#[derive(Serialize, Default)]
pub struct Get_incoming_relationsEdgesReturnType<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub from_node: &'a str,
    pub to_node: &'a str,
    pub relation_type: Option<&'a Value>,
    pub strength: Option<&'a Value>,
}
```

In the handler (line 1085), add after the `source` (`.in_node`) line:
```rust
    let edges = G::from_iter(&db, &txn, std::iter::once(target.clone()), &arena)
.in_e("RelatesTo").collect::<Result<Vec<_>, _>>()?;
```

And include `"edges"` in the response JSON (same pattern as above).

**Step 4: Rebuild and deploy**

```bash
cd /home/mehdi/projects/kaizen/helix && helix push dev
```

**Step 5: Run integration tests**

```bash
cargo test -p shabka-core --no-default-features --test helix_roundtrip -- --ignored test_relations
cargo test -p shabka-core --no-default-features --test mcp_integration -- --ignored test_count_contradictions
```

Expected: both pass.

**Step 6: Commit**

```bash
git add helix/.helix/dev/helix-container/src/queries.rs
git commit -m "fix(helix): add edge property retrieval to get_relations handlers"
```

---

### Task 2: Add `schemars` derive support

**Files:**
- Modify: `crates/shabka-core/Cargo.toml`

**Step 1: Add schemars dependency**

`schemars` is already in the workspace (used by rmcp). Add it to shabka-core:

```toml
schemars = { workspace = true }
```

If not in workspace deps, add `schemars = "1"` to `[workspace.dependencies]` in root `Cargo.toml` first.

**Step 2: Verify it builds**

```bash
cargo check -p shabka-core --no-default-features
```

**Step 3: Commit**

```bash
git add Cargo.toml crates/shabka-core/Cargo.toml
git commit -m "build: add schemars to shabka-core for structured extraction"
```

---

### Task 3: Add `extractor()` to LlmService

**Files:**
- Modify: `crates/shabka-core/src/llm.rs`

**Step 1: Add the extractor method**

The key challenge: `LlmService` stores `Box<dyn RigCompletionAdapter>` — we've already erased the concrete Rig model type. Rig's `Extractor` needs a concrete `CompletionModel`. Instead of re-architecting, add a `generate_structured<T>()` method that:
1. Calls `generate()` to get raw text
2. Strips markdown fences
3. Deserializes into `T: DeserializeOwned`

This is simpler than exposing Rig's Extractor (which would require un-erasing the model type) and still eliminates duplicated parsing code.

Add after the `generate()` method:

```rust
/// Generate structured output from the LLM.
///
/// Calls `generate()` and deserializes the JSON response into `T`.
/// Strips markdown fences if present. Falls back gracefully on parse errors.
pub async fn generate_structured<T: serde::de::DeserializeOwned>(
    &self,
    prompt: &str,
    system: Option<&str>,
) -> Result<T> {
    let raw = self.generate(prompt, system).await?;
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(cleaned).map_err(|e| {
        ShabkaError::Llm(format!("failed to parse structured LLM response: {e}"))
    })
}
```

**Step 2: Add test**

```rust
#[test]
fn test_generate_structured_parse() {
    // Test the markdown-stripping + deserialization logic directly
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct TestResponse {
        name: String,
        value: f32,
    }

    // Simulate what generate_structured does internally
    let raw = "```json\n{\"name\":\"test\",\"value\":0.5}\n```";
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let result: TestResponse = serde_json::from_str(cleaned).unwrap();
    assert_eq!(result.name, "test");
    assert!((result.value - 0.5).abs() < f32::EPSILON);
}
```

**Step 3: Run tests**

```bash
cargo test -p shabka-core --no-default-features -- llm::tests
```

**Step 4: Commit**

```bash
git add crates/shabka-core/src/llm.rs
git commit -m "feat(llm): add generate_structured<T>() for typed LLM responses"
```

---

### Task 4: Convert `auto_tag.rs` to structured extraction

**Files:**
- Modify: `crates/shabka-core/src/auto_tag.rs`

**Step 1: Define response struct and replace parse function**

Replace the `parse_auto_tag_response` function and add a `serde::Deserialize` struct:

```rust
use serde::Deserialize;

/// Raw JSON response from the LLM for auto-tagging.
#[derive(Deserialize, Debug)]
struct AutoTagLlmResponse {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_importance")]
    importance: f64,
}

fn default_importance() -> f64 {
    0.5
}
```

Update `auto_tag()` to use `generate_structured`:

```rust
pub async fn auto_tag(memory: &Memory, llm: &LlmService) -> Option<AutoTagResult> {
    let prompt = format!(
        "Title: {}\nKind: {}\nContent: {}",
        memory.title, memory.kind, memory.content,
    );

    let response: AutoTagLlmResponse = llm
        .generate_structured(&prompt, Some(AUTO_TAG_SYSTEM_PROMPT))
        .await
        .ok()?;

    let tags: Vec<String> = response
        .tags
        .into_iter()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    if tags.is_empty() {
        return None;
    }

    let importance = (response.importance as f32).clamp(0.0, 1.0);
    Some(AutoTagResult { tags, importance })
}
```

Delete the old `parse_auto_tag_response` function entirely.

**Step 2: Update tests**

The existing tests called `parse_auto_tag_response()` directly. Since that's removed, convert them to test the struct deserialization directly:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn parse_response(raw: &str) -> Option<AutoTagResult> {
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let response: AutoTagLlmResponse = serde_json::from_str(cleaned).ok()?;
        let tags: Vec<String> = response
            .tags
            .into_iter()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if tags.is_empty() {
            return None;
        }
        let importance = (response.importance as f32).clamp(0.0, 1.0);
        Some(AutoTagResult { tags, importance })
    }

    #[test]
    fn test_parse_auto_tag_valid() {
        let response = r#"{"tags":["rust","helix-db","config"],"importance":0.7}"#;
        let result = parse_response(response).unwrap();
        assert_eq!(result.tags, vec!["rust", "helix-db", "config"]);
        assert!((result.importance - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_auto_tag_with_fences() {
        let response = "```json\n{\"tags\":[\"wsl2\",\"bug-fix\"],\"importance\":0.6}\n```";
        let result = parse_response(response).unwrap();
        assert_eq!(result.tags, vec!["wsl2", "bug-fix"]);
        assert!((result.importance - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_auto_tag_invalid_json() {
        assert!(parse_response("not valid json").is_none());
    }

    #[test]
    fn test_parse_auto_tag_empty_tags() {
        assert!(parse_response(r#"{"tags":[],"importance":0.5}"#).is_none());
    }

    #[test]
    fn test_parse_auto_tag_clamps_importance() {
        let result = parse_response(r#"{"tags":["test"],"importance":1.5}"#).unwrap();
        assert!((result.importance - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_auto_tag_missing_importance_defaults() {
        let result = parse_response(r#"{"tags":["test"]}"#).unwrap();
        assert!((result.importance - 0.5).abs() < f32::EPSILON);
    }
}
```

**Step 3: Run tests**

```bash
cargo test -p shabka-core --no-default-features -- auto_tag::tests
```

**Step 4: Commit**

```bash
git add crates/shabka-core/src/auto_tag.rs
git commit -m "refactor(auto_tag): use generate_structured instead of manual JSON parsing"
```

---

### Task 5: Convert `consolidate.rs` to structured extraction

**Files:**
- Modify: `crates/shabka-core/src/consolidate.rs`

**Step 1: Define response struct**

Add near the top (after imports):

```rust
use serde::Deserialize;

/// Raw JSON response from the LLM for consolidation.
#[derive(Deserialize, Debug)]
struct ConsolidateLlmResponse {
    title: String,
    content: String,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_importance")]
    importance: f64,
}

fn default_kind() -> String {
    "observation".to_string()
}

fn default_importance() -> f64 {
    0.5
}
```

**Step 2: Replace `consolidate_cluster` and delete `parse_consolidated_response`**

```rust
pub async fn consolidate_cluster(
    cluster: &[Memory],
    llm: &LlmService,
) -> std::result::Result<ConsolidatedMemory, String> {
    let mut prompt = String::from("MEMORIES TO CONSOLIDATE:\n\n");
    for (idx, memory) in cluster.iter().enumerate() {
        prompt.push_str(&format!(
            "--- Memory {} ---\nTitle: {}\nKind: {}\nContent: {}\nTags: {}\n\n",
            idx + 1,
            memory.title,
            memory.kind,
            memory.content,
            memory.tags.join(", "),
        ));
    }
    prompt.push_str("Merge these into a single comprehensive memory.");

    let response: ConsolidateLlmResponse = llm
        .generate_structured(&prompt, Some(CONSOLIDATE_SYSTEM_PROMPT))
        .await
        .map_err(|e| format!("LLM call failed: {e}"))?;

    let kind: MemoryKind = response.kind.parse().unwrap_or(MemoryKind::Observation);
    let tags: Vec<String> = response.tags.into_iter().map(|t| t.to_lowercase()).collect();
    let importance = (response.importance as f32).clamp(0.0, 1.0);

    Ok(ConsolidatedMemory {
        title: response.title,
        content: response.content,
        kind,
        tags,
        importance,
    })
}
```

Delete the old `parse_consolidated_response` function.

**Step 3: Update tests to use struct deserialization**

Replace the `parse_consolidated_response` test calls with direct struct deserialization tests (same pattern as auto_tag — keep a local `parse_response` test helper).

**Step 4: Run tests**

```bash
cargo test -p shabka-core --no-default-features -- consolidate::tests
```

**Step 5: Commit**

```bash
git add crates/shabka-core/src/consolidate.rs
git commit -m "refactor(consolidate): use generate_structured instead of manual JSON parsing"
```

---

### Task 6: Convert `dedup.rs` to structured extraction

**Files:**
- Modify: `crates/shabka-core/src/dedup.rs`

**Step 1: Define response struct**

```rust
use serde::Deserialize;

/// Raw JSON response from the LLM for dedup decisions.
#[derive(Deserialize, Debug)]
struct DedupLlmResponse {
    decision: String,
    target_id: Option<serde_json::Value>,
    merged_title: Option<String>,
    merged_content: Option<String>,
    reason: Option<String>,
}
```

Note: `target_id` is `Option<serde_json::Value>` because LLMs may return it as string `"0"` or number `0`.

**Step 2: Replace `check_duplicate_with_llm` and simplify parsing**

```rust
async fn check_duplicate_with_llm(
    llm: &LlmService,
    new_title: &str,
    new_content: &str,
    candidates: &[&(Memory, f32)],
) -> std::result::Result<DedupDecision, String> {
    let (prompt, id_mapping) = build_dedup_prompt(new_title, new_content, candidates);

    let response: DedupLlmResponse = llm
        .generate_structured(&prompt, Some(DEDUP_SYSTEM_PROMPT))
        .await
        .map_err(|e| format!("LLM call failed: {e}"))?;

    map_dedup_response(response, &id_mapping)
}
```

**Step 3: Replace `parse_llm_response` and `resolve_target_id` with `map_dedup_response`**

```rust
/// Map the typed LLM response to a DedupDecision, resolving temp IDs.
fn map_dedup_response(
    response: DedupLlmResponse,
    id_mapping: &[(Uuid, String, f32)],
) -> std::result::Result<DedupDecision, String> {
    let decision = response.decision.to_uppercase();

    match decision.as_str() {
        "ADD" => Ok(DedupDecision::Add),
        "SKIP" => {
            let idx = resolve_target_id_from_value(&response.target_id, id_mapping)?;
            let (id, title, sim) = &id_mapping[idx];
            Ok(DedupDecision::Skip {
                existing_id: *id,
                existing_title: title.clone(),
                similarity: *sim,
            })
        }
        "UPDATE" => {
            let idx = resolve_target_id_from_value(&response.target_id, id_mapping)?;
            let (id, title, sim) = &id_mapping[idx];
            let merged_title = response.merged_title.unwrap_or_else(|| title.clone());
            let merged_content = response
                .merged_content
                .ok_or("UPDATE decision missing 'merged_content'")?;
            Ok(DedupDecision::Update {
                existing_id: *id,
                existing_title: title.clone(),
                merged_content,
                merged_title,
                similarity: *sim,
            })
        }
        "CONTRADICT" => {
            let idx = resolve_target_id_from_value(&response.target_id, id_mapping)?;
            let (id, title, sim) = &id_mapping[idx];
            let reason = response
                .reason
                .unwrap_or_else(|| "contradicts existing memory".to_string());
            Ok(DedupDecision::Contradict {
                existing_id: *id,
                existing_title: title.clone(),
                similarity: *sim,
                reason,
            })
        }
        other => Err(format!("unknown decision: '{other}'")),
    }
}

/// Resolve target_id from a JSON value (string "0" or number 0) to an index.
fn resolve_target_id_from_value(
    target_id: &Option<serde_json::Value>,
    id_mapping: &[(Uuid, String, f32)],
) -> std::result::Result<usize, String> {
    let value = target_id.as_ref().ok_or("missing 'target_id' field")?;

    let idx: usize = if let Some(s) = value.as_str() {
        s.parse()
            .map_err(|_| format!("target_id '{s}' is not a valid integer"))?
    } else if let Some(n) = value.as_u64() {
        n as usize
    } else {
        return Err("target_id is not a valid integer".to_string());
    };

    if idx >= id_mapping.len() {
        return Err(format!(
            "target_id {idx} out of range (0..{})",
            id_mapping.len()
        ));
    }

    Ok(idx)
}
```

Delete the old `parse_llm_response` and `resolve_target_id` functions.

**Step 4: Update tests**

Keep `build_dedup_prompt` test unchanged. Convert `parse_llm_response` tests to test `map_dedup_response` with `DedupLlmResponse` structs instead of raw JSON strings. Keep the same assertions.

**Step 5: Run tests**

```bash
cargo test -p shabka-core --no-default-features -- dedup::tests
```

**Step 6: Commit**

```bash
git add crates/shabka-core/src/dedup.rs
git commit -m "refactor(dedup): use generate_structured instead of manual JSON parsing"
```

---

### Task 7: Add 5 new LLM providers

**Files:**
- Modify: `crates/shabka-core/src/llm.rs:100-210`
- Modify: `crates/shabka-core/src/embedding/mod.rs`
- Modify: `crates/shabka-core/src/config/mod.rs`

**Step 1: Add new match arms in `LlmService::from_config()`**

After the `"anthropic" | "claude"` arm, add:

```rust
"deepseek" => {
    let api_key = config::resolve_api_key(
        config.api_key.as_deref(),
        config.env_var.as_deref(),
        "DEEPSEEK_API_KEY",
        "deepseek",
        "LLM",
    )?;

    let client = rig::providers::deepseek::Client::<reqwest::Client>::builder()
        .api_key(&api_key)
        .build()
        .map_err(|e| ShabkaError::Llm(format!("failed to build DeepSeek LLM client: {e}")))?;

    use rig::prelude::CompletionClient;
    let model = client.completion_model(&config.model);
    Box::new(RigCompletionWrapper { model, model_name: config.model.clone() })
}

"groq" => {
    let api_key = config::resolve_api_key(
        config.api_key.as_deref(),
        config.env_var.as_deref(),
        "GROQ_API_KEY",
        "groq",
        "LLM",
    )?;

    let client = rig::providers::groq::Client::<reqwest::Client>::builder()
        .api_key(&api_key)
        .build()
        .map_err(|e| ShabkaError::Llm(format!("failed to build Groq LLM client: {e}")))?;

    use rig::prelude::CompletionClient;
    let model = client.completion_model(&config.model);
    Box::new(RigCompletionWrapper { model, model_name: config.model.clone() })
}

"xai" => {
    let api_key = config::resolve_api_key(
        config.api_key.as_deref(),
        config.env_var.as_deref(),
        "XAI_API_KEY",
        "xai",
        "LLM",
    )?;

    let client = rig::providers::xai::Client::<reqwest::Client>::builder()
        .api_key(&api_key)
        .build()
        .map_err(|e| ShabkaError::Llm(format!("failed to build xAI LLM client: {e}")))?;

    use rig::prelude::CompletionClient;
    let model = client.completion_model(&config.model);
    Box::new(RigCompletionWrapper { model, model_name: config.model.clone() })
}

"cohere" => {
    let api_key = config::resolve_api_key(
        config.api_key.as_deref(),
        config.env_var.as_deref(),
        "COHERE_API_KEY",
        "cohere",
        "LLM",
    )?;

    let client = rig::providers::cohere::Client::<reqwest::Client>::builder()
        .api_key(&api_key)
        .build()
        .map_err(|e| ShabkaError::Llm(format!("failed to build Cohere LLM client: {e}")))?;

    use rig::prelude::CompletionClient;
    let model = client.completion_model(&config.model);
    Box::new(RigCompletionWrapper { model, model_name: config.model.clone() })
}
```

Update the error message in the `other` arm to list all providers.

**Step 2: Add Cohere to `EmbeddingService::from_config()`**

In `crates/shabka-core/src/embedding/mod.rs`, add before the `"hash"` arm:

```rust
"cohere" => {
    let api_key = config::resolve_api_key(
        config.api_key.as_deref(),
        config.env_var.as_deref(),
        "COHERE_API_KEY",
        "cohere",
        "embedding",
    )?;

    let model_name = if config.model == "hash-128d" {
        "embed-english-v3.0".to_string()
    } else {
        config.model.clone()
    };

    let dims = config.dimensions.unwrap_or(1024);

    let client = rig::providers::cohere::Client::<reqwest::Client>::builder()
        .api_key(&api_key)
        .build()
        .map_err(|e| ShabkaError::Embedding(format!("failed to build Cohere client: {e}")))?;

    use rig::prelude::EmbeddingsClient;
    let model = client.embedding_model_with_ndims(&model_name, dims);

    Ok(Self {
        inner: EmbeddingInner::Rig(Box::new(RigModelWrapper {
            model,
            model_name,
        })),
        provider: "cohere",
        dimensions: dims,
    })
}
```

**Step 3: Update config constants**

In `crates/shabka-core/src/config/mod.rs`:

```rust
pub const VALID_LLM_PROVIDERS: &[&str] = &["ollama", "openai", "gemini", "anthropic", "deepseek", "groq", "xai", "cohere"];
pub const VALID_PROVIDERS: &[&str] = &["hash", "ollama", "openai", "gemini", "cohere"];
```

**Step 4: Add tests for new providers**

In `llm.rs` tests, add key-required tests for each new provider (same pattern as existing tests):

```rust
#[test]
fn test_from_config_deepseek_without_key_errors() {
    let saved = std::env::var("DEEPSEEK_API_KEY").ok();
    std::env::remove_var("DEEPSEEK_API_KEY");
    let config = LlmConfig { provider: "deepseek".into(), model: "deepseek-chat".into(), api_key: None, ..Default::default() };
    let result = LlmService::from_config(&config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("API key"));
    if let Some(key) = saved { std::env::set_var("DEEPSEEK_API_KEY", key); }
}

// Same pattern for groq, xai, cohere
```

In `embedding/mod.rs` tests, add:
```rust
#[test]
fn test_cohere_without_key_errors() { ... }
```

Update `test_valid_providers_list` to assert new providers.

**Step 5: Run tests**

```bash
cargo test -p shabka-core --no-default-features
```

**Step 6: Commit**

```bash
git add crates/shabka-core/src/llm.rs crates/shabka-core/src/embedding/mod.rs crates/shabka-core/src/config/mod.rs
git commit -m "feat: add deepseek, groq, xai, cohere LLM providers + cohere embeddings"
```

---

### Task 8: Clippy + final validation

**Step 1: Run clippy**

```bash
cargo clippy --workspace --no-default-features -- -D warnings
```

**Step 2: Run full test suite**

```bash
cargo test -p shabka-core --no-default-features
cargo test -p shabka-hooks --no-default-features
```

**Step 3: Run integration tests**

```bash
cargo test -p shabka-core --no-default-features -- --ignored
```

**Step 4: Commit any fixes, then push**

```bash
git push origin main
```
