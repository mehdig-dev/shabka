# Configuration

Shabka uses layered TOML configuration: global (`~/.config/shabka/config.toml`), project (`.shabka/config.toml`), and local (`.shabka/config.local.toml`, gitignored).

```toml
[embedding]
provider = "ollama"           # hash, ollama, openai, gemini, local
model = "nomic-embed-text"

[graph]
similarity_threshold = 0.6    # Min similarity for auto-relate
max_relations = 3             # Max auto-relations per save
max_chain_depth = 5           # Default chain traversal depth
stale_days = 90               # Days before marking memory as stale
dedup_enabled = true
dedup_skip_threshold = 0.95   # Skip saving near-duplicates
dedup_update_threshold = 0.85 # Supersede similar memories

[history]
enabled = true
max_events = 10000

[scrub]
enabled = true
emails = true                 # Redact email addresses
api_keys = true               # Redact API keys / bearer tokens
ip_addresses = true           # Redact IPs (preserves 127.0.0.1, 192.168.*)
file_paths = true             # Redact /home/user/... paths
custom_patterns = []          # Additional regex patterns
replacement = "[REDACTED]"

[llm]
enabled = false               # Enable LLM features (session compression, consolidation, auto-tagging)
provider = "ollama"           # ollama, openai, gemini
model = "llama3.2"
max_tokens = 2048

[consolidate]
min_cluster_size = 3          # Min memories to form a cluster
similarity_threshold = 0.8    # Min similarity within cluster
max_cluster_size = 10         # Max memories per cluster
min_age_days = 7              # Only consolidate memories older than this

[capture]
session_compression = true    # Compress session events into memories at Stop
auto_tag = false              # LLM-powered auto-tagging (requires [llm] enabled)

[sharing]
user_id = "alice"

[privacy]
default_level = "private"     # public, team, private
```

## Embedding Providers

| Provider | Model | Dimensions | Notes |
|----------|-------|-----------|-------|
| `hash` | hash-128d | 128 | Default. Deterministic, no semantic search. For testing. |
| `ollama` | nomic-embed-text | 768 | Local, no API key. Needs Ollama running. |
| `openai` | text-embedding-3-small | 1536 | Needs `OPENAI_API_KEY`. Supports custom `base_url`. |
| `gemini` | text-embedding-004 | 768 | Needs `GEMINI_API_KEY`. |
| `local` | bge-small-en-v1.5 | 384 | Needs `embed-local` feature. Fails on WSL2. |
