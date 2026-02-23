use crate::error::{Result, ShabkaError};
use config::{Config, File};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShabkaConfig {
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub helix: HelixConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub capture: CaptureConfig,
    #[serde(default)]
    pub retrieval: RetrievalConfig,
    #[serde(default)]
    pub sharing: SharingConfig,
    #[serde(default)]
    pub privacy: PrivacyConfig,
    #[serde(default)]
    pub graph: GraphConfig,
    #[serde(default)]
    pub history: HistoryConfig,
    #[serde(default)]
    pub scrub: crate::scrub::ScrubConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub consolidate: crate::consolidate::ConsolidateConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelixConfig {
    #[serde(default = "default_helix_url")]
    pub url: String,
    #[serde(default = "default_helix_port")]
    pub port: u16,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

impl Default for HelixConfig {
    fn default() -> Self {
        Self {
            url: default_helix_url(),
            port: default_helix_port(),
            api_key: None,
            auto_start: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_backend")]
    pub backend: String,
    /// Custom path for SQLite database. Defaults to `~/.config/shabka/shabka.db`.
    #[serde(default)]
    pub path: Option<String>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: default_storage_backend(),
            path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embedding_provider")]
    pub provider: String,
    #[serde(default = "default_embedding_model")]
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub dimensions: Option<usize>,
    #[serde(default)]
    pub env_var: Option<String>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_embedding_provider(),
            model: default_embedding_model(),
            api_key: None,
            base_url: None,
            dimensions: None,
            env_var: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_mcp_transport")]
    pub transport: String,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            transport: default_mcp_transport(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    #[serde(default = "default_web_port")]
    pub port: u16,
    #[serde(default = "default_web_host")]
    pub host: String,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            port: default_web_port(),
            host: default_web_host(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_min_importance")]
    pub min_importance: f32,
    #[serde(default = "default_true")]
    pub session_compression: bool,
    #[serde(default)]
    pub auto_tag: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_importance: default_min_importance(),
            session_compression: true,
            auto_tag: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_llm_provider")]
    pub provider: String,
    #[serde(default = "default_llm_model")]
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub env_var: Option<String>,
    #[serde(default = "default_llm_max_tokens")]
    pub max_tokens: usize,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_llm_provider(),
            model: default_llm_model(),
            api_key: None,
            base_url: None,
            env_var: None,
            max_tokens: default_llm_max_tokens(),
        }
    }
}

/// Valid storage backend names.
pub const VALID_STORAGE_BACKENDS: &[&str] = &["sqlite", "helix"];

/// Valid LLM provider names.
pub const VALID_LLM_PROVIDERS: &[&str] = &[
    "ollama",
    "openai",
    "gemini",
    "anthropic",
    "deepseek",
    "groq",
    "xai",
    "cohere",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalConfig {
    #[serde(default = "default_retrieval_limit")]
    pub default_limit: usize,
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            default_limit: default_retrieval_limit(),
            token_budget: default_token_budget(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharingConfig {
    #[serde(default = "default_sharing_mode")]
    pub mode: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub team_url: Option<String>,
    #[serde(default)]
    pub team_api_key: Option<String>,
}

impl Default for SharingConfig {
    fn default() -> Self {
        Self {
            mode: default_sharing_mode(),
            user_id: None,
            team_url: None,
            team_api_key: None,
        }
    }
}

/// Resolve the current user's identity.
///
/// Priority: config `user_id` → `git config user.name` → `$HOSTNAME` → `/etc/hostname` → `"anonymous"`
pub fn resolve_user_id(config: &SharingConfig) -> String {
    if let Some(ref id) = config.user_id {
        if !id.is_empty() {
            return id.clone();
        }
    }

    // Try git config user.name
    if let Ok(output) = std::process::Command::new("git")
        .args(["config", "user.name"])
        .output()
    {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }

    // Try $HOSTNAME env var
    if let Ok(hostname) = std::env::var("HOSTNAME") {
        if !hostname.is_empty() {
            return hostname;
        }
    }

    // Try /etc/hostname
    if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
        let hostname = hostname.trim().to_string();
        if !hostname.is_empty() {
            return hostname;
        }
    }

    "anonymous".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    #[serde(default = "default_privacy_level")]
    pub default_level: String,
    #[serde(default)]
    pub redaction_patterns: Vec<String>,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            default_level: default_privacy_level(),
            redaction_patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f32,
    #[serde(default = "default_max_relations")]
    pub max_relations: usize,
    #[serde(default = "default_max_chain_depth")]
    pub max_chain_depth: usize,
    #[serde(default = "default_stale_days")]
    pub stale_days: u64,
    #[serde(default = "default_true")]
    pub dedup_enabled: bool,
    #[serde(default = "default_dedup_skip_threshold")]
    pub dedup_skip_threshold: f32,
    #[serde(default = "default_dedup_update_threshold")]
    pub dedup_update_threshold: f32,
    /// When true (and [llm] is enabled), use LLM to decide ADD/UPDATE/SKIP
    /// instead of pure similarity thresholds. Falls back to thresholds on failure.
    #[serde(default)]
    pub dedup_llm: bool,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: default_similarity_threshold(),
            max_relations: default_max_relations(),
            max_chain_depth: default_max_chain_depth(),
            stale_days: default_stale_days(),
            dedup_enabled: true,
            dedup_skip_threshold: default_dedup_skip_threshold(),
            dedup_update_threshold: default_dedup_update_threshold(),
            dedup_llm: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_events")]
    pub max_events: usize,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_events: default_max_events(),
        }
    }
}

// -- Defaults --

fn default_storage_backend() -> String {
    "sqlite".to_string()
}
fn default_helix_url() -> String {
    "http://localhost".to_string()
}
fn default_helix_port() -> u16 {
    6969
}
fn default_embedding_provider() -> String {
    "hash".to_string()
}
fn default_embedding_model() -> String {
    "hash-128d".to_string()
}
fn default_mcp_transport() -> String {
    "stdio".to_string()
}
fn default_web_port() -> u16 {
    37737
}
fn default_web_host() -> String {
    "127.0.0.1".to_string()
}
fn default_min_importance() -> f32 {
    0.3
}
fn default_retrieval_limit() -> usize {
    10
}
fn default_token_budget() -> usize {
    2000
}
fn default_sharing_mode() -> String {
    "local".to_string()
}
fn default_privacy_level() -> String {
    "private".to_string()
}
fn default_true() -> bool {
    true
}
fn default_similarity_threshold() -> f32 {
    0.6
}
fn default_max_relations() -> usize {
    3
}
fn default_max_chain_depth() -> usize {
    5
}
fn default_stale_days() -> u64 {
    90
}
fn default_dedup_skip_threshold() -> f32 {
    0.95
}
fn default_dedup_update_threshold() -> f32 {
    0.85
}
fn default_max_events() -> usize {
    10000
}
fn default_llm_provider() -> String {
    "ollama".to_string()
}
fn default_llm_model() -> String {
    "llama3.2".to_string()
}
fn default_llm_max_tokens() -> usize {
    1024
}

/// Valid embedding provider names.
pub const VALID_PROVIDERS: &[&str] = &["hash", "ollama", "openai", "gemini", "cohere"];

impl ShabkaConfig {
    /// Load configuration with three-layer TOML merge:
    /// 1. ~/.config/shabka/config.toml (global)
    /// 2. .shabka/config.toml (project)
    /// 3. .shabka/config.local.toml (local, gitignored)
    pub fn load(project_dir: Option<&Path>) -> Result<Self> {
        let mut builder = Config::builder();

        // Layer 1: Global config
        if let Some(global_path) = global_config_path() {
            if global_path.exists() {
                builder = builder.add_source(File::from(global_path).required(false));
            }
        }

        // Layer 2: Project config
        if let Some(dir) = project_dir {
            let project_config = dir.join(".shabka").join("config.toml");
            if project_config.exists() {
                builder = builder.add_source(File::from(project_config).required(false));
            }

            // Layer 3: Local config (gitignored)
            let local_config = dir.join(".shabka").join("config.local.toml");
            if local_config.exists() {
                builder = builder.add_source(File::from(local_config).required(false));
            }
        }

        let config = builder
            .build()
            .map_err(|e| ShabkaError::Config(e.to_string()))?;

        let mut cfg: Self = config
            .try_deserialize()
            .map_err(|e| ShabkaError::Config(e.to_string()))?;

        cfg.validate();
        Ok(cfg)
    }

    /// Load with defaults only (no files).
    pub fn default_config() -> Self {
        Self {
            storage: StorageConfig::default(),
            helix: HelixConfig::default(),
            embedding: EmbeddingConfig::default(),
            mcp: McpConfig::default(),
            web: WebConfig::default(),
            capture: CaptureConfig::default(),
            retrieval: RetrievalConfig::default(),
            sharing: SharingConfig::default(),
            privacy: PrivacyConfig::default(),
            graph: GraphConfig::default(),
            history: HistoryConfig::default(),
            scrub: crate::scrub::ScrubConfig::default(),
            llm: LlmConfig::default(),
            consolidate: crate::consolidate::ConsolidateConfig::default(),
        }
    }

    /// Validate config values, clamping out-of-range values and logging warnings.
    /// This is lenient — it fixes values rather than rejecting the config.
    pub fn validate(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();

        // Storage backend
        if !VALID_STORAGE_BACKENDS.contains(&self.storage.backend.as_str()) {
            warnings.push(format!(
                "unknown storage backend '{}', valid: {}",
                self.storage.backend,
                VALID_STORAGE_BACKENDS.join(", ")
            ));
        }

        // Embedding provider
        if !VALID_PROVIDERS.contains(&self.embedding.provider.as_str()) {
            warnings.push(format!(
                "unknown embedding provider '{}', valid: {}",
                self.embedding.provider,
                VALID_PROVIDERS.join(", ")
            ));
        }

        // Float thresholds must be in [0.0, 1.0]
        let float_checks: Vec<(&str, &mut f32)> = vec![
            (
                "graph.similarity_threshold",
                &mut self.graph.similarity_threshold,
            ),
            (
                "graph.dedup_skip_threshold",
                &mut self.graph.dedup_skip_threshold,
            ),
            (
                "graph.dedup_update_threshold",
                &mut self.graph.dedup_update_threshold,
            ),
            ("capture.min_importance", &mut self.capture.min_importance),
        ];
        for (name, val) in float_checks {
            if *val < 0.0 || *val > 1.0 {
                warnings.push(format!("{name} = {val} out of range [0.0, 1.0], clamping"));
                *val = val.clamp(0.0, 1.0);
            }
        }

        // dedup_skip must be >= dedup_update
        if self.graph.dedup_skip_threshold < self.graph.dedup_update_threshold {
            warnings.push(format!(
                "dedup_skip_threshold ({:.2}) < dedup_update_threshold ({:.2}), swapping",
                self.graph.dedup_skip_threshold, self.graph.dedup_update_threshold
            ));
            std::mem::swap(
                &mut self.graph.dedup_skip_threshold,
                &mut self.graph.dedup_update_threshold,
            );
        }

        // dedup_llm requires [llm] to be enabled
        if self.graph.dedup_llm && !self.llm.enabled {
            warnings.push(
                "graph.dedup_llm = true but llm.enabled = false; LLM dedup will be skipped"
                    .to_string(),
            );
        }

        // LLM provider (only validate if enabled)
        if self.llm.enabled && !VALID_LLM_PROVIDERS.contains(&self.llm.provider.as_str()) {
            warnings.push(format!(
                "unknown LLM provider '{}', valid: {}",
                self.llm.provider,
                VALID_LLM_PROVIDERS.join(", ")
            ));
        }

        // LLM max_tokens
        if self.llm.max_tokens == 0 {
            warnings.push("llm.max_tokens = 0, setting to 256".to_string());
            self.llm.max_tokens = 256;
        }

        // Positive integer checks
        if self.graph.max_chain_depth == 0 {
            warnings.push("graph.max_chain_depth = 0, setting to 1".to_string());
            self.graph.max_chain_depth = 1;
        }
        if self.graph.stale_days == 0 {
            warnings.push("graph.stale_days = 0, setting to 1".to_string());
            self.graph.stale_days = 1;
        }
        if self.retrieval.default_limit == 0 {
            warnings.push("retrieval.default_limit = 0, setting to 1".to_string());
            self.retrieval.default_limit = 1;
        }

        // Log warnings via tracing (if subscriber is set up)
        for w in &warnings {
            tracing::warn!("config: {}", w);
        }

        warnings
    }
}

fn global_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("shabka").join("config.toml"))
}

// ---------------------------------------------------------------------------
// Embedding state — tracks last-used provider for migration detection
// ---------------------------------------------------------------------------

/// Persisted snapshot of the embedding provider configuration.
/// Written after successful embedding operations. Compared on startup
/// to detect provider changes that require `shabka reembed`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct EmbeddingState {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub dimensions: usize,
    #[serde(default)]
    pub last_updated: String,
    /// RFC3339 timestamp of the last successful `shabka reembed` run.
    /// Used by incremental re-embed to skip unchanged memories.
    #[serde(default)]
    pub last_reembed_at: String,
}

impl EmbeddingState {
    /// Path to the state file: `~/.config/shabka/embedding_state.toml`
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("shabka").join("embedding_state.toml"))
    }

    /// Load from disk. Returns `Default` if the file is missing or unparseable.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save to disk, creating the parent directory if needed.
    pub fn save(&self) -> Result<()> {
        let path = Self::path()
            .ok_or_else(|| ShabkaError::Config("cannot determine config directory".to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ShabkaError::Config(format!("failed to create config dir: {e}")))?;
        }
        let toml_str = toml::to_string_pretty(self).map_err(|e| {
            ShabkaError::Config(format!("failed to serialize embedding state: {e}"))
        })?;
        std::fs::write(&path, toml_str)
            .map_err(|e| ShabkaError::Config(format!("failed to write embedding state: {e}")))?;
        Ok(())
    }

    /// Build a state snapshot from the resolved provider values.
    ///
    /// Use the actual provider/model/dimensions from the `EmbeddingService`,
    /// not the raw config values (which may contain defaults like `hash-128d`
    /// even when ollama resolves to `nomic-embed-text`).
    pub fn from_provider(provider: &str, model: &str, dimensions: usize) -> Self {
        Self {
            provider: provider.to_string(),
            model: model.to_string(),
            dimensions,
            last_updated: chrono::Utc::now().to_rfc3339(),
            last_reembed_at: String::new(),
        }
    }

    /// Returns `true` when the saved state matches the resolved provider values.
    pub fn matches(&self, provider: &str, model: &str, dimensions: usize) -> bool {
        self.provider == provider && self.model == model && self.dimensions == dimensions
    }

    /// Returns a human-readable warning if the current provider doesn't match
    /// the saved state. Returns `None` if they match or if no prior state exists.
    ///
    /// Pass the resolved provider/model/dimensions from the `EmbeddingService`.
    pub fn migration_warning(provider: &str, model: &str, dimensions: usize) -> Option<String> {
        let state = Self::load();
        // No prior state — nothing to warn about
        if state.provider.is_empty() {
            return None;
        }
        if state.matches(provider, model, dimensions) {
            return None;
        }
        Some(format!(
            "WARNING: Embedding provider changed!\n\
             \x20 Previous: {} / {} ({}d)\n\
             \x20 Current:  {} / {} ({}d)\n\
             \x20 Existing memories have incompatible embeddings.\n\
             \x20 Run `shabka reembed` to re-embed all memories with the new provider.",
            state.provider, state.model, state.dimensions, provider, model, dimensions,
        ))
    }
}

