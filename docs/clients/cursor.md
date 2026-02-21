# Shabka + Cursor

Cursor uses **HTTP** transport via MCP config file.

## Install

```bash
cargo install shabka-mcp
# or
curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh
```

## Start the Server

```bash
shabka-mcp --http 8080
```

Keep this running in a terminal (or run as a background service).

## Configure Cursor

Add to `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "shabka": {
      "url": "http://localhost:8080/mcp"
    }
  }
}
```

Restart Cursor after saving.

## Verify

In Cursor's chat, try:

- *"Search for authentication"*
- *"Save a memory about our API design"*

## Troubleshooting

| Problem | Fix |
|---------|-----|
| Connection refused | Make sure `shabka-mcp --http 8080` is running |
| No tools available | Check `~/.cursor/mcp.json` syntax, restart Cursor |
| "shabka-mcp not found" | Ensure `~/.cargo/bin` or `~/.shabka/bin` is in your PATH |
