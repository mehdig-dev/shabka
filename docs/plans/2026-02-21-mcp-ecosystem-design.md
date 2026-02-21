# v0.5.0 MCP Ecosystem Design

## Goal

Make Shabka work with every MCP-capable coding agent — not just Claude Code. Add HTTP transport, multi-client setup guides, and a universal auto-capture tool.

## Workstreams

### 1. Streamable HTTP Transport

rmcp 0.16 provides `StreamableHttpService` — a Tower `Service` implementing the MCP Streamable HTTP protocol (POST requests with SSE responses). This integrates directly with Axum.

**Two modes, one implementation:**

- **`shabka-mcp --http [port]`** — standalone HTTP server (default port 8080). Same binary, transport selected by flag. Stdio remains the default for backward compatibility.
- **`/mcp` route in shabka-web** — mounts `StreamableHttpService` as a nested Axum service. Single process serves both the web dashboard and MCP protocol.

Both wrap the same `ShabkaServer` — the only difference is the transport layer.

**Feature flag:** `transport-streamable-http-server` on rmcp. Add to `shabka-mcp/Cargo.toml`. The `shabka-web` integration needs rmcp as a dependency too (or the HTTP service lives in a shared module).

**Architecture:**

```
shabka-mcp binary:
  --stdio (default)  → rmcp::transport::stdio → ShabkaServer
  --http [port]      → StreamableHttpService<ShabkaServer> → axum::Router → listen

shabka-web binary:
  /mcp               → StreamableHttpService<ShabkaServer> (mounted as Axum service)
  /api/v1/*          → existing REST routes
  /*                  → existing web dashboard
```

### 2. Multi-Client Setup Guides

Create `docs/clients/` with one file per client:

- `claude-code.md` — `claude mcp add shabka shabka-mcp` (stdio)
- `cursor.md` — `~/.cursor/mcp.json` with HTTP URL
- `windsurf.md` — Windsurf MCP config with HTTP URL
- `cline.md` — VS Code settings.json MCP config
- `continue.md` — Continue config.json MCP config

Each guide follows the same structure:
1. Install shabka-mcp
2. Start the server (`shabka-mcp --http` or stdio depending on client)
3. Add config snippet (copy-paste ready)
4. Verify: "Ask your agent to search for memories"
5. Troubleshooting (common errors)

### 3. Universal Auto-Capture: `save_session_summary` Tool

A 15th MCP tool that any agent can call to persist what it learned during a conversation.

**Input:**
```json
{
  "memories": [
    {
      "title": "Use JWT with short-lived tokens for API auth",
      "content": "Decided on JWT over sessions because...",
      "kind": "decision",
      "tags": ["auth", "api"],
      "importance": 0.8
    }
  ],
  "session_context": "Refactoring the authentication system"
}
```

**Behavior:**
- Validates and saves each memory (same pipeline as `save_memory`: embed, dedup check, auto-relate)
- Groups them under a session ID
- Returns summary: how many saved, how many deduped, session ID

**Why a tool instead of hooks:**
- Works with any MCP client — no client-specific integration
- The agent decides what's worth remembering (better signal than intercepting every tool call)
- Simpler to maintain than per-client hook adapters

The existing `save_memory` tool already does most of this for single memories. `save_session_summary` is a batch version with session grouping and dedup.

## Success Criteria

- `shabka-mcp --http` starts an HTTP server that responds to MCP Streamable HTTP protocol
- Cursor and at least one other client (Cline or Windsurf) can connect and use all 15 tools
- `save_session_summary` works from any MCP client
- Setup guides tested and published in docs/

## Non-Goals

- OAuth/authentication on the HTTP transport (local-first for now)
- Client-specific hook adapters (the universal tool replaces this need)
- WebSocket transport (Streamable HTTP with SSE is the MCP standard)
