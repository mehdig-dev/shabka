# Web Dashboard Upgrade — Design

## Goal

Make the web dashboard interactive, actionable, and mobile-friendly by wiring up the already-loaded HTMX, adding inline editing, making quality issues fixable from the analytics page, and polishing the responsive layout.

## Current State

- Server-rendered Askama templates, custom CSS (600+ lines), no frontend framework
- HTMX 2.0.4 loaded in `<head>` but zero `hx-*` attributes anywhere
- Vanilla JS for graph (Cytoscape.js), charts (Chart.js), markdown (marked.js)
- REST API v1 already exists for all CRUD operations
- Dark/light theme toggle (manual only, no system detection)
- Responsive breakpoints exist but graph/chain explorer struggles on mobile

## Architecture

No new dependencies. HTMX attributes on existing HTML elements, new Askama partial templates for HTMX responses, a few new API routes for partial HTML fragments. All interactivity stays server-rendered — no client-side JS framework.

---

## Priority 1: HTMX Interactivity

### Live Search

Convert the search form from full-page `GET /search` to in-place HTMX updates.

- Search input: `hx-get="/search" hx-trigger="keyup changed delay:300ms" hx-target="#search-results" hx-push-url="true"`
- Project filter: same HTMX trigger, included via `hx-include`
- Server returns a partial template (just the results `<div>`) when `HX-Request` header is present, or the full page otherwise
- Extract `search_results_partial.html` — the result cards loop and count line
- Keep the existing query highlighting JS — runs after HTMX swap via `htmx:afterSwap` event

### Inline Editing on Detail Page

Memory detail page (`/memories/{id}`) gains click-to-edit for:
- **Title** — click heading → `<input>` with `hx-patch="/api/v1/memories/{id}"` on blur/Enter
- **Content** — click content area → `<textarea>` with save button
- **Tags** — click tag area → comma-separated input with `hx-patch`
- **Kind** — click badge → `<select>` dropdown
- **Importance** — click percentage → slider input

Each field edits independently via `PATCH /api/v1/memories/{id}` (already exists as `PUT`, add `PATCH` alias). On success, HTMX swaps the field back to display mode. Toast notification via `HX-Trigger: showToast` response header.

### Verification Buttons

The detail page already has fetch-based verification buttons. Convert to HTMX:
- `hx-put="/api/v1/memories/{id}"` with `hx-vals='{"verification_status": "verified"}'`
- Swap the button group to show the new active state

### Delete Confirmation

Replace the vanilla JS confirm modal with HTMX:
- Delete button: `hx-delete="/api/v1/memories/{id}" hx-confirm="Delete this memory?"`
- On success: `HX-Redirect: /` header to return to list

---

## Priority 2: Actionable Analytics

### Archive Stale Memories

Add an "Archive All Stale" button next to the stale count card on the analytics page.

- Button: `hx-post="/api/v1/memories/bulk/archive"` with stale memory IDs
- New server route: `POST /api/v1/analytics/archive-stale` — runs the decay analysis, archives all stale memories, returns updated count
- After swap: stale count card updates to 0, toast shows "Archived N memories"

### Quality Issue Links

Each quality issue row (generic titles, short content, no tags, etc.) becomes a link:
- Click "Generic titles (5)" → navigates to `/search?quality=generic_title` or filters the list page
- Simpler approach: each row links to `/?quality_issue=generic_title` which filters the memory list to show only affected memories
- Requires: new query param on list handler, new filter logic using the `assess` module

### Contradiction Resolution

If `contradiction_count > 0`, show a "Review Contradictions" section below the quality panel:
- List contradiction pairs (memory A contradicts memory B) with links to both
- "Resolve" button for each pair → opens the detail page of the newer memory with the older one shown for comparison
- This is informational linking, not a new workflow engine — keep it simple

---

## Priority 3: Mobile + Polish

### System Dark Mode Detection

Add `prefers-color-scheme` media query in CSS:
```css
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) { /* dark variables */ }
}
@media (prefers-color-scheme: light) {
  :root:not([data-theme="dark"]) { /* light variables */ }
}
```
Manual toggle still overrides via `data-theme` attribute. First visit respects system preference.

### Graph Page Mobile

- Collapse filter sidebar by default on screens < 768px
- Add hamburger toggle button for the filter panel
- Graph canvas takes full width
- Detail sidebar opens as bottom sheet instead of right panel on mobile

### Accessibility Quick Wins

- Add `aria-label` to nav links, buttons, and interactive elements
- Skip-to-content link at top of `base.html`
- Focus ring styles for keyboard navigation (`:focus-visible` outlines)
- `role="search"` on search forms
- `aria-live="polite"` on toast container and search results (so screen readers announce updates)

---

## New Files

| File | Purpose |
|------|---------|
| `templates/partials/search_results.html` | Search results fragment for HTMX partial responses |
| `templates/partials/memory_field.html` | Inline edit/display toggle for a single memory field |
| `templates/partials/stale_card.html` | Updated stale count card for HTMX swap |

## Modified Files

| File | Changes |
|------|---------|
| `templates/base.html` | System dark mode CSS, skip-to-content, aria-live on toast container |
| `templates/search.html` | HTMX attributes on search form, include partial |
| `templates/memories/detail.html` | Inline edit attributes, HTMX verification buttons, delete confirm |
| `templates/analytics.html` | Archive stale button, quality issue links, contradiction section |
| `templates/graph.html` | Mobile responsive sidebar, bottom sheet |
| `src/routes/search.rs` | Detect `HX-Request` header, return partial or full page |
| `src/routes/analytics.rs` | New archive-stale handler, contradiction pairs query |
| `src/routes/memories.rs` | PATCH handler for inline field edits |
| `src/routes/api.rs` | HX-Trigger response headers for toast notifications |

## Testing

- Existing 26 web tests continue to pass (no behavioral changes to existing routes)
- New tests: archive-stale endpoint, PATCH field update, partial search response (HX-Request header)
- Manual testing: live search responsiveness, inline edit round-trip, mobile layout
