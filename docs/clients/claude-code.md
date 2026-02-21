# Shabka + Claude Code

Claude Code uses **stdio** transport — the simplest setup.

## Install

```bash
cargo install shabka-mcp
# or
curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh
```

## Register

```bash
claude mcp add shabka shabka-mcp
```

## Verify

Open a new Claude Code session and try:

- *"Search for authentication"*
- *"Save a memory about how our auth system works"*
- *"What do you remember?"*

All 15 Shabka tools are now available.

## Auto-Capture (optional)

Install the hooks for zero-effort memory capture:

```bash
shabka init
```

This adds Claude Code hooks that automatically capture decisions, patterns, and fixes during your sessions.

## Troubleshooting

| Problem | Fix |
|---------|-----|
| "shabka-mcp not found" | Ensure `~/.cargo/bin` or `~/.shabka/bin` is in your PATH |
| "No memories found" | Run `shabka demo` to seed sample data, then try again |
| Tools not showing | Restart Claude Code after `claude mcp add` |
