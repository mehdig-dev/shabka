<p align="center">
  <img src="shabka.png" alt="Shabka" width="200">
</p>

<h1 align="center">Shabka</h1>

<p align="center">Zero-config memory for AI coding agents.<br>Single Rust binary, no API keys, no Python, no infrastructure.</p>

<p align="center">
  <a href="https://github.com/mehdig-dev/shabka/actions"><img src="https://github.com/mehdig-dev/shabka/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/shabka-core"><img src="https://img.shields.io/crates/v/shabka-core.svg" alt="crates.io"></a>
  <a href="https://docs.rs/shabka-core"><img src="https://docs.rs/shabka-core/badge.svg" alt="docs.rs"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg" alt="License"></a>
  <a href="https://mehdig-dev.github.io/shabka/"><img src="https://img.shields.io/badge/docs-mdbook-blue" alt="Docs"></a>
</p>

LLMs forget everything between sessions. Shabka fixes that.

Shabka is an MCP server that gives AI coding assistants persistent, searchable memory backed by the [COALA memory architecture](https://arxiv.org/abs/2309.02427) — the same cognitive framework used in state-of-the-art language agent research. It ships as a single binary with SQLite (zero setup), 16 MCP tools, a CLI, and a web dashboard.

## Why Shabka?

- **Zero config** — `cargo install shabka-mcp && claude mcp add shabka shabka-mcp`. SQLite storage, hash embeddings, no API keys needed to start.
- **Privacy-first** — Everything stays local. No data leaves your machine. PII scrubbing and per-memory privacy levels (public, team, private).
- **COALA memory model** — Procedural, semantic, and episodic memory types with a dedicated `remember` tool for standing rules. Not just a key-value store.
- **16 MCP tools** — Search, save, relate, consolidate, verify, assess, and more. Knowledge graph with typed relations and trust scoring.

## COALA Memory Architecture

Shabka implements the three memory types from the [COALA framework](https://arxiv.org/abs/2309.02427) (Cognitive Architectures for Language Agents):

| Memory Type | What It Stores | Shabka Feature |
|-------------|----------------|----------------|
| **Procedural** | Rules, preferences, standing instructions | `remember` tool — *"always use snake_case in this project"* |
| **Semantic** | Facts, patterns, technical knowledge | `save_memory` tool — *"our auth uses JWT with RS256"* |
| **Episodic** | Session experiences, what happened when | `save_session_summary` — auto-captured at session end |

Most memory tools for LLMs are flat key-value stores. Shabka adds **typed relations** (supports, contradicts, extends, supersedes), **trust scoring** (verified/contested/unverified), **auto-consolidation** (merges related memories on a schedule), and **decay/pruning** (stale memories fade). See [Memory Types](https://mehdig-dev.github.io/shabka/concepts/memory-types.html) for details.

## How It Compares

| Feature | Shabka | Mem0 | Zep |
|---------|--------|------|-----|
| Zero-config setup | Single binary, SQLite | Python, Redis, API keys | Docker, PostgreSQL |
| Privacy / local-only | Everything local | Cloud-first | Self-hosted option |
| COALA memory model | Procedural + Semantic + Episodic | Flat memories | Sessions + facts |
| Knowledge graph | Typed relations, chain traversal | Graph memory (cloud) | Temporal knowledge graph |
| Trust scoring | Verified / contested / unverified | No | No |
| Auto-consolidation | LLM-powered merge on schedule | No | No |
| PII scrubbing | Built-in, configurable | No | No |
| Review mode | Pending approval before activation | No | No |
| MCP tools | 16 tools, stdio + HTTP | REST API | REST API |
| Language | Rust (single binary) | Python | Python + Go |

## What Gets Auto-Captured?

When you add Shabka's [hooks](docs/src/clients/claude-code.md) to Claude Code, it watches for tool calls and session endings:

- **Code edits** — file paths, languages, what changed and why
- **Terminal commands** — commands run and their outcomes
- **Session summaries** — what was accomplished, compressed at session end

All captures go through importance scoring (trivial edits are dropped), smart deduplication (similar memories merge), and optional **review mode** (memories stay `pending` until you approve them).

## Day 1 Workflow

```bash
# Install
curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh

# Register with Claude Code
claude mcp add shabka shabka-mcp

# Start coding — Shabka auto-captures as you work
# Next day, in a new session:
```

> **You:** *"What did we do with the auth system yesterday?"*
>
> **Claude:** searches Shabka, finds session summaries and saved memories
>
> **You:** *"Remember: always use parameterized queries in this project"*
>
> **Claude:** calls `remember` tool → saved as procedural memory, persists across all future sessions

## Embedding Providers

| Provider | Setup | Semantic Search | Best For |
|----------|-------|-----------------|----------|
| `hash` (default) | Nothing — works immediately | No (keyword only) | Getting started, CI/CD |
| `ollama` | [Install Ollama](https://ollama.com), pull `nomic-embed-text` | Yes | Local, no API key |
| `openai` | Set `OPENAI_API_KEY` | Yes | Best quality |
| `gemini` | Set `GEMINI_API_KEY` | Yes | Google ecosystem |

Start with `hash` to try the pipeline, then switch to `ollama` or `openai` when you want semantic search. See [Configuration](docs/src/getting-started/configuration.md).

## Screenshots

| Dashboard | Search | Detail |
|:-:|:-:|:-:|
| ![Dashboard](docs/src/screenshots/dashboard.png) | ![Search](docs/src/screenshots/search.png) | ![Detail](docs/src/screenshots/detail.png) |
| Memory list with kind filters | Filter by kind, tag, project | Markdown content, relations, metadata |

| Knowledge Graph | Analytics |
|:-:|:-:|
| ![Graph](docs/src/screenshots/graph.png) | ![Analytics](docs/src/screenshots/analytics.png) |
| Interactive graph visualization | Quality score, charts, stats |

## Quick Start

### Install

```bash
# Option A: Install script (recommended)
curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh

# Option B: Homebrew (macOS / Linux)
brew install mehdig-dev/tap/shabka

# Option C: From crates.io
cargo install shabka-cli shabka-mcp
```

### Register the MCP server

```bash
claude mcp add shabka shabka-mcp
```

### Try it

```bash
shabka demo                    # Seed 12 sample memories
shabka tui                     # Browse interactively
shabka search "authentication" # Search from CLI
```

Open a Claude Code session — 16 Shabka tools are now available:

- *"Save a memory about how our auth system works"*
- *"Remember: always run tests before committing"*
- *"What do you remember about the database schema?"*

## Works With

| Client | Transport | Guide |
|--------|-----------|-------|
| Claude Code | stdio | [Setup](docs/src/clients/claude-code.md) |
| Cursor | HTTP | [Setup](docs/src/clients/cursor.md) |
| Windsurf | HTTP | [Setup](docs/src/clients/windsurf.md) |
| Cline | stdio or HTTP | [Setup](docs/src/clients/cline.md) |
| Continue | stdio | [Setup](docs/src/clients/continue.md) |

Any MCP-capable client can connect via `shabka-mcp --http 8080`.

## CLI Highlights

```bash
shabka search "auth tokens"           # Semantic + keyword hybrid search
shabka get a1b2c3d4                   # View memory details (short ID prefix)
shabka chain a1b2c3d4 --depth 3      # Follow relation chains
shabka status                         # Storage health + memory count
shabka review --list                  # View pending memories
shabka consolidate                    # Merge related memories
shabka context-pack "project setup"   # Paste-ready context block for LLMs
```

See [CLI Reference](docs/src/guide/cli.md) for all commands.

## Web Dashboard

```bash
shabka-web   # http://localhost:37737
```

Browse, search, and manage memories with markdown rendering, graph visualization, analytics, and dark/light theme. See [Web Dashboard](docs/src/guide/web-dashboard.md).

## Documentation

| Document | Description |
|----------|-------------|
| [Configuration](docs/src/getting-started/configuration.md) | TOML config, embedding providers, storage backends |
| [Memory Types](https://mehdig-dev.github.io/shabka/concepts/memory-types.html) | COALA framework, procedural/semantic/episodic memory |
| [CLI Reference](docs/src/guide/cli.md) | All commands and flags |
| [Web Dashboard](docs/src/guide/web-dashboard.md) | Dashboard features, REST API |
| [API Reference](docs/src/guide/api.md) | 16 MCP tools, retrieval patterns |
| [Client Setup](docs/src/clients/) | Claude Code, Cursor, Windsurf, Cline, Continue |
| [Architecture](docs/src/reference/architecture.md) | System diagram, crate structure |

## License

MIT OR Apache-2.0
