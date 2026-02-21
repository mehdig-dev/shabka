# Shabka + Cline (VS Code)

Cline supports both **stdio** and **HTTP** transports.

## Install

```bash
cargo install shabka-mcp
# or
curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh
```

## Option A: Stdio (simpler)

In VS Code settings (`settings.json`), add:

```json
{
  "cline.mcpServers": {
    "shabka": {
      "command": "shabka-mcp",
      "args": []
    }
  }
}
```

## Option B: HTTP

Start the server:

```bash
shabka-mcp --http 8080
```

In VS Code settings:

```json
{
  "cline.mcpServers": {
    "shabka": {
      "url": "http://localhost:8080/mcp"
    }
  }
}
```

## Verify

In Cline's chat: *"Search for memories about authentication"*

**Alternative:** If you're already running `shabka-web` (the web dashboard), the MCP endpoint is also available at `http://localhost:37737/mcp` — no separate server needed.

## Troubleshooting

| Problem | Fix |
|---------|-----|
| "shabka-mcp not found" | Use full path: `"command": "/home/user/.cargo/bin/shabka-mcp"` |
| Connection refused (HTTP) | Make sure `shabka-mcp --http 8080` is running |
| Tools not appearing | Reload VS Code window after config change |
