# CLI

[← Back to README](../README.md)

Install the CLI with `just cli-install` (or `cargo install --path crates/shabka-cli --no-default-features`).

```bash
shabka search <query>         # Semantic + keyword hybrid search
    --kind <kind>             # Filter by kind (observation, decision, pattern, etc.)
    --limit <n>               # Max results (default 10)
    --tag <tag>               # Filter by tag
    --token-budget <n>        # Cap results to fit within estimated token budget
    --json                    # JSON output

shabka get <memory-id>        # View full memory details
                              # Supports short 8-char prefix (e.g. shabka get a1b2c3d4)
    --json                    # JSON output

shabka chain <memory-id>      # Follow relation chains from a memory
    --relation <type>         # Filter by relation type (can repeat)
    --depth <n>               # Max traversal depth (default from config)
    --json                    # JSON output

shabka prune                  # Archive stale memories
    --days <n>                # Inactivity threshold (default from config)
    --dry-run                 # Preview without changes
    --decay-importance        # Also reduce importance of stale memories

shabka history                # Show recent audit events (with field change details)
    <memory-id>               # Show history for a specific memory
    --limit <n>               # Max events (default 20)
    --json                    # JSON output

shabka status                 # HelixDB health, memory count, embedding info
shabka init                   # Create .shabka/config.toml scaffold
    --provider <name>         # Pre-configure embedding provider (hash, ollama, openai, gemini)
    --check                   # Check prerequisites (Ollama, API keys, HelixDB) without creating files

shabka export -o file.json    # Export all memories + relations
    --privacy <level>         # Filter by privacy threshold (default: private)
    --scrub                   # Redact PII (emails, API keys, IPs, file paths)
    --scrub-report            # Scan for PII without exporting

shabka import file.json       # Re-embed and import memories

shabka reembed                # Re-embed memories with current provider
    --batch-size <n>          # Batch size (default 10)
    --dry-run                 # Preview without changes
    --force                   # Force full re-embed, skip incremental logic

shabka consolidate            # Merge clusters of similar memories (requires LLM)
    --dry-run                 # Preview clusters without merging
    --min-cluster <n>         # Min cluster size (default from config)
    --min-age <n>             # Min memory age in days (default from config)
    --json                    # JSON output

shabka verify <memory-id>     # Set verification status on a memory
    --status <status>         # verified, disputed, outdated, unverified

shabka context-pack [query]   # Generate paste-ready context from project memories
    --tokens <n>              # Token budget (default 2000)
    --project <name>          # Filter by project
    --kind <kind>             # Filter by memory kind
    --tag <tag>               # Filter by tag
    --json                    # JSON output instead of markdown
    -o <file>                 # Write to file instead of stdout
```
