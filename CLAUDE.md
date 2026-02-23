# Shabka — Project Instructions

## Build & Test

- Always run `cargo check` and `cargo test -p shabka-core --no-default-features` after modifying Rust code before considering a task complete.
- If clippy is relevant: `cargo clippy --workspace --no-default-features -- -D warnings`
- Use `just check` as the single command for full validation (clippy + tests).
- Integration tests require HelixDB (`just db`) and/or Ollama. Run with `just test-integration`.

## Approach & Planning

- Before implementing, briefly state your planned approach and ask for confirmation if the task is ambiguous or has multiple valid strategies.
- Never assume domain-specific terminology — ask for clarification on terms that could have multiple meanings.
- Check actual dependency versions and API surfaces before writing code that depends on them (e.g. read the crate source, don't guess import paths).

## System Constraints

- This runs on **Ubuntu 20.04 WSL2** — glibc 2.31, OpenSSL 1.1, no systemd.
- Do NOT run `sudo` commands via Bash. Instead, print the exact command for the user to run manually.
- Do NOT attempt interactive CLI wizards or prompts. Print the equivalent non-interactive command or defer to the user with clear instructions.
- The `fastembed` crate (ONNX) does not link on this system. Always use `--no-default-features` to exclude it.

## Dev Commands

- `just build` — build all crates
- `just test` — run unit tests (462 tests)
- `just check` — clippy + tests
- `just test-helix` — HelixDB integration tests (requires: `just db`)
- `just test-ollama` — Ollama integration tests (requires: Ollama + HelixDB)
- `just test-integration` — all integration tests
- `just test-all` — unit + integration tests
- `just db` — start HelixDB (port 6969)
- `just mcp` — run MCP server
- `just web` — run web dashboard (port 37737)
- `just cli-install` — build and install the CLI

## Key Conventions

- HelixDB field names: `memory_id`, `session_id` (not `id` — it's reserved)
- Default embedding provider: `hash` (128d deterministic, for testing pipeline)
- Config defaults are hardcoded in `shabka-core/src/config/mod.rs`, not loaded from `config/default.toml`
- Integration tests use `#[ignore]` + runtime service guards — skipped by `cargo test`, run with `--ignored`
- Test memories use UUID-tagged titles to prevent collisions between test runs

## Workspace Crates

| Crate          | Purpose                                                                                                                  |
| -------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `shabka-core`  | Data model, storage (HelixDB), embeddings, ranking, sharing, graph intelligence, decay/pruning, history audit trail, smart dedup, PII scrubbing, trust scoring |
| `shabka-mcp`   | MCP server — 15 tools (search, get_memories, get_context, timeline, save/update/delete_memory, relate_memories, reembed, follow_chain, history, assess, consolidate, verify_memory, save_session_summary) |
| `shabka-hooks` | Auto-capture from Claude Code sessions via hooks (PostToolUse, Stop)                                                            |
| `shabka-web`   | Web dashboard — Axum + Askama, graph visualization, CRUD, REST API (`/api/v1/`), analytics dashboard                            |
| `shabka-cli`   | CLI — search, get, list, delete, chain, prune, verify, history, status, export, import, init, reembed, consolidate, context-pack, demo, tui |

## Embedding Providers

| Provider | Model                  | Dimensions | Notes                                               |
| -------- | ---------------------- | ---------- | --------------------------------------------------- |
| `hash`   | hash-128d              | 128        | Default. Deterministic, no semantic search.         |
| `ollama` | nomic-embed-text       | 768        | Local, no API key. Uses OpenAI-compatible API.      |
| `openai` | text-embedding-3-small | 1536       | Needs `OPENAI_API_KEY`. Supports custom `base_url`. |
| `gemini` | text-embedding-004     | 768        | Needs `GEMINI_API_KEY`.                             |
| `local`  | bge-small-en-v1.5      | 384        | Needs `embed-local` feature. Fails on WSL2.         |

