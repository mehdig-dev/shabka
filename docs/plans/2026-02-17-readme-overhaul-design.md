# README Overhaul & Screenshots — Design

**Date:** 2026-02-17
**Goal:** Restructure README as a user-facing landing page with screenshots and value proposition. Move detailed docs to `docs/`.

## Context

Friend feedback:
- Screenshots/demo are the first thing people look for
- Explain the value add (team sharing, persistent memory)
- Split user-facing docs from developer docs
- Keep README short, link to deeper pages

## Decisions

- **Audience:** Users first. README is a storefront.
- **Approach:** Minimal landing page (~90 lines) + detailed docs in `docs/`.
- **Screenshots:** All major pages (6 screenshots via Playwright).
- **Dev docs location:** `docs/` directory with links from README.

## New README Structure (~90 lines)

1. Hero: logo + tagline + one-paragraph description
2. "Why Shabka?" — 4 value-prop bullets (persistent memory, team sharing, auto-capture, works everywhere)
3. Screenshots: 3x2 grid table with captions
4. Quick Start: 4 numbered steps (install HelixDB, start, register MCP, use it)
5. CLI highlight: 3-4 most common commands (not the full reference)
6. Documentation links table
7. License

## Documentation Split

| File | Content |
|------|---------|
| `docs/configuration.md` | Full TOML config reference, embedding providers, layered config |
| `docs/cli.md` | Complete CLI reference (all commands + flags) |
| `docs/web-dashboard.md` | Web features, REST API, page screenshots |
| `docs/api.md` | REST API endpoints, MCP tools table |
| `docs/development.md` | Dev commands, testing guide, resetting HelixDB, project structure |
| `docs/architecture.md` | Architecture diagram, workspace crates, module descriptions |

Each doc has a "Back to README" link at top.

## Screenshots

Captured via Playwright at 1280x800, saved to `docs/screenshots/`.

| Screenshot | Page | Shows |
|---|---|---|
| `dashboard.png` | `/` | Memory list with mixed kinds, pagination |
| `search.png` | `/search?q=authentication` | Search with highlighted terms |
| `detail.png` | `/memories/{id}` | Markdown content, relations, similar |
| `graph.png` | `/graph` | Knowledge graph with connected nodes |
| `analytics.png` | `/analytics` | Charts, quality gauge, stats |
| `dark-mode.png` | `/` (dark) | Dashboard in dark mode |

## Demo Data

~15 memories seeded via REST API before screenshots:
- Mix of kinds (decision, pattern, observation, error, fix, lesson)
- Realistic project context (auth, API, database, deployment)
- Varied tags, importance levels
- Several relations between memories (caused_by, fixes, related)

## Value Proposition Copy

```
LLMs forget everything between sessions. Shabka fixes that.

- Persistent memory — Decisions, patterns, and lessons survive across sessions.
- Team knowledge sharing — Share context with privacy controls.
- Zero-effort capture — Auto-captures from Claude Code via hooks.
- Works everywhere — MCP server for Claude/Cursor, plus CLI and web dashboard.
```
