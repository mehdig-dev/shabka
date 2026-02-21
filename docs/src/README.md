# Shabka

**Persistent memory for LLM coding agents.**
Save, search, and connect knowledge across AI sessions.

LLMs forget everything between sessions. Shabka fixes that.

Shabka is an MCP server that gives AI coding assistants persistent, searchable memory. It uses **SQLite by default** (zero setup) with optional [HelixDB](https://github.com/HelixDB/helix-db) for graph-vector features. Memories are stored with vector embeddings for semantic search and connected by typed relations for relationship-aware retrieval. **15 MCP tools**, a CLI, and a web dashboard included.

## Why Shabka?

- **Persistent memory** — Decisions, patterns, and lessons survive across sessions. No more re-explaining your codebase.
- **Team knowledge sharing** — Share context across team members with privacy controls (public, team, private).
- **Zero-effort capture** — Auto-captures insights from Claude Code sessions via hooks. No manual saving needed.
- **Works everywhere** — MCP server for Claude/Cursor, plus a CLI and web dashboard for browsing and managing memories.

## Works With

| Client | Transport | Guide |
|--------|-----------|-------|
| Claude Code | stdio | [Setup](clients/claude-code.md) |
| Cursor | HTTP | [Setup](clients/cursor.md) |
| Windsurf | HTTP | [Setup](clients/windsurf.md) |
| Cline | stdio or HTTP | [Setup](clients/cline.md) |
| Continue | stdio | [Setup](clients/continue.md) |

Any MCP-capable client can connect via `shabka-mcp --http 8080`.

## Get Started

Head to the [Installation](getting-started/installation.md) page to install Shabka, then follow the [Quick Start](getting-started/quickstart.md) guide.
