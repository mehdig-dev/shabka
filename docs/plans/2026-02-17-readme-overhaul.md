# README Overhaul & Screenshots Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restructure README as a user-facing landing page with screenshots and value proposition. Move detailed docs to `docs/`.

**Architecture:** Seed ~15 demo memories via REST API, take 6 Playwright screenshots, rewrite README to ~90 lines, extract detailed content into 6 doc files under `docs/`.

**Tech Stack:** Playwright (MCP), curl (seed data), Markdown

---

### Task 1: Seed Demo Data

**Prereqs:** HelixDB running (`just db`), web dashboard running (`just web` or `cargo run -p shabka-web --no-default-features`)

**Step 1: Seed 15 memories via REST API**

Run each curl command to create memories through the web API (`POST /api/v1/memories`). These create a realistic-looking project with varied kinds, tags, and importance levels.

```bash
# 1. Decision: Authentication approach
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Use JWT with refresh tokens for API auth","content":"After evaluating session-based auth vs JWT vs OAuth2, we chose JWT with short-lived access tokens (15min) and longer refresh tokens (7d). Reasons:\n\n1. **Stateless** — no server-side session store needed\n2. **Mobile-friendly** — tokens work across platforms\n3. **Microservice-ready** — services can verify tokens independently\n\nRefresh tokens are stored in httpOnly cookies. Access tokens go in Authorization header.","kind":"decision","tags":["auth","jwt","security","api"],"importance":0.9}'

# 2. Pattern: Error handling
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Centralized error handling with typed error enum","content":"All API errors go through a unified `AppError` enum that implements `IntoResponse`. Each variant maps to an HTTP status code:\n\n```rust\nenum AppError {\n    NotFound(String),      // 404\n    Validation(String),    // 422\n    Unauthorized,          // 401\n    Internal(anyhow::Error), // 500\n}\n```\n\nThis prevents leaking internal details to clients while giving structured error responses.","kind":"pattern","tags":["rust","error-handling","api"],"importance":0.7}'

# 3. Observation: Database query performance
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Vector search latency spikes above 10k memories","content":"Observed that HelixDB vector search latency increases from ~5ms to ~50ms when the memory count exceeds 10,000. The HNSW index parameters may need tuning.\n\nCurrent config: ef_construction=200, M=16. Consider increasing M to 32 for better recall at scale, or implementing pre-filtering by kind/project before vector search.","kind":"observation","tags":["performance","helixdb","vector-search"],"importance":0.6}'

# 4. Error: CORS misconfiguration
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"CORS preflight failing for PUT requests from dashboard","content":"The web dashboard was getting CORS errors when updating memories via PUT /api/v1/memories/{id}. Root cause: the CORS middleware only allowed GET and POST methods.\n\nFix: Added PUT and DELETE to the allowed methods list in the Axum CORS layer configuration.","kind":"error","tags":["cors","web","bug"],"importance":0.5}'

# 5. Fix: CORS resolution
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Fix CORS by adding PUT/DELETE to allowed methods","content":"Updated the CORS middleware configuration in `shabka-web/src/main.rs` to include all required HTTP methods:\n\n```rust\nCorsLayer::new()\n    .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])\n    .allow_origin(Any)\n    .allow_headers(Any)\n```\n\nVerified with `curl -X OPTIONS` that preflight now returns correct headers.","kind":"fix","tags":["cors","web"],"importance":0.5}'

# 6. Lesson: Embedding provider migration
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Always re-embed all memories when switching providers","content":"When switching from hash embeddings to ollama (nomic-embed-text), search results were garbage because old memories had 128d hash vectors while new ones had 768d semantic vectors.\n\n**Lesson:** Run `shabka reembed --force` after any provider change. The CLI now warns about this automatically via EmbeddingState migration detection.","kind":"lesson","tags":["embeddings","migration","ops"],"importance":0.8}'

# 7. Decision: Graph database choice
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"HelixDB chosen over Neo4j for graph+vector storage","content":"Evaluated three options for combined graph + vector storage:\n\n| Option | Pros | Cons |\n|--------|------|------|\n| Neo4j + Pinecone | Mature, scalable | Two services, complex sync |\n| HelixDB | Single binary, native graph+vector | Newer, smaller community |\n| PostgreSQL + pgvector | Familiar, battle-tested | No native graph traversal |\n\nChose HelixDB for simplicity — single service with both graph edges and HNSW vector index. Docker-based deployment keeps it portable.","kind":"decision","tags":["architecture","database","helixdb"],"importance":0.9}'

