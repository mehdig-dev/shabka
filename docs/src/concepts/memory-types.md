# Memory Types

Shabka organizes knowledge using the **COALA memory architecture** ([Cognitive Architectures for Language Agents](https://arxiv.org/abs/2309.02427)), a framework from academic research on how language agents should manage long-term memory. COALA identifies three complementary memory types that mirror how humans learn and recall:

## The Three Memory Types

### Procedural Memory — Rules and Preferences

**What:** Standing instructions, coding conventions, and personal preferences that should apply across all sessions.

**When to use:** When the user says "remember", "always", "never", or "from now on".

**How it works in Shabka:**

```
User: "Remember: always use parameterized queries in this project"

→ Claude calls the `remember` MCP tool
→ Saved as kind: procedure, importance: 0.9
→ Tagged: rule, preference
→ Retrieved automatically in future sessions via get_context
```

The `remember` tool is purpose-built for procedural memory. It sets high importance (0.9) so rules surface reliably, and tags memories as `rule` + `preference` for easy filtering. You can scope rules to a project with `project_id`.

**Examples:**
- "Always use snake_case for variable names"
- "Never commit directly to main"
- "Prefer composition over inheritance in this codebase"
- "Run `cargo clippy` before every commit"

### Semantic Memory — Facts and Knowledge

**What:** Technical knowledge, architectural decisions, patterns, and factual information about a codebase.

**When to use:** When saving knowledge that should persist — how systems work, why decisions were made, what patterns to follow.

**How it works in Shabka:**

```
User: "Save a memory about our authentication system"

→ Claude calls `save_memory` with kind: decision/pattern/fact
→ Embedded for vector search
→ Connected to related memories via typed relations
```

Semantic memories are the bulk of what Shabka stores. They span multiple kinds:

| Kind | Use Case |
|------|----------|
| `observation` | What you noticed — "The API response time spikes after 5pm" |
| `decision` | Why something was chosen — "We picked PostgreSQL over MongoDB for ACID guarantees" |
| `pattern` | Recurring approaches — "All API endpoints follow the handler → service → repository pattern" |
| `fact` | Objective information — "The database schema has 12 tables" |
| `error` | What went wrong — "OOM crash when batch size exceeds 10k" |
| `fix` | How it was resolved — "Fixed by adding pagination with cursor-based offset" |
| `lesson` | What was learned — "Always check for null before accessing nested fields" |
| `preference` | Style choices — "Team prefers explicit error handling over exceptions" |
| `todo` | Future work — "Need to add rate limiting to the public API" |

### Episodic Memory — Session Experiences

**What:** Records of what happened during a coding session — what was accomplished, what files were changed, what problems were encountered.

**When to use:** Automatically captured at the end of Claude Code sessions via the `save_session_summary` tool and hooks.

**How it works in Shabka:**

```
Claude Code session ends

→ Hooks trigger session compression
→ Key events extracted: files edited, commands run, decisions made
→ Saved as a session summary memory
→ Next session: "What did we do yesterday?" retrieves the summary
```

Episodic memories provide continuity between sessions. When you ask "what were we working on?", Shabka retrieves session summaries that reconstruct the timeline.

## How Memory Types Work Together

The three types complement each other:

1. **Procedural** memories set the rules: *"always use parameterized queries"*
2. **Semantic** memories provide context: *"our database uses PostgreSQL 15 with pgvector"*
3. **Episodic** memories track history: *"yesterday we migrated the user table to the new schema"*

When `get_context` is called, Shabka retrieves all three types, giving the LLM a complete picture: the rules to follow, the knowledge to draw on, and the recent history to continue from.

## Beyond Flat Memory: Relations and Trust

Unlike flat key-value memory stores, Shabka connects memories with **typed relations**:

| Relation | Meaning |
|----------|---------|
| `supports` | This memory provides evidence for another |
| `contradicts` | This memory conflicts with another |
| `extends` | This memory adds detail to another |
| `supersedes` | This memory replaces an older one |
| `related_to` | General topical connection |

Relations enable **chain traversal** — follow a memory's connections to discover related knowledge:

```bash
shabka chain a1b2c3d4 --depth 3   # Follow relations up to 3 levels deep
```

Memories also carry a **trust score** (verified, contested, unverified) and undergo **auto-consolidation** — when enough related memories accumulate, an LLM merges them into a comprehensive summary and supersedes the originals.

## Memory Lifecycle

```
Created → Active → [Consolidated / Archived / Superseded]
                ↗
        Pending (if review mode enabled)
```

- **Active** — Appears in search results and context
- **Pending** — Saved but awaiting approval (when `review_mode: true` in config)
- **Archived** — Hidden from search but preserved
- **Superseded** — Replaced by a consolidated memory, linked via `supersedes` relation

## References

- Sumers, T.R., et al. (2023). [Cognitive Architectures for Language Agents](https://arxiv.org/abs/2309.02427). *arXiv:2309.02427*
- LangChain. (2024). [Memory in Agents](https://blog.langchain.dev/memory-in-agents/). LangChain Blog.
