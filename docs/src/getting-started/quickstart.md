# Quick Start

After [installing](installation.md) Shabka, register the MCP server with your AI client and start using persistent memory.

## Register the MCP server

### Claude Code (stdio)

```bash
claude mcp add shabka shabka-mcp
```

### Cursor / Windsurf / Cline (HTTP)

Start the server in a terminal:

```bash
shabka-mcp --http 8080
```

Then point your client to `http://localhost:8080/mcp`. See the [client setup guides](../clients/claude-code.md) for detailed instructions per client.

## Try it

Open a new session with your AI client. All 15 Shabka tools are now available:

- *"Save a memory about how our auth system works"*
- *"Search for authentication"*
- *"What do you remember about the database schema?"*

That's it — SQLite storage works out of the box with zero configuration.

## Try the CLI in 30 seconds

```bash
shabka demo                    # Seed 12 sample memories
shabka tui                     # Browse interactively
shabka search "authentication" # Search from CLI
```

## Auto-capture (Claude Code)

Install hooks for zero-effort memory capture during coding sessions:

```bash
shabka init
```

This adds Claude Code hooks that automatically capture decisions, patterns, and fixes.

## Advanced: HelixDB backend

For graph-vector features (native vector search, graph traversals), switch to HelixDB:

1. Install HelixDB:
   ```bash
   cargo install --git https://github.com/HelixDB/helix-db helix-cli
   ```

2. Start the database:
   ```bash
   cd helix && helix push dev
   ```

3. Update config (`~/.config/shabka/config.toml`):
   ```toml
   [storage]
   backend = "helix"
   ```
