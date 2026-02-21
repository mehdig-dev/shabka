# Shabka — MCP Registry Metadata

Use this information when submitting to MCP registries (mcp.so, Smithery, etc.).

## Basic Info

- **Name:** Shabka
- **Tagline:** Persistent memory for LLM coding agents
- **Description:** 15-tool MCP server that gives AI coding agents persistent memory across sessions. Save decisions, track errors and fixes, search semantically, and build a knowledge graph that grows with your codebase.
- **Repository:** https://github.com/mehdig-dev/shabka
- **License:** MIT OR Apache-2.0
- **Categories:** memory, knowledge-management, developer-tools

## Installation

### Pre-built binary (Linux/macOS)

```bash
curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh
```

### From crates.io

```bash
cargo install shabka-mcp
```

### Register with Claude Code

```bash
claude mcp add shabka -- shabka-mcp
```

## Tools (15)

| Tool | Description |
|------|-------------|
| `search` | Semantic + keyword search across all memories with ranking |
| `get_memories` | Retrieve one or more memories by ID |
| `get_context` | Token-budgeted context pack from project memories |
| `timeline` | Chronological memory feed with optional filters |
| `save_memory` | Create a new memory with kind, tags, and importance |
| `update_memory` | Modify an existing memory's content or metadata |
| `delete_memory` | Permanently remove a memory |
| `relate_memories` | Create typed relations (fixes, caused_by, related, supersedes, contradicts) |
| `reembed` | Re-embed memories after changing embedding provider |
| `follow_chain` | BFS traversal of relation chains from a starting memory |
| `history` | Audit trail of all changes to a memory |
| `assess` | Analyze memory quality and find issues |
| `consolidate` | Merge similar memory clusters into summaries (requires LLM) |
| `verify_memory` | Set verification status (verified, disputed, outdated) |
| `save_session_summary` | Batch-save multiple session learnings in one call |

## Key Features

- **Zero-config start** — SQLite storage, works immediately with `shabka demo`
- **9 memory kinds** — Observation, Decision, Pattern, Error, Fix, Preference, Fact, Lesson, Todo
- **Knowledge graph** — typed relations between memories, chain traversal
- **Multi-provider embeddings** — Hash (default), Ollama, OpenAI, Gemini, Cohere
- **Trust scoring** — 4-factor trust formula with verification status
- **PII scrubbing** — regex-based redaction for export
- **Interactive TUI** — `shabka tui` for browsing and searching
