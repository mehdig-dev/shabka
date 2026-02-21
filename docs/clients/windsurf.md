# Shabka + Windsurf

Windsurf uses **HTTP** transport.

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

## Configure Windsurf

Open Windsurf settings and add MCP server:

- **Name:** shabka
- **URL:** `http://localhost:8080/mcp`
- **Transport:** Streamable HTTP

Or add to your Windsurf MCP config file:

```json
{
  "mcpServers": {
    "shabka": {
      "serverUrl": "http://localhost:8080/mcp"
    }
  }
}
```

## Verify

Ask Windsurf: *"Search for recent memories"*

## Troubleshooting

| Problem | Fix |
|---------|-----|
| Connection refused | Make sure `shabka-mcp --http 8080` is running |
| No tools available | Restart Windsurf after config change |