# 8. Pattern: Layered configuration
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Three-layer TOML config: global, project, local","content":"Configuration loads in priority order (later overrides earlier):\n\n1. **Global** `~/.config/shabka/config.toml` — user-wide defaults\n2. **Project** `.shabka/config.toml` — committed to git, shared with team\n3. **Local** `.shabka/config.local.toml` — gitignored, personal overrides\n\nThis lets teams share embedding provider and graph settings while individuals override API keys and user IDs locally.","kind":"pattern","tags":["config","architecture"],"importance":0.7}'

# 9. Fact: MCP protocol details
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"MCP uses JSON-RPC 2.0 over stdio for tool communication","content":"The Model Context Protocol (MCP) is a standard for LLM tool integration. Key details:\n\n- Transport: JSON-RPC 2.0 over stdin/stdout\n- Tools are registered with name, description, and JSON Schema parameters\n- The LLM client (Claude Code, Cursor) discovers tools at startup\n- Each tool call is a request/response pair\n\nShabka exposes 12 tools via rmcp 0.14, registered as a stdio server.","kind":"fact","tags":["mcp","protocol","integration"],"importance":0.6}'

# 10. Todo: Add semantic search to CLI
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Add fuzzy matching fallback when vector search returns no results","content":"Currently if the embedding provider returns a vector that does not match anything (e.g., a very niche query), the user gets zero results with no explanation.\n\nShould add a fallback that does substring matching on titles and content when vector search returns empty. Show a message like \"No semantic matches found, showing keyword results instead.\"","kind":"todo","tags":["search","ux","cli"],"importance":0.4}'

# 11. Preference: Code style
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Prefer explicit error handling over unwrap in production code","content":"Project convention: never use `.unwrap()` or `.expect()` in library code (shabka-core). Use `anyhow::Result` for application code (CLI, web, hooks) and `thiserror` for typed errors in core.\n\nTests can use `.unwrap()` freely since panics are the correct behavior for unexpected failures in test code.","kind":"preference","tags":["rust","style","errors"],"importance":0.5}'

# 12. Observation: Session compression effectiveness
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Heuristic session compression captures 80% of useful context","content":"After running session compression for 2 weeks with both heuristic and LLM modes:\n\n- **Heuristic** (no LLM): Captures file edit patterns, error/fix pairs, command sequences. ~80% as useful as LLM compression.\n- **LLM** (ollama/llama3.2): Generates better titles and extracts intent, but adds 2-5s latency per session.\n\nRecommendation: Use heuristic by default, enable LLM mode only when running a local model with low latency.","kind":"observation","tags":["hooks","compression","benchmarks"],"importance":0.7}'

# 13. Decision: Privacy model
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Three-tier privacy: public, team, private","content":"Memories have three privacy levels:\n\n- **Public** — visible to everyone (cross-team knowledge)\n- **Team** — visible to all team members (shared project context)\n- **Private** — visible only to the creator (personal notes, credentials references)\n\nDefault is `private`. The sharing filter runs on every search/list/timeline query before results reach the user. Export respects privacy thresholds.","kind":"decision","tags":["privacy","sharing","security"],"importance":0.8}'

# 14. Pattern: Smart deduplication
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Three-tier dedup: skip, supersede, or add based on similarity","content":"When saving a new memory, Shabka checks embedding similarity against existing memories:\n\n- **>=0.95 similarity** → Skip (exact duplicate, do not save)\n- **>=0.85 similarity** → Supersede (update existing memory, archive old version)\n- **<0.85 similarity** → Add (new memory, auto-relate if >0.6)\n\nThresholds are configurable in `[graph]` config section. LLM-powered dedup can override these decisions with semantic understanding.","kind":"pattern","tags":["dedup","embeddings","quality"],"importance":0.7}'

