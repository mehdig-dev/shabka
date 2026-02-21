# Development

## Prerequisites

- Rust 1.80+
- Docker
- [just](https://github.com/casey/just) (optional, for task automation)

## Dev Commands

```bash
just build              # Build all crates
just test               # Run unit tests (334 tests)
just check              # Clippy + tests
just fmt                # Format code

just test-helix         # Integration tests: HelixDB (requires: just db)
just test-ollama        # Integration tests: Ollama (requires: Ollama + HelixDB)
just test-integration   # All integration tests
just test-all           # Unit + integration tests

just db                 # Start HelixDB
just db-stop            # Stop HelixDB
just db-logs            # View HelixDB logs

just mcp                # Run the MCP server
just mcp-register       # Print the claude mcp add command
just web                # Run the web dashboard
just cli-install        # Build and install the CLI
```

## Testing

- **Unit tests (334 — 290 core + 7 MCP + 37 hooks):** Run with `just test`. No external services needed.
- **Integration tests (19):** Run with `just test-integration`. Requires HelixDB (`just db`); Ollama tests additionally need Ollama with `nomic-embed-text` pulled.
- Integration tests use `#[ignore]` so they're skipped by default and won't break CI without services running.

## Resetting HelixDB

To wipe all data and start fresh:

```bash
helix stop dev
sudo rm -rf helix/.helix/.volumes/dev
cd helix && helix push dev
```
