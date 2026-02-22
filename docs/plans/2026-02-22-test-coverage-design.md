# Test Coverage — Breadth-First Design

## Goal

Raise test coverage across all 4 under-tested crates (MCP, CLI, Web, Hooks) from 374 to ~444 tests. Focus on the highest-value untested code paths in each crate.

## Current State

| Crate | Tests | Coverage Quality |
|-------|-------|-----------------|
| shabka-core | 303 | Excellent |
| shabka-hooks | 37 | Good for classify/session, gaps in relate + async |
| shabka-mcp | 10 | Poor — error mapping + serde only |
| shabka-web | 12 | API routes only |
| shabka-cli | 12 | TUI only |

## Test Infrastructure

All crates share the same pattern:
- **Storage:** `SqliteStorage::open_in_memory()` — full SQLite with extensions, zero I/O
- **Embeddings:** Hash provider (128d deterministic, no network)
- **Config:** `ShabkaConfig::default_config()` — no file reads
- **Async:** `#[tokio::test]` everywhere

---

## MCP Crate (~20 new tests)

Construct `ShabkaServer` with in-memory storage. Call tool handlers directly via `server.tool_name(Parameters(params)).await`.

| Test | Handler | Validates |
|------|---------|-----------|
| `test_search_empty` | search | Empty store returns no results |
| `test_search_with_results` | search | Keyword match after save |
| `test_save_memory` | save_memory | Returns ID, persists |
| `test_save_memory_validation` | save_memory | Empty title → error |
| `test_get_memories` | get_memories | Retrieve by ID |
| `test_get_memories_not_found` | get_memories | Missing ID → error |
| `test_update_memory` | update_memory | Title change persists |
| `test_delete_memory` | delete_memory | Removed from store |
| `test_timeline` | timeline | Chronological order |
| `test_relate_memories` | relate_memories | Relation created |
| `test_follow_chain` | follow_chain | Multi-hop traversal |
| `test_history` | history | Audit trail recorded |
| `test_verify_memory` | verify_memory | Status set correctly |
| `test_assess` | assess | Quality report structure |
| `test_get_context` | get_context | Token-budgeted output |
| `test_reembed` | reembed | No error on re-embed |
| `test_save_session_summary` | save_session_summary | Batch save works |
| `test_save_session_summary_dedup` | save_session_summary | Dedup detected |
| `test_consolidate_no_llm` | consolidate | Error without LLM |

---

## CLI Crate (~18 unit + ~6 integration)

### Unit tests

Call `cmd_*()` directly with in-memory storage. Verify `Result::Ok` and storage side effects. JSON output mode tests capture stdout.

| Test | Command | Validates |
|------|---------|-----------|
| `test_cmd_search_no_results` | search | Empty store OK |
| `test_cmd_search_with_results` | search | Finds saved memory |
| `test_cmd_search_json` | search | JSON output parseable |
| `test_cmd_get_found` | get | Full memory display |
| `test_cmd_get_not_found` | get | Error on missing ID |
| `test_cmd_list_empty` | list | Empty table |
| `test_cmd_list_with_filter` | list | Kind filter works |
| `test_cmd_delete_single` | delete | Removes memory |
| `test_cmd_delete_bulk_no_confirm` | delete | Fails without --confirm |
| `test_cmd_status` | status | Runs without error |
| `test_cmd_export_import` | export+import | Roundtrip preserves data |
| `test_cmd_chain_no_relations` | chain | Handles isolated memory |
| `test_cmd_history` | history | Shows audit events |
| `test_cmd_verify` | verify | Sets status |
| `test_cmd_prune_dry_run` | prune | Dry run no side effects |
| `test_cmd_assess` | assess | Quality report |
| `test_cmd_context_pack` | context-pack | Generates markdown |
| `test_cmd_demo_and_clean` | demo | Seed + cleanup |

### Integration tests (`#[ignore]`)

Run `shabka` binary via subprocess, assert exit codes and stdout.

| Test | Command | Validates |
|------|---------|-----------|
| `test_cli_list_json` | `shabka list --json` | JSON array on stdout |
| `test_cli_delete_requires_confirm` | `shabka delete --kind error` | Non-zero exit |
| `test_cli_search_fuzzy` | `shabka search "authentcation"` | Fuzzy matching works |
| `test_cli_demo_lifecycle` | `shabka demo` → `shabka demo --clean` | Full lifecycle |
| `test_cli_init_creates_config` | `shabka init` | Config file created |
| `test_cli_status_output` | `shabka status` | Zero exit code |

---

## Web Crate (~15 new tests)

Extend existing `test_app_state()` / `test_router()` infrastructure. Use `tower::ServiceExt::oneshot()` for both API and page handler tests. Page tests verify status code + Content-Type.

| Test | Route | Validates |
|------|-------|-----------|
| `test_health_endpoint` | GET /health | 200 + JSON status |
| `test_not_found_handler` | GET /nonexistent | 404 response |
| `test_list_memories_page` | GET /memories | HTML 200 |
| `test_show_memory_page` | GET /memories/{id} | HTML with memory data |
| `test_search_page` | GET /search?q=test | HTML 200 |
| `test_timeline_page` | GET /timeline | HTML 200 |
| `test_graph_page` | GET /graph | HTML 200 |
| `test_analytics_page` | GET /analytics | HTML 200 |
| `test_graph_data_json` | GET /api/v1/graph | JSON nodes/edges |
| `test_create_memory_form` | GET /memories/new | HTML form |
| `test_create_memory_dedup` | POST duplicate | Dedup behavior |
| `test_pagination` | GET /memories?page=2 | Page bounds |
| `test_api_error_format` | Invalid API call | Structured JSON error |
| `test_search_project_filter` | GET /search?project=x | Filtered results |
| `test_memory_chain_api` | GET /api/v1/memories/{id}/chain | Chain JSON |

---

## Hooks Crate (~10 new tests)

Fill gaps in relate strategies, session edge cases, and classify coverage.

| Test | Function | Validates |
|------|----------|-----------|
| `test_session_thread_matching` | session_thread() | Session-based grouping |
| `test_same_file_cluster` | same_file_cluster() | File-path grouping |
| `test_error_fix_chain` | error_fix_chain() | Error→fix detection |
| `test_auto_relate_orchestration` | auto_relate() | All strategies compose |
| `test_find_stale_buffers` | find_stale_buffers() | Stale detection |
| `test_buffer_dedup_edge_cases` | SessionBuffer::append() | Identical content dedup |
| `test_classify_write_file_change` | classify() | Write tool classified |
| `test_classify_bash_truncation` | classify() | Long command truncated |
| `test_compress_heuristic_no_events` | compress_heuristic() | Empty → None |
| `test_derive_project_id` | derive_project_id() | Path extraction |

---

## Summary

- **~70 new tests** across 4 crates
- **No new dependencies** — all use existing in-memory SQLite + hash embedder
- **No LLM required** — tests verify behavior without AI services
- **Target: 374 → ~444 tests**