# 15. Lesson: Testing with HelixDB
curl -s -X POST http://127.0.0.1:37737/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{"title":"Use UUID-tagged titles to prevent test collision in shared HelixDB","content":"Integration tests that run against a shared HelixDB instance can collide if they use static titles like \"test memory\". Solution: prefix test titles with a UUID generated per test run.\n\n```rust\nlet tag = Uuid::now_v7();\nlet title = format!(\"test-{tag}-auth-flow\");\n```\n\nThe `#[ignore]` attribute ensures these tests only run with `--ignored` flag, preventing accidental execution in CI without HelixDB.","kind":"lesson","tags":["testing","helixdb","ci"],"importance":0.6}'
```

**Step 2: Create relations between memories**

After seeding, extract the IDs from the responses and create relations. The exact IDs will be dynamic, so use the list API to find them:

```bash
# Get all memory IDs
MEMORIES=$(curl -s http://127.0.0.1:37737/api/v1/memories | python3 -c "
import sys, json
data = json.load(sys.stdin)
for m in data:
    print(f\"{m['id']}\t{m['title'][:50]}\")
")

# Create relations between:
# - CORS error -> CORS fix (fixes)
# - JWT decision -> Privacy decision (related)
# - HelixDB decision -> Vector search observation (caused_by)
# - Embedding lesson -> Smart dedup pattern (related)
# Use the IDs from the list output above

# Example (replace IDs with actual values):
# curl -s -X POST http://127.0.0.1:37737/api/v1/memories/{cors_fix_id}/relate \
#   -H "Content-Type: application/json" \
#   -d '{"target_id":"{cors_error_id}","relation_type":"fixes"}'
```

**Step 3: Verify data looks good**

Open `http://127.0.0.1:37737/` in browser and confirm:
- 15 memories visible on list page
- Mixed kinds with colored badges
- Relations visible on detail pages

---

### Task 2: Take Screenshots via Playwright

**Prereqs:** Task 1 complete, web dashboard running

**Step 1: Create screenshots directory**

```bash
mkdir -p docs/screenshots
```

**Step 2: Capture 6 screenshots**

Use Playwright MCP tools at 1280x800 viewport:

1. **Dashboard** — Navigate to `/`, capture full list page with memory cards
2. **Search** — Navigate to `/search?q=authentication`, capture results with highlights
3. **Detail** — Navigate to a specific memory (JWT auth decision), capture with relations + markdown
4. **Graph** — Navigate to `/graph`, wait for graph to render, capture
5. **Analytics** — Navigate to `/analytics`, capture charts and quality gauge
6. **Dark mode** — On the dashboard, toggle dark mode via navbar button, capture

Save each as `docs/screenshots/{name}.png`.

**Step 3: Verify screenshots**

Check each file exists and looks good (no loading spinners, no empty states).

**Step 4: Commit screenshots**

```bash
git add docs/screenshots/
git commit -m "docs: add web dashboard screenshots for README"
```

---

### Task 3: Create Documentation Files

**Files to create:** 6 markdown files under `docs/`, each extracting content from the current README.

**Step 1: Create `docs/configuration.md`**

Extract from current README lines 219-270 (Configuration section). Add "Back to README" link at top. Content:
- Layered TOML config explanation
- Full config reference with all sections
- Embedding providers table

**Step 2: Create `docs/cli.md`**

Extract from current README lines 116-176 (CLI section). Add all commands with all flags. Content:
- Installation instructions
- Complete command reference (all 13 commands)
- Examples for common workflows

**Step 3: Create `docs/web-dashboard.md`**

Extract from current README lines 170-206 (Web Dashboard + REST API sections). Content:
- Features list
- REST API endpoints table
- Screenshots of each page (referencing `screenshots/` dir)

**Step 4: Create `docs/api.md`**

Extract from current README lines 49-72 (MCP Tools section) + REST API. Content:
- MCP tools table with descriptions
- REST API endpoints table
- Retrieval pattern explanation
- Smart dedup explanation

**Step 5: Create `docs/development.md`**

Extract from current README lines 282-319 (Development + Testing + Resetting sections). Content:
- Dev commands (just recipes)
- Testing guide (unit, integration, test counts)
- Resetting HelixDB instructions

**Step 6: Create `docs/architecture.md`**

Extract from current README lines 11-47 + 321-363 (Architecture + Project Structure). Content:
- Architecture diagram (ASCII)
- Workspace crates table
- Module descriptions
- Project structure tree

**Step 7: Commit all docs**

```bash
git add docs/
git commit -m "docs: extract detailed documentation from README into docs/"
```

---

### Task 4: Rewrite README

**File:** `README.md`

**Step 1: Replace entire README with new landing page**

The new README should be ~90 lines:

```markdown
<p align="center">
  <img src="shabka.png" alt="Shabka" width="200">
</p>

<h1 align="center">Shabka</h1>

<p align="center">A shared LLM memory system. Save, search, and connect knowledge across AI coding sessions.</p>

---

## Why Shabka?

LLMs forget everything between sessions. Shabka fixes that.

- **Persistent memory** — Decisions, patterns, and lessons survive across sessions. Your AI assistant remembers what worked and what didn't.
- **Team knowledge sharing** — Share architectural decisions and project context across your team with privacy controls (public/team/private).
- **Zero-effort capture** — Auto-captures from Claude Code sessions via hooks. No manual note-taking needed.
- **Works everywhere** — MCP server works with Claude Code, Cursor, and any MCP-compatible client. CLI and web dashboard for direct access.

## Screenshots

| | |
|---|---|
| ![Dashboard](docs/screenshots/dashboard.png) **Memory Dashboard** — Browse, filter, and manage memories | ![Search](docs/screenshots/search.png) **Semantic Search** — Hybrid vector + keyword search with highlights |
| ![Detail](docs/screenshots/detail.png) **Memory Detail** — Markdown rendering, relations, audit history | ![Graph](docs/screenshots/graph.png) **Knowledge Graph** — Interactive visualization of memory connections |
| ![Analytics](docs/screenshots/analytics.png) **Analytics** — Quality scores, trends, and distribution charts | ![Dark Mode](docs/screenshots/dark-mode.png) **Dark Mode** — Full dark theme with toggle |

## Quick Start

### Prerequisites

- Rust 1.80+, Docker, [just](https://github.com/casey/just) (optional)

### 1. Install & start HelixDB

```bash
cargo install --git https://github.com/HelixDB/helix-db helix-cli
just db
```

### 2. Register the MCP server

```bash
claude mcp add shabka -- cargo run --manifest-path /path/to/shabka/Cargo.toml -p shabka-mcp --no-default-features
```

### 3. Start using it

Open a new Claude Code session. The 12 Shabka tools are available:

- "Save a memory about how our auth system works"
- "Search for authentication patterns"
- "Show me the memory timeline for today's session"

## CLI

Install: `just cli-install`

```bash
shabka search "auth flow"                    # Semantic search
shabka search "auth" --token-budget 1500     # Budget-aware search
shabka get a1b2c3d4                          # View memory details
shabka context-pack --project myapp          # Paste-ready context block
shabka status                                # Health check
```

See [full CLI reference](docs/cli.md) for all 13 commands.

## Web Dashboard

```bash
just web   # http://localhost:37737
```

Browse memories, search, visualize the knowledge graph, view analytics. See [dashboard docs](docs/web-dashboard.md).

## Documentation

| | |
|---|---|
| [Configuration](docs/configuration.md) | TOML config, embedding providers, privacy settings |
| [CLI Reference](docs/cli.md) | All 13 commands with flags and examples |
| [Web Dashboard](docs/web-dashboard.md) | Features, REST API, screenshots |
| [MCP & API](docs/api.md) | 12 MCP tools, REST endpoints, retrieval patterns |
| [Development](docs/development.md) | Building, testing, dev commands |
| [Architecture](docs/architecture.md) | Crate structure, modules, design decisions |

## License

MIT OR Apache-2.0
```

**Step 2: Verify links resolve**

Check that all 6 doc files exist and screenshot paths are correct.

**Step 3: Commit**

```bash
git add README.md
git commit -m "docs: rewrite README as user-facing landing page with screenshots"
```

---

### Task 5: Final Validation

**Step 1: Verify all links**

Check each `docs/*.md` file exists and has content. Check each `docs/screenshots/*.png` exists.

**Step 2: Verify README renders well**

Read through the final README and ensure the screenshot grid, links, and formatting are correct.

**Step 3: Run `just check` to ensure no code regressions**

```bash
just check
```

Expected: all tests pass, clippy clean.

**Step 4: Push**

```bash
git push origin main
```
