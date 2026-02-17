# Web Dashboard

[← Back to README](../README.md)

```bash
just web   # Start on http://localhost:37737
```

## Features

- **Memory list** — Browse, filter by kind/project, bulk archive/delete, pagination
- **Memory detail** — Markdown rendering, relations, similar memories, audit history, chain explorer graph
- **Create/edit** — Kind descriptions, markdown hints, char counter, project ID field, styled sliders
- **Search** — Semantic + keyword search with ranked results and query term highlighting
- **Graph** — Interactive knowledge graph visualization (Cytoscape.js)
- **Analytics** — Memory distribution charts, creation trends, quality score gauge, contradiction count
- **Breadcrumb navigation** — Contextual breadcrumbs on all pages
- **Styled modals** — Confirmation dialogs and toast notifications replace browser alerts
- **Dark/light theme** — Toggle in navbar, persists across sessions
- **Keyboard shortcuts** — `Ctrl+K` or `/` to focus search
- **REST API** — Full JSON API at `/api/v1/` for external integrations

See [screenshots](screenshots/) for visual examples.

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
