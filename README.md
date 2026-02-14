# Shabka

A shared LLM memory system. Save, search, and connect knowledge across AI coding sessions.

Shabka gives LLMs persistent memory through an MCP server backed by [HelixDB](https://github.com/HelixDB/helix-db) (a graph-vector database). Memories are stored as nodes with vector embeddings for semantic search, connected by typed edges for relationship-aware retrieval.

## Architecture

```
┌─────────────┐     stdio      ┌─────────────┐     HTTP      ┌─────────────┐
│  Claude Code │◄──────────────►│  shabka-mcp │◄─────────────►│   HelixDB   │
│  (or any MCP │                │  (MCP server)│               │  port 6969  │
│   client)    │                └─────────────┘               └─────────────┘
└─────────────┘                       │
      │                               │ uses
      │ hooks                   ┌─────────────┐
      ▼                         │ shabka-core │        ┌─────────────┐
┌─────────────┐                 │  - model    │        │  shabka-web │
│ shabka-hooks│─────────────────│  - storage  │◄───────│  port 37737 │
│ (auto-capture)                │  - embedding│        │  (dashboard)│
└─────────────┘                 │  - ranking  │        └─────────────┘
                                │  - sharing  │
                                │  - graph    │        ┌─────────────┐
                                │  - history  │◄───────│  shabka-cli │
                                │  - dedup    │        │  (CLI tool) │
                                │  - scrub    │        └─────────────┘
                                │  - llm      │
                                │  - auto_tag │
                                │  - assess   │
                                │  - consolidate│
                                │  - retry    │
                                └─────────────┘
```

**Workspace crates:**

| Crate | Purpose |
|-------|---------|
| `shabka-core` | Data model, storage, embeddings, ranking, sharing, graph intelligence, history audit trail, smart dedup, PII scrubbing, LLM service, auto-tagging, quality assessment, memory consolidation, retry logic |
| `shabka-mcp` | MCP server (12 tools for LLM integration) |
| `shabka-hooks` | Auto-capture + auto-relate from Claude Code sessions |
| `shabka-web` | Web dashboard (CRUD, search, graph visualization, REST API, analytics) |
| `shabka-cli` | CLI tool (search, get, chain, prune, history, status, export, import, init, reembed, consolidate) |

## MCP Tools

Shabka exposes 12 tools via the MCP protocol:

| Tool | Description |
|------|-------------|
| `search` | Semantic + keyword hybrid search across memories |
| `get_memories` | Retrieve full memory details by ID |
| `timeline` | Chronological view with optional date/session filters |
| `save_memory` | Create a new memory with auto-embedding, smart dedup, and auto-relate |
| `update_memory` | Modify title, content, tags, importance, status |
| `delete_memory` | Permanently remove a memory |
| `relate_memories` | Link two memories (caused_by, fixes, supersedes, related, contradicts) |
| `follow_chain` | BFS traversal along typed edges (debugging narratives, version history) |
| `reembed` | Re-embed memories with current provider (incremental or forced) |
| `history` | View audit trail of memory mutations |
| `assess` | Memory quality scorecard (0-100 score, issue counts, top issues) |
| `consolidate` | Merge clusters of similar memories using LLM |

**Retrieval pattern:** Start with `search` (compact index, ~50-100 tokens each), drill into `get_memories` for full content, use `timeline` for chronological context.

**Smart dedup:** When saving, Shabka checks for near-duplicates via embedding similarity. Exact matches (>=0.95) are skipped, near-matches (>=0.85) supersede the old memory, and new content is auto-related to similar existing memories.

## Quick Start

### Prerequisites

- Rust 1.80+
- Docker
- [just](https://github.com/casey/just) (optional, for task automation)

### 1. Install HelixDB CLI

```bash
# Build from source (required on Ubuntu 20.04 / WSL2 due to OpenSSL 3 dependency)
cargo install --git https://github.com/HelixDB/helix-db helix-cli

# Or use the installer (requires OpenSSL 3 / Ubuntu 22.04+)
curl -sSL https://install.helix-db.com | bash
```

### 2. Start HelixDB

```bash
just db
# or manually:
cd helix && helix push dev
```

This compiles the HQL queries, builds a Docker image, and starts HelixDB on port 6969.

**Dashboard** (optional): `helix dashboard start dev` then open http://localhost:3000

### 3. Register the MCP Server

```bash
claude mcp add shabka -- cargo run --manifest-path /path/to/shabka/Cargo.toml -p shabka-mcp --no-default-features
```

### 4. Use It

Open a new Claude Code session. The 12 Shabka tools will be available. Try:

- "Save a memory about how our auth system works"
- "Search for authentication"
- "Link these two related memories"

## CLI

Install the CLI with `just cli-install` (or `cargo install --path crates/shabka-cli --no-default-features`).

```bash
shabka search <query>         # Semantic + keyword hybrid search
    --kind <kind>             # Filter by kind (observation, decision, pattern, etc.)
    --limit <n>               # Max results (default 10)
    --tag <tag>               # Filter by tag
    --json                    # JSON output

shabka get <memory-id>        # View full memory details
                              # Supports short 8-char prefix (e.g. shabka get a1b2c3d4)
    --json                    # JSON output

shabka chain <memory-id>      # Follow relation chains from a memory
    --relation <type>         # Filter by relation type (can repeat)
    --depth <n>               # Max traversal depth (default from config)
    --json                    # JSON output

shabka prune                  # Archive stale memories
    --days <n>                # Inactivity threshold (default from config)
    --dry-run                 # Preview without changes
    --decay-importance        # Also reduce importance of stale memories

shabka history                # Show recent audit events (with field change details)
    <memory-id>               # Show history for a specific memory
    --limit <n>               # Max events (default 20)
    --json                    # JSON output

shabka status                 # HelixDB health, memory count, embedding info
shabka init                   # Create .shabka/config.toml scaffold
    --provider <name>         # Pre-configure embedding provider (hash, ollama, openai, gemini)
    --check                   # Check prerequisites (Ollama, API keys, HelixDB) without creating files

shabka export -o file.json    # Export all memories + relations
    --privacy <level>         # Filter by privacy threshold (default: private)
    --scrub                   # Redact PII (emails, API keys, IPs, file paths)
    --scrub-report            # Scan for PII without exporting

shabka import file.json       # Re-embed and import memories

shabka reembed                # Re-embed memories with current provider
    --batch-size <n>          # Batch size (default 10)
    --dry-run                 # Preview without changes
    --force                   # Force full re-embed, skip incremental logic

shabka consolidate            # Merge clusters of similar memories (requires LLM)
    --dry-run                 # Preview clusters without merging
    --min-cluster <n>         # Min cluster size (default from config)
    --min-age <n>             # Min memory age in days (default from config)
    --json                    # JSON output
```

## Web Dashboard

```bash
just web   # Start on http://localhost:37737
```

Features:
- **Memory list** — Browse, filter by kind/project, bulk archive/delete, pagination
- **Memory detail** — Markdown rendering, relations, similar memories, audit history, chain explorer graph
- **Create/edit** — Kind descriptions, markdown hints, char counter, project ID field, styled sliders
- **Search** — Semantic + keyword search with ranked results and query term highlighting
- **Graph** — Interactive knowledge graph visualization (Cytoscape.js)
- **Analytics** — Memory distribution charts, creation trends, quality score gauge, contradiction count
- **Breadcrumb navigation** — Contextual breadcrumbs on all pages
- **Styled modals** — Confirmation dialogs and toast notifications replace browser alerts
- **Dark/light theme** — Toggle in navbar, persists across sessions
- **Keyboard shortcuts** — `Ctrl+K` or `/` to focus search
- **REST API** — Full JSON API at `/api/v1/` for external integrations

### REST API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/memories` | POST | Create memory (dedup-aware) |
| `/api/v1/memories` | GET | List memories (`?kind=&limit=&status=`) |
| `/api/v1/memories/{id}` | GET | Get memory with relations |
| `/api/v1/memories/{id}` | PUT | Update memory |
| `/api/v1/memories/{id}` | DELETE | Delete memory |
| `/api/v1/memories/{id}/relate` | POST | Add relation |
| `/api/v1/memories/{id}/relations` | GET | Get relations |
| `/api/v1/memories/{id}/history` | GET | Get audit history |
| `/api/v1/search` | GET | Search (`?q=&kind=&limit=&tag=`) |
| `/api/v1/timeline` | GET | Timeline (`?limit=&session_id=`) |
| `/api/v1/stats` | GET | Analytics data |
| `/api/v1/memories/bulk/archive` | POST | Bulk archive by IDs |
| `/api/v1/memories/bulk/delete` | POST | Bulk delete by IDs |

## Auto-Capture Hooks

Shabka can automatically capture memories from Claude Code sessions via hooks. Install and register:

```bash
just hooks-install    # Build and copy to ~/.local/bin
just hooks-register   # Print registration instructions for .claude/settings.json
```

The hooks capture tool use events (file edits, command runs, errors) and auto-relate new memories to semantically similar existing ones.

## Configuration

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

### Embedding Providers

| Provider | Model | Dimensions | Notes |
|----------|-------|-----------|-------|
| `hash` | hash-128d | 128 | Default. Deterministic, no semantic search. For testing. |
| `ollama` | nomic-embed-text | 768 | Local, no API key. Needs Ollama running. |
| `openai` | text-embedding-3-small | 1536 | Needs `OPENAI_API_KEY`. Supports custom `base_url`. |
| `gemini` | text-embedding-004 | 768 | Needs `GEMINI_API_KEY`. |
| `local` | bge-small-en-v1.5 | 384 | Needs `embed-local` feature. Fails on WSL2. |

## Development

```bash
just build              # Build all crates
just test               # Run unit tests (243 tests)
just check              # Clippy + tests
just fmt                # Format code

just test-helix         # Integration tests: HelixDB (requires: just db)
just test-ollama        # Integration tests: Ollama (requires: Ollama + HelixDB)
just test-integration   # All integration tests
just test-all           # Unit + integration tests

just db                 # Start HelixDB
just db-stop            # Stop HelixDB
just db-logs            # View HelixDB logs

just mcp                # Run the MCP server
just mcp-register       # Print the claude mcp add command
just web                # Run the web dashboard
just cli-install        # Build and install the CLI
```

### Testing

- **Unit tests (243 — 206 core + 37 hooks):** Run with `just test`. No external services needed.
- **Integration tests (12):** Run with `just test-integration`. Requires HelixDB (`just db`); Ollama tests additionally need Ollama with `nomic-embed-text` pulled.
- Integration tests use `#[ignore]` so they're skipped by default and won't break CI without services running.

### Resetting HelixDB

To wipe all data and start fresh:

```bash
helix stop dev
sudo rm -rf helix/.helix/.volumes/dev
cd helix && helix push dev
```

## Project Structure

```
shabka/
├── Cargo.toml              # Workspace root
├── Justfile                # Dev task automation
├── helix/
│   ├── helix.toml          # HelixDB project config
│   ├── schema.hx           # Node/Edge/Vector definitions (HQL v2)
│   └── queries.hx          # Pre-compiled queries (HQL v2)
└── crates/
    ├── shabka-core/        # Core library
    │   ├── src/
    │   │   ├── model/      # Memory, Session, Relation types
    │   │   ├── storage/    # HelixDB backend (StorageBackend trait)
    │   │   ├── embedding/  # Hash, OpenAI, Ollama, Gemini providers
    │   │   ├── config/     # Layered TOML config loading
    │   │   ├── ranking.rs  # Fusion ranking (similarity + keyword + recency + importance + graph)
    │   │   ├── sharing.rs  # Privacy enforcement, visibility filtering
    │   │   ├── graph.rs    # Semantic auto-relate, chain traversal
    │   │   ├── decay.rs    # Staleness analysis, importance decay
    │   │   ├── dedup.rs    # Smart duplicate detection
    │   │   ├── history.rs  # JSONL audit trail
    │   │   ├── scrub.rs    # PII detection and redaction
    │   │   ├── llm.rs      # LLM service (Ollama, OpenAI, Gemini)
    │   │   ├── auto_tag.rs # LLM-powered auto-tagging
    │   │   ├── assess.rs   # Memory quality assessment
    │   │   ├── consolidate.rs # Memory cluster consolidation
    │   │   └── retry.rs    # Exponential backoff retry logic
    │   └── tests/          # Integration tests (HelixDB + Ollama)
    ├── shabka-mcp/         # MCP server binary (rmcp 0.14)
    ├── shabka-hooks/       # Auto-capture + auto-relate
    ├── shabka-web/         # Web dashboard (Axum + Askama)
    │   └── src/routes/
    │       ├── memories.rs # CRUD + list with pagination
    │       ├── search.rs   # Semantic search page
    │       ├── graph.rs    # Graph visualization + chain API
    │       ├── analytics.rs# Analytics dashboard
    │       └── api.rs      # REST API (/api/v1/)
    └── shabka-cli/         # CLI tool (clap)
```

## License

MIT OR Apache-2.0
