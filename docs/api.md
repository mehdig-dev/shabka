# API Reference

[← Back to README](../README.md)

## MCP Tools

Shabka exposes 13 tools via the MCP protocol:

| Tool | Description |
|------|-------------|
| `search` | Semantic + keyword hybrid search (supports `token_budget` for capped results) |
| `get_memories` | Retrieve full memory details by ID |
| `timeline` | Chronological view with optional date/session filters |
| `save_memory` | Create a new memory with auto-embedding, smart dedup, and auto-relate |
| `update_memory` | Modify title, content, tags, importance, status, verification |
| `delete_memory` | Permanently remove a memory |
| `relate_memories` | Link two memories (caused_by, fixes, supersedes, related, contradicts) |
| `follow_chain` | BFS traversal along typed edges (debugging narratives, version history) |
| `reembed` | Re-embed memories with current provider (incremental or forced) |
| `history` | View audit trail of memory mutations |
| `assess` | Memory quality scorecard (0-100 score, issue counts, top issues) |
| `consolidate` | Merge clusters of similar memories using LLM |
| `verify_memory` | Set verification status (verified, disputed, outdated, unverified) |

**Retrieval pattern:** Start with `search` (compact index, ~50-100 tokens each), drill into `get_memories` for full content, use `timeline` for chronological context. Pass `token_budget` to `search` to cap results within a token limit (~4 chars/token estimate) — useful for rate-limited or budget-conscious LLM usage.

**Smart dedup:** When saving, Shabka checks for near-duplicates via embedding similarity. Exact matches (>=0.95) are skipped, near-matches (>=0.85) supersede the old memory, and new content is auto-related to similar existing memories.

## REST API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/memories` | POST | Create memory (dedup-aware) |
| `/api/v1/memories` | GET | List memories (`?kind=&limit=&status=`) |
| `/api/v1/memories/{id}` | GET | Get memory with relations |
| `/api/v1/memories/{id}` | PUT | Update memory |
| `/api/v1/memories/{id}` | DELETE | Delete memory |
| `/api/v1/memories/{id}/relate` | POST | Add relation |
| `/api/v1/memories/{id}/relations` | GET | Get relations |
| `/api/v1/memories/{id}/history` | GET | Get audit history |
| `/api/v1/search` | GET | Search (`?q=&kind=&limit=&tag=`) |
| `/api/v1/timeline` | GET | Timeline (`?limit=&session_id=`) |
| `/api/v1/stats` | GET | Analytics data |
| `/api/v1/memories/bulk/archive` | POST | Bulk archive by IDs |
| `/api/v1/memories/bulk/delete` | POST | Bulk delete by IDs |
