# Adoption & Growth Strategy

**Goal:** Go from zero users to first traction by fixing positioning, distribution, and discoverability — not adding more features.

**Principle:** The bottleneck is marketing, not engineering. Ship less code, write more words.

---

## Week 1: README Rewrite + HN Launch

### Task 1: Rewrite README with killer positioning

**Problem:** Current README says "Persistent memory for LLM coding agents" — identical to Claude-Mem (30k stars). Shabka is invisible.

**Action:**
- [ ] New tagline: lead with what's unique (Rust, zero-config, no API keys, single binary)
- [ ] Add "Why Shabka?" section (3-4 bullet points, not a wall of text)
- [ ] Add comparison table vs Mem0 / Claude-Mem / Zep (no API key, single binary, built-in graph, trust scoring, PII scrubbing, 15 MCP tools, web dashboard)
- [ ] Add "What gets auto-captured?" explanation — users don't know what `shabka init` hooks into
- [ ] Add "Day 1 workflow" section: install -> init -> work normally -> search next day
- [ ] Add "When to use which embedding provider?" guidance (hash for testing, Ollama for local semantic, OpenAI for production)
- [ ] Move screenshots to show "aha moments" — graph viz, search results, analytics

### Task 2: Write HN launch post

**Action:**
- [ ] Title: "Show HN: Shabka — Rust-native memory for coding agents (no API keys, single binary)"
- [ ] Body: problem (LLMs forget), why existing solutions need API keys / Python infra, how Shabka is different
- [ ] Post on weekday 8-9am ET for best visibility
- [ ] Cross-post to r/rust, r/LocalLLaMA, r/ClaudeAI

---

## Week 2: Content + Benchmarks

### Task 3: Write "Why I Built Shabka" blog post

**Action:**
- [ ] Personal story: what problem you hit, why existing tools didn't work
- [ ] Publish on mehdig-dev.github.io/shabka/blog/ or dev.to
- [ ] Link from README

### Task 4: Write "Shabka vs Mem0 vs Claude-Mem" comparison post

**Action:**
- [ ] Honest comparison — acknowledge where competitors are stronger (ecosystem, SDK languages)
- [ ] Highlight where Shabka wins (zero-config, no API key, Rust performance, privacy, trust scoring)
- [ ] Include the comparison table from Task 1

### Task 5: Publish latency benchmarks

**Action:**
- [ ] Benchmark search latency at 1k / 10k / 100k memories (SQLite + sqlite-vec)
- [ ] Benchmark cold start time (single binary vs Python + Qdrant)
- [ ] Benchmark memory usage (RSS of shabka-mcp vs mem0 server)
- [ ] Publish results in docs and blog post
- [ ] Even if semantic accuracy isn't best (hash embeddings), latency and resource usage will win

---

## Week 3: Expand Reach

### Task 6: Ship Python SDK wrapper

**Problem:** LLM memory market is 95% Python. No Python SDK = invisible to most developers.

**Action:**
- [ ] Create `shabka` PyPI package that wraps the HTTP API
- [ ] API: `Shabka()`, `.save()`, `.search()`, `.get()`, `.delete()`, `.timeline()`
- [ ] Requires shabka-mcp or shabka-web running locally (document this clearly)
- [ ] Quickstart in README:
  ```python
  from shabka import Shabka
  s = Shabka()
  s.save("User prefers dark mode", kind="preference", tags=["ui"])
  results = s.search("user preferences")
  ```

### Task 7: Add `shabka init` for more clients

**Problem:** Auto-capture only works for Claude Code. Cursor/Windsurf/Cline users must configure manually.

**Action:**
- [ ] `shabka init --cursor` — generates MCP config for Cursor
- [ ] `shabka init --windsurf` — generates MCP config for Windsurf
- [ ] `shabka init --cline` — generates MCP config for Cline
- [ ] Print clear instructions for what was configured and how to verify

---

## Week 4: Community + Discoverability

### Task 8: Submit to curated lists

**Action:**
- [ ] [awesome-mcp-servers](https://github.com/punkpeye/awesome-mcp-servers) — PR to add Shabka
- [ ] [awesome-rust](https://github.com/rust-unofficial/awesome-rust) — PR under "AI / Machine Learning"
- [ ] [awesome-llm](https://github.com/Hannibal046/Awesome-LLM) or similar
- [ ] MCP registry (if not already listed prominently)

### Task 9: Engage in existing conversations

**Action:**
- [ ] Find GitHub discussions / Reddit threads about "LLM memory", "MCP memory server", "Claude memory"
- [ ] Comment with genuine help + mention Shabka where relevant (not spam)
- [ ] Answer questions on r/LocalLLaMA about local-first memory setups
- [ ] Post in Claude Code community channels

### Task 10: Add social proof to README

**Action:**
- [ ] Star count badge
- [ ] Downloads badge (crates.io)
- [ ] "Used by" section (even if it's just you — "Built for personal use, now open source")
- [ ] Link to any blog posts, HN discussions, or community mentions

---

## Future Quarter: Bigger Bets (Not Yet)

These are worth doing but only after Weeks 1-4 generate initial traction:

- **TypeScript SDK** — second-biggest audience after Python
- **Hosted demo / playground** — try without installing
- **Temporal memory** — `valid_at`/`invalid_at` timestamps (Graphiti's angle, 23k stars)
- **Agent framework guides** — LangChain, CrewAI, AutoGen integration docs
- **LoCoMo benchmark** — formal accuracy comparison (needs Ollama or OpenAI embeddings)
- **Video content** — 2-min "zero to first memory" screencast

---

## Anti-Patterns to Avoid

- **Don't add more Rust features.** You have more features than projects with 30k stars.
- **Don't build a managed cloud.** No users to upsell yet.
- **Don't try to out-feature Mem0.** They have $24M and 254 contributors.
- **Don't write generic docs.** Write opinionated guides ("Here's the best way to...")
- **Don't wait for perfection.** Ship the README rewrite before the benchmarks are ready.

---

## Success Metrics

| Metric | Week 1 | Week 4 | Month 3 |
|--------|--------|--------|---------|
| GitHub stars | current | +50 | +500 |
| crates.io downloads | current | +100 | +1000 |
| PyPI downloads | 0 | first | +500 |
| HN upvotes | 0 | 20+ | — |
| GitHub issues from external users | 0 | 3+ | 20+ |
