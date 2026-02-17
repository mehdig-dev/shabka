# Architecture

[← Back to README](../README.md)

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
                                │  - tokens   │
                                │  - context_pack│
                                │  - retry    │
                                └─────────────┘
```

## Workspace Crates

| Crate | Purpose |
|-------|---------|
| `shabka-core` | Data model, storage, embeddings, ranking, sharing, graph intelligence, history audit trail, smart dedup, PII scrubbing, LLM service, auto-tagging, quality assessment, memory consolidation, token estimation, context packs, retry logic |
| `shabka-mcp` | MCP server (12 tools for LLM integration) |
| `shabka-hooks` | Auto-capture + auto-relate from Claude Code sessions |
| `shabka-web` | Web dashboard (CRUD, search, graph visualization, REST API, analytics) |
| `shabka-cli` | CLI tool (search, get, chain, prune, history, status, export, import, init, reembed, consolidate, context-pack) |

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
    │   │   ├── tokens.rs   # Token estimation (byte-length / 4 heuristic)
    │   │   ├── context_pack.rs # Context pack builder + markdown formatter
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