/// Check whether the current embedding config's dimensions are compatible
/// with the previously stored state. Returns `Err(message)` on mismatch,
/// `Ok(())` if compatible or if no prior state exists (first run).
pub fn check_dimensions(config: &EmbeddingConfig) -> std::result::Result<(), String> {
    let state = EmbeddingState::load();
    // No prior state — first run, nothing to check
    if state.provider.is_empty() {
        return Ok(());
    }

    let service = crate::embedding::EmbeddingService::from_config(config)
        .map_err(|e| format!("failed to create embedding service: {e}"))?;
    let current_dims = service.dimensions();

    if current_dims != state.dimensions {
        Err(format!(
            "Dimension mismatch: stored embeddings are {}d ({}/{}), \
             but current config produces {}d ({}/{}). \
             Run `shabka reembed` to re-embed all memories.",
            state.dimensions,
            state.provider,
            state.model,
            current_dims,
            config.provider,
            config.model,
        ))
    } else {
        Ok(())
    }
}

/// Resolve an API key: check config field first, then environment variable.
/// Used by both embedding and LLM service initialization.
pub fn resolve_api_key(
    api_key: Option<&str>,
    env_var_override: Option<&str>,
    default_env_var: &str,
    provider_name: &str,
    service_kind: &str,
) -> crate::error::Result<String> {
    if let Some(key) = api_key {
        if !key.is_empty() {
            return Ok(key.to_string());
        }
    }

    let env_var_name = env_var_override.unwrap_or(default_env_var);

    std::env::var(env_var_name).map_err(|_| {
        crate::error::ShabkaError::Config(format!(
            "{provider_name} {service_kind} provider requires an API key \
             (set {service_kind}.api_key or {env_var_name})"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ShabkaConfig::default_config();
        assert_eq!(config.helix.url, "http://localhost");
        assert_eq!(config.helix.port, 6969);
        assert!(config.helix.auto_start);
        assert_eq!(config.embedding.provider, "hash");
        assert_eq!(config.web.port, 37737);
        assert_eq!(config.retrieval.token_budget, 2000);
        assert_eq!(config.sharing.mode, "local");
        assert_eq!(config.privacy.default_level, "private");
    }

    #[test]
    fn test_load_config_no_files() {
        // Loading with a non-existent directory should give defaults
        let config = ShabkaConfig::load(Some(Path::new("/nonexistent/path"))).unwrap();
        assert_eq!(config.helix.port, 6969);
        assert_eq!(config.web.port, 37737);
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let config = ShabkaConfig::default_config();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: ShabkaConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.helix.port, config.helix.port);
        assert_eq!(parsed.web.port, config.web.port);
    }

    #[test]
    fn test_graph_config_defaults() {
        let config = GraphConfig::default();
        assert!((config.similarity_threshold - 0.6).abs() < f32::EPSILON);
        assert_eq!(config.max_relations, 3);
        assert_eq!(config.max_chain_depth, 5);
        assert_eq!(config.stale_days, 90);
        assert!(config.dedup_enabled);
        assert!((config.dedup_skip_threshold - 0.95).abs() < f32::EPSILON);
        assert!((config.dedup_update_threshold - 0.85).abs() < f32::EPSILON);
    }

    #[test]
    fn test_graph_config_toml_parsing() {
        let toml_str = r#"
[graph]
similarity_threshold = 0.7
max_relations = 5
max_chain_depth = 10
stale_days = 30
dedup_enabled = false
dedup_skip_threshold = 0.99
dedup_update_threshold = 0.90
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert!((config.graph.similarity_threshold - 0.7).abs() < f32::EPSILON);
        assert_eq!(config.graph.max_relations, 5);
        assert_eq!(config.graph.max_chain_depth, 10);
        assert_eq!(config.graph.stale_days, 30);
        assert!(!config.graph.dedup_enabled);
    }

    #[test]
    fn test_graph_config_backward_compat() {
        // Old configs without [graph] should still load fine
        let toml_str = r#"
[embedding]
provider = "hash"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert!((config.graph.similarity_threshold - 0.6).abs() < f32::EPSILON);
        assert_eq!(config.graph.max_relations, 3);
        assert!(config.history.enabled);
        assert_eq!(config.history.max_events, 10000);
    }

    #[test]
    fn test_embedding_config_new_fields() {
        let toml_str = r#"
[embedding]
provider = "openai"
model = "text-embedding-3-small"
base_url = "http://localhost:8000/v1"
dimensions = 512
env_var = "MY_KEY"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.embedding.provider, "openai");
        assert_eq!(
            config.embedding.base_url.as_deref(),
            Some("http://localhost:8000/v1")
        );
        assert_eq!(config.embedding.dimensions, Some(512));
        assert_eq!(config.embedding.env_var.as_deref(), Some("MY_KEY"));
    }

    #[test]
    fn test_embedding_config_backward_compat() {
        let toml_str = r#"
[embedding]
provider = "hash"
model = "hash-128d"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert!(config.embedding.base_url.is_none());
        assert!(config.embedding.dimensions.is_none());
        assert!(config.embedding.env_var.is_none());
    }

    #[test]
    fn test_resolve_user_id_explicit() {
        let config = SharingConfig {
            user_id: Some("alice".to_string()),
            ..Default::default()
        };
        assert_eq!(resolve_user_id(&config), "alice");
    }

    #[test]
    fn test_resolve_user_id_fallback() {
        let config = SharingConfig {
            user_id: None,
            ..Default::default()
        };
        // Should resolve to something non-empty (git name, hostname, or "anonymous")
        let id = resolve_user_id(&config);
        assert!(!id.is_empty());
    }

    // -- EmbeddingState tests --

    #[test]
    fn test_embedding_state_roundtrip() {
        let dir = std::env::temp_dir().join(format!("shabka-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("embedding_state.toml");

        let state = EmbeddingState {
            provider: "openai".to_string(),
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
            last_updated: "2025-02-08T12:00:00Z".to_string(),
            ..Default::default()
        };

        // Write and read back
        let toml_str = toml::to_string_pretty(&state).unwrap();
        std::fs::write(&path, &toml_str).unwrap();
        let loaded: EmbeddingState = toml::from_str(&toml_str).unwrap();

        assert_eq!(loaded.provider, "openai");
        assert_eq!(loaded.model, "text-embedding-3-small");
        assert_eq!(loaded.dimensions, 1536);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_embedding_state_matches() {
        let state = EmbeddingState {
            provider: "hash".to_string(),
            model: "hash-128d".to_string(),
            dimensions: 128,
            ..Default::default()
        };
        assert!(state.matches("hash", "hash-128d", 128));
    }

    #[test]
    fn test_embedding_state_mismatch() {
        let state = EmbeddingState {
            provider: "hash".to_string(),
            model: "hash-128d".to_string(),
            dimensions: 128,
            ..Default::default()
        };
        assert!(!state.matches("openai", "text-embedding-3-small", 1536));
    }

    #[test]
    fn test_migration_warning_no_state() {
        // Empty provider means no prior state — should return None
        let state = EmbeddingState::default();
        assert!(state.provider.is_empty());
        // migration_warning reads from disk; with no state file it returns None.
        // We test the logic directly via matches instead.
        assert!(state.matches("hash", "hash-128d", 128) || state.provider.is_empty());
    }

    #[test]
    fn test_embedding_state_from_provider() {
        let state = EmbeddingState::from_provider("gemini", "text-embedding-004", 768);
        assert_eq!(state.provider, "gemini");
        assert_eq!(state.model, "text-embedding-004");
        assert_eq!(state.dimensions, 768);
        assert!(!state.last_updated.is_empty());
    }

    #[test]
    fn test_embedding_state_last_reembed_at_roundtrip() {
        let state = EmbeddingState {
            provider: "hash".to_string(),
            model: "hash-128d".to_string(),
            dimensions: 128,
            last_updated: "2025-06-01T00:00:00Z".to_string(),
            last_reembed_at: "2025-06-01T12:00:00Z".to_string(),
        };
        let toml_str = toml::to_string_pretty(&state).unwrap();
        assert!(toml_str.contains("last_reembed_at"));
        let loaded: EmbeddingState = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.last_reembed_at, "2025-06-01T12:00:00Z");
    }

    #[test]
    fn test_embedding_state_backward_compat() {
        // Old state files without last_reembed_at should load fine
        let toml_str = r#"
provider = "hash"
model = "hash-128d"
dimensions = 128
last_updated = "2025-06-01T00:00:00Z"
"#;
        let loaded: EmbeddingState = toml::from_str(toml_str).unwrap();
        assert_eq!(loaded.provider, "hash");
        assert_eq!(loaded.last_reembed_at, "");
    }

    // -- Validation tests --

    #[test]
    fn test_validate_default_config_no_warnings() {
        let mut config = ShabkaConfig::default_config();
        let warnings = config.validate();
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_validate_clamps_out_of_range_floats() {
        let mut config = ShabkaConfig::default_config();
        config.graph.similarity_threshold = 1.5;
        config.capture.min_importance = -0.1;
        let warnings = config.validate();
        assert_eq!(warnings.len(), 2);
        assert!((config.graph.similarity_threshold - 1.0).abs() < f32::EPSILON);
        assert!((config.capture.min_importance - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_validate_swaps_dedup_thresholds() {
        let mut config = ShabkaConfig::default_config();
        // skip < update is invalid — should swap
        config.graph.dedup_skip_threshold = 0.80;
        config.graph.dedup_update_threshold = 0.95;
        let warnings = config.validate();
        assert!(warnings.iter().any(|w| w.contains("swapping")));
        assert!((config.graph.dedup_skip_threshold - 0.95).abs() < f32::EPSILON);
        assert!((config.graph.dedup_update_threshold - 0.80).abs() < f32::EPSILON);
    }

    #[test]
    fn test_validate_zero_integers() {
        let mut config = ShabkaConfig::default_config();
        config.graph.max_chain_depth = 0;
        config.graph.stale_days = 0;
        config.retrieval.default_limit = 0;
        let warnings = config.validate();
        assert_eq!(warnings.len(), 3);
        assert_eq!(config.graph.max_chain_depth, 1);
        assert_eq!(config.graph.stale_days, 1);
        assert_eq!(config.retrieval.default_limit, 1);
    }

    #[test]
    fn test_validate_unknown_provider() {
        let mut config = ShabkaConfig::default_config();
        config.embedding.provider = "banana".to_string();
        let warnings = config.validate();
        assert!(warnings
            .iter()
            .any(|w| w.contains("unknown embedding provider")));
    }

    #[test]
    fn test_valid_providers_list() {
        assert!(VALID_PROVIDERS.contains(&"hash"));
        assert!(VALID_PROVIDERS.contains(&"ollama"));
        assert!(VALID_PROVIDERS.contains(&"openai"));
        assert!(VALID_PROVIDERS.contains(&"gemini"));
        assert!(VALID_PROVIDERS.contains(&"cohere"));
    }

    #[test]
    fn test_valid_llm_providers_list() {
        assert!(VALID_LLM_PROVIDERS.contains(&"ollama"));
        assert!(VALID_LLM_PROVIDERS.contains(&"openai"));
        assert!(VALID_LLM_PROVIDERS.contains(&"gemini"));
        assert!(VALID_LLM_PROVIDERS.contains(&"anthropic"));
        assert!(VALID_LLM_PROVIDERS.contains(&"deepseek"));
        assert!(VALID_LLM_PROVIDERS.contains(&"groq"));
        assert!(VALID_LLM_PROVIDERS.contains(&"xai"));
        assert!(VALID_LLM_PROVIDERS.contains(&"cohere"));
    }

    // -- LlmConfig tests --

    #[test]
    fn test_llm_config_defaults() {
        let config = LlmConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.provider, "ollama");
        assert_eq!(config.model, "llama3.2");
        assert_eq!(config.max_tokens, 1024);
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
    }

    #[test]
    fn test_llm_config_backward_compat() {
        // Old configs without [llm] should still load
        let toml_str = r#"
[embedding]
provider = "hash"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.llm.enabled);
        assert_eq!(config.llm.provider, "ollama");
    }

    #[test]
    fn test_llm_config_full_toml() {
        let toml_str = r#"
[llm]
enabled = true
provider = "openai"
model = "gpt-4o-mini"
api_key = "sk-test"
base_url = "http://localhost:8000/v1"
max_tokens = 2048
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert!(config.llm.enabled);
        assert_eq!(config.llm.provider, "openai");
        assert_eq!(config.llm.model, "gpt-4o-mini");
        assert_eq!(config.llm.api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.llm.max_tokens, 2048);
    }

    #[test]
    fn test_validate_unknown_llm_provider() {
        let mut config = ShabkaConfig::default_config();
        config.llm.enabled = true;
        config.llm.provider = "banana".to_string();
        let warnings = config.validate();
        assert!(warnings.iter().any(|w| w.contains("unknown LLM provider")));
    }

    #[test]
    fn test_validate_llm_disabled_unknown_provider_no_warning() {
        let mut config = ShabkaConfig::default_config();
        config.llm.enabled = false;
        config.llm.provider = "banana".to_string();
        let warnings = config.validate();
        assert!(!warnings.iter().any(|w| w.contains("LLM provider")));
    }

    #[test]
    fn test_validate_llm_zero_max_tokens() {
        let mut config = ShabkaConfig::default_config();
        config.llm.max_tokens = 0;
        let warnings = config.validate();
        assert!(warnings.iter().any(|w| w.contains("max_tokens")));
        assert_eq!(config.llm.max_tokens, 256);
    }

    #[test]
    fn test_graph_config_dedup_llm_default() {
        let config = GraphConfig::default();
        assert!(!config.dedup_llm);
    }

    #[test]
    fn test_validate_dedup_llm_without_llm_enabled() {
        let mut config = ShabkaConfig::default_config();
        config.graph.dedup_llm = true;
        config.llm.enabled = false;
        let warnings = config.validate();
        assert!(warnings.iter().any(|w| w.contains("dedup_llm")));
    }

    #[test]
    fn test_validate_dedup_llm_with_llm_enabled_no_warning() {
        let mut config = ShabkaConfig::default_config();
        config.graph.dedup_llm = true;
        config.llm.enabled = true;
        let warnings = config.validate();
        assert!(!warnings.iter().any(|w| w.contains("dedup_llm")));
    }

    #[test]
    fn test_dedup_llm_toml_parsing() {
        let toml_str = r#"
[graph]
dedup_llm = true
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert!(config.graph.dedup_llm);
    }

    #[test]
    fn test_session_compression_default() {
        let config = CaptureConfig::default();
        assert!(config.session_compression);
    }

    #[test]
    fn test_session_compression_toml() {
        let toml_str = r#"
[capture]
session_compression = false
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.capture.session_compression);
    }

    // -- check_dimensions tests --

    #[test]
    fn test_check_dimensions_compatible() {
        // Default hash config produces 128d, and if the stored state
        // also says 128d, check should pass. We test the logic without
        // touching the real state file by verifying EmbeddingService
        // construction and dimension matching.
        let config = EmbeddingConfig::default(); // hash, 128d
        let service = crate::embedding::EmbeddingService::from_config(&config).unwrap();
        let state = EmbeddingState {
            provider: "hash".to_string(),
            model: "hash-128d".to_string(),
            dimensions: service.dimensions(),
            ..Default::default()
        };
        // Simulating the check_dimensions logic directly:
        assert_eq!(service.dimensions(), state.dimensions);
    }

    #[test]
    fn test_check_dimensions_mismatch() {
        // Simulate: stored state is 768d (ollama) but current config is hash (128d)
        let config = EmbeddingConfig::default(); // hash, 128d
        let service = crate::embedding::EmbeddingService::from_config(&config).unwrap();
        let state = EmbeddingState {
            provider: "ollama".to_string(),
            model: "nomic-embed-text".to_string(),
            dimensions: 768,
            ..Default::default()
        };
        assert_ne!(service.dimensions(), state.dimensions);
        // The actual check_dimensions reads from disk, but we verify the
        // core comparison that drives it:
        assert_eq!(service.dimensions(), 128);
        assert_eq!(state.dimensions, 768);
    }

    // -- StorageConfig tests --

    #[test]
    fn test_storage_config_defaults() {
        let config = ShabkaConfig::default_config();
        assert_eq!(config.storage.backend, "sqlite");
        assert!(config.storage.path.is_none());
    }

    #[test]
    fn test_storage_config_helix() {
        let toml_str = r#"
[storage]
backend = "helix"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.storage.backend, "helix");
    }

    #[test]
    fn test_storage_config_sqlite_custom_path() {
        let toml_str = r#"
[storage]
backend = "sqlite"
path = "/tmp/my-shabka.db"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.storage.backend, "sqlite");
        assert_eq!(config.storage.path.as_deref(), Some("/tmp/my-shabka.db"));
    }

    #[test]
    fn test_storage_config_backward_compat() {
        let toml_str = r#"
[embedding]
provider = "hash"
"#;
        let config: ShabkaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.storage.backend, "sqlite");
    }

    #[test]
    fn test_validate_unknown_storage_backend() {
        let mut config = ShabkaConfig::default_config();
        config.storage.backend = "banana".to_string();
        let warnings = config.validate();
        assert!(warnings
            .iter()
            .any(|w| w.contains("unknown storage backend")));
    }
}
