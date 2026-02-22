# Changelog

All notable changes to Shabka are documented here.

## [0.5.1] — 2026-02-22

Storage performance and cross-platform fixes.

- **sqlite-vec KNN search** — replaced brute-force cosine similarity with SIMD-accelerated KNN via the `sqlite-vec` extension.
- **sqlean extensions** — registered `fuzzy`, `stats`, and `crypto` extensions for future fuzzy matching and analytics queries.
- **Mixed-dimension handling** — `ensure_vec_table()` uses statistical mode to pick the dominant dimension, filters migration, and warns about mismatches.
- **Idempotent save_memory** — `INSERT OR REPLACE` for memories/embeddings; delete-then-insert for vec0 virtual table. Enables `shabka reembed` without errors.
- **ARM64 cross-compilation fix** — use `std::ffi::c_char`/`c_int` in extension transmute (ARM Linux defines `c_char` as `u8`, not `i8`).

## [0.5.0] — 2026-02-21

MCP Ecosystem — wider client compatibility.

- **Streamable HTTP transport** — `shabka-mcp --http [port]` for non-stdio clients (Cursor, Windsurf, etc.).
- **MCP endpoint in web dashboard** — `/mcp` route serves the same 15 tools alongside the web UI.
- **`save_session_summary` tool** — 15th MCP tool for batch-saving session learnings from any agent.
- **Multi-client setup guides** — docs for Claude Code, Cursor, Windsurf, Cline, and Continue.

## [0.4.0] — 2026-02-21

Launch readiness — distribution and onboarding.

- **Interactive TUI** — `shabka tui` for browsing, searching, and inspecting memories (ratatui-based).
- **`shabka demo`** — seed sample memories for instant first-run experience.
- **Install script** — `curl -sSf ... | sh` for Linux/macOS.
- **Release pipeline** — GitHub Actions cross-compiles for Linux, macOS (Intel + Apple Silicon), Windows.
- **CI expanded** — tests all 5 crates on every push/PR.

## [0.3.0] — 2026-02-21

SQLite as the default storage backend — zero external dependencies.

- **SQLite storage mode** — works out of the box, no HelixDB required. Brute-force cosine vector search, WAL mode, 4-table schema.
- **Configurable backend** — `[storage]` config section with `backend = "sqlite"` (default) or `"helix"`.
- **`get_context` MCP tool** — token-budgeted context packs with query/project/kind/tag filters (14th tool).
- **Rig v0.31 integration** — replaced hand-rolled HTTP with `rig-core` adapters for embeddings and completions.
- **Structured extraction** — `generate_structured<T>()` for typed LLM responses, used in auto-tag, consolidate, dedup.
- **8 LLM providers** — ollama, openai, gemini, anthropic, deepseek, groq, xai, cohere.
- **5 embedding providers** — hash, ollama, openai, gemini, cohere.
- **334 tests** across 5 crates.

## [0.2.0] — 2026-02-18

Quality, trust, and advanced retrieval features.

- **Trust scoring** — 4-factor formula (verification, source, contradictions, quality) as a ranking signal.
- **Verification status** — `Verified`, `Disputed`, `Outdated`, `Unverified` — settable via MCP, CLI, and web dashboard.
- **Memory consolidation** — merge similar memory clusters via LLM (CLI + MCP tool).
- **Session compression** — hooks batch events per session, compress at stop (heuristic or LLM-powered).
- **LLM auto-tagging** — automatic tag extraction from memory content.
- **Contradiction handling** — `Contradict` dedup decision, `Contradicts` relation type.
- **Quality dashboard** — quality score gauge, issue counts, top issues on analytics page.
- **Context packs** — `shabka context-pack` CLI for paste-ready LLM context blocks.
- **13 MCP tools** — added `verify_memory`, `consolidate`, `history`, `follow_chain`, `assess`.

## [0.1.0] — 2026-02-14

Initial release — foundation for persistent LLM memory.

- **Core data model** — Memory struct with embeddings, tags, kind, importance, relations.
- **MCP server** — 8 tools for search, save, update, delete, relate, reembed.
- **Auto-capture hooks** — Claude Code PostToolUse and Stop hooks for zero-effort memory capture.
- **Web dashboard** — Axum + Askama, memory list, detail view, graph visualization, analytics, dark/light theme.
- **CLI** — search, get, chain, prune, status, export, import, init, reembed.
- **Multi-provider embeddings** — hash (default), ollama, openai, gemini.
- **Hybrid search** — keyword scoring fused with vector similarity in ranking formula.
- **PII scrubbing** — regex-based redaction for exports (emails, API keys, IPs, file paths).

[0.5.1]: https://github.com/mehdig-dev/shabka/releases/tag/v0.5.1
[0.5.0]: https://github.com/mehdig-dev/shabka/releases/tag/v0.5.0
[0.4.0]: https://github.com/mehdig-dev/shabka/releases/tag/v0.4.0
[0.3.0]: https://github.com/mehdig-dev/shabka/releases/tag/v0.3.0
[0.2.0]: https://github.com/mehdig-dev/shabka/releases/tag/v0.2.0
[0.1.0]: https://github.com/mehdig-dev/shabka/releases/tag/v0.1.0
