# Shabka + Continue

Continue supports **stdio** transport.

## Install

```bash
cargo install shabka-mcp
# or
curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh
```

## Configure

Add to `~/.continue/config.json` (or `.continue/config.json` in your project):

```json
{
  "mcpServers": [
    {
      "name": "shabka",
      "command": "shabka-mcp",
      "args": []
    }
  ]
}
```

## Verify

In Continue's chat: *"Search for memories"*

## Troubleshooting

| Problem | Fix |
|---------|-----|
| "shabka-mcp not found" | Use full path to the binary |
| Tools not appearing | Restart VS Code after config change |
