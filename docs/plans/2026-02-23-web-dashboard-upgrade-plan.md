# Web Dashboard Upgrade Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the web dashboard interactive (HTMX live search, inline editing), actionable (archive stale from analytics, quality issue links), and polished (system dark mode, mobile graph, accessibility).

**Architecture:** Add `hx-*` attributes to existing Askama templates. Extract partial templates for HTMX fragment responses. Detect `HX-Request` header server-side to return partials vs full pages. No new JS frameworks, no build step, no new Cargo dependencies.

**Tech Stack:** Axum, Askama, HTMX 2.0.4 (already loaded), Cytoscape.js, Chart.js

---

## Task 1: Live Search — Extract Partial Template

**Files:**
- Create: `crates/shabka-web/templates/partials/search_results.html`
- Modify: `crates/shabka-web/templates/search.html`
- Modify: `crates/shabka-web/src/routes/search.rs`

**What to do:**

1. Create `templates/partials/` directory and `search_results.html` — extract the results section from `search.html` (lines 25-88: the count line, empty state, result cards loop, and highlighting script) into a standalone partial template (no `{% extends %}`, just the fragment). The partial template struct needs `query: String`, `results: Vec<SearchResult>`.

2. In `search.html`, replace the extracted section with `{% include "partials/search_results.html" %}` wrapped in a `<div id="search-results">`.

3. Add HTMX attributes to the search input (line 16-17 of `search.html`):
   ```html
   hx-get="/search" hx-trigger="keyup changed delay:300ms, search"
   hx-target="#search-results" hx-push-url="true" hx-include="[name='project']"
   ```
   Keep the form's `action="/search"` for no-JS fallback.

4. In `search.rs`, detect the `HX-Request` header. If present, render only the `SearchResultsPartial` template. Otherwise render the full `SearchTemplate` as before. Use `axum::http::HeaderMap` in the handler signature.

**Verify:** `cargo test -p shabka-web --no-default-features` — all 26 tests pass. `cargo check -p shabka-web --no-default-features` compiles.

**Commit:** `feat(web): live search with HTMX partial rendering`

---

## Task 2: Inline Editing — Detail Page Field Edits

**Files:**
- Modify: `crates/shabka-web/templates/memories/detail.html`
- Modify: `crates/shabka-web/src/routes/api.rs`

**What to do:**

1. The existing `PUT /api/v1/memories/{id}` handler already accepts partial JSON updates. Add a `PATCH` route alias in `api.rs` routes (line ~43): `.route("/api/v1/memories/{id}", get(...).put(...).patch(update_memory).delete(...))`. This lets HTMX use `hx-patch`.

2. On the detail page title (`<h1>` at line 11), add click-to-edit behavior:
   - Display mode: `<h1 id="title-display" hx-get="/api/v1/memories/{{ memory.id }}/edit-field?field=title" hx-trigger="click" hx-target="this" hx-swap="outerHTML">{{ memory.title }}</h1>`
   - Create a new route `GET /api/v1/memories/{id}/edit-field?field=title` that returns an `<input>` element with `hx-patch="/api/v1/memories/{id}"` and `hx-target="this"` `hx-swap="outerHTML"` `hx-trigger="blur, keyup[key=='Enter']"` `hx-vals='js:{"title": event.target.value}'`. On success, the PATCH handler returns the display-mode HTML fragment.

3. Similarly for **tags** (comma input), **kind** (select dropdown), and **importance** (range slider). Each gets a `field` param variant in the edit-field route.

4. Add the `edit-field` route and a `patch-field` response handler in `api.rs`:
   - `GET /api/v1/memories/{id}/edit-field` — returns the HTML form element for the requested field
   - The existing `PATCH /api/v1/memories/{id}` handler should return HTML (not JSON) when `HX-Request` header is present — specifically the display-mode fragment for the updated field. Add `HX-Trigger: showToast` response header.

5. For **content** (markdown body at line 51): click the `.content-body` div to swap in a `<textarea>` with a Save button. On save, `hx-patch` sends the new content. Response swaps back to rendered markdown.

**Verify:** `cargo test -p shabka-web --no-default-features` — all tests pass. Manual: start `just web`, navigate to a memory, click title to edit, press Enter, verify it saves.

**Commit:** `feat(web): inline editing on memory detail page via HTMX`

---

## Task 3: HTMX Verification Buttons & Delete Confirmation

**Files:**
- Modify: `crates/shabka-web/templates/memories/detail.html`

**What to do:**

1. Replace the three vanilla JS `onclick="setVerification('...')"` buttons (lines 37-39) with HTMX:
   ```html
   <button hx-put="/api/v1/memories/{{ memory.id }}"
           hx-vals='{"verification": "verified"}'
           hx-target=".verify-actions" hx-swap="outerHTML"
           hx-confirm="Mark this memory as verified?"
           class="btn btn-outline" style="...">Verify</button>
   ```
   The PUT handler returns the updated button group HTML fragment when `HX-Request` is present.

2. Replace the delete form's JS `addEventListener` (lines 64-68) with:
   ```html
   <button hx-delete="/api/v1/memories/{{ memory.id }}"
           hx-confirm="Permanently delete this memory? This cannot be undone."
           hx-headers='{"HX-Redirect": "/"}'
           class="btn btn-danger">Delete</button>
   ```
   The DELETE handler should set `HX-Redirect: /?toast=Memory%20deleted` response header.

3. Remove the `setVerification()` and delete-form JS functions that are now replaced by HTMX.

**Verify:** `cargo test -p shabka-web --no-default-features`. Manual test: click Verify button, confirm dialog appears, status updates without page reload.

**Commit:** `feat(web): HTMX verification buttons and delete confirmation`

---

## Task 4: Actionable Analytics — Archive Stale

**Files:**
- Modify: `crates/shabka-web/templates/analytics.html`
- Modify: `crates/shabka-web/src/routes/analytics.rs`

**What to do:**

1. Add a new route `POST /analytics/archive-stale` in `analytics.rs`:
   - Run the same stale detection logic (memories where `days_inactive >= stale_threshold`)
   - For each stale memory, call `storage.update_memory(id, &UpdateMemoryInput { status: Some(MemoryStatus::Archived), ..Default::default() })` and log a history event
   - Return an HTML fragment: the updated stale count card showing 0 stale, with a success toast via `HX-Trigger: {"showToast": {"message": "Archived N memories", "type": "success"}}`

2. In `analytics.html`, wrap the stale count card (lines 36-39) in a `<div id="stale-card">`. Add an "Archive All" button below the stale count that only appears when `stale_count > 0`:
   ```html
   {% if stale_count > 0 %}
   <button hx-post="/analytics/archive-stale" hx-target="#stale-card" hx-swap="innerHTML"
           hx-confirm="Archive {{ stale_count }} stale memories?"
           class="btn btn-outline" style="font-size:0.75rem;margin-top:0.35rem">
     Archive All Stale
   </button>
   {% endif %}
   ```

3. Extract the stale card inner HTML into a reusable block or just inline both display states.

**Verify:** `cargo test -p shabka-web --no-default-features`. Add a test `test_archive_stale_endpoint` that POSTs to `/analytics/archive-stale` and verifies 200.

**Commit:** `feat(web): archive stale memories from analytics page`

---

## Task 5: Quality Issue Links

**Files:**
- Modify: `crates/shabka-web/templates/analytics.html`
- Modify: `crates/shabka-web/src/routes/memories.rs`

**What to do:**

1. Make each quality issue row in `analytics.html` (lines 70-77) a clickable link. Each row links to the memory list filtered by that issue type:
   ```html
   <a href="/?quality=generic_title" style="...">Generic titles <span>{{ quality_counts.generic_titles }}</span></a>
   ```
   Map: `generic_title`, `short_content`, `no_tags`, `low_importance`, `stale`, `orphaned`, `low_trust`.

2. In `memories.rs`, add `quality: Option<String>` to `ListParams` (line 104-109).

3. In `list_memories` handler, after fetching memories, if `quality` param is set:
   - Run `assess::analyze_memory()` on each memory (same as analytics does)
   - Filter to only memories that have the matching issue label
   - This reuses the existing `assess` module — no new logic needed

4. Similarly, make the "top issues" rows (lines 82-86) link to `/memories/{id}` for each memory.

**Verify:** `cargo test -p shabka-web --no-default-features`. Manual: click "Generic titles (5)" on analytics → see filtered list.

**Commit:** `feat(web): link quality issues to filtered memory list`

---

## Task 6: System Dark Mode Detection

**Files:**
- Modify: `crates/shabka-web/templates/base.html`

**What to do:**

1. Add `prefers-color-scheme` media queries in the `<style>` block (after the existing `.light` ruleset around line 26). The logic: if no manual override (`data-theme` attribute), follow system preference. If user has toggled, the `data-theme` attribute takes priority.

2. Update the theme toggle JS (currently at the bottom of base.html) to set `data-theme` on `<html>` element and store in localStorage. On page load, check localStorage first, then fall back to system preference.

3. Current approach: `html.light` class toggles light mode. Change to `html[data-theme="light"]` selector for explicit override. Add:
   ```css
   @media (prefers-color-scheme: light) {
     html:not([data-theme="dark"]) {
       /* same light mode variables */
     }
   }
   ```

**Verify:** Manual: remove localStorage theme, check that system dark/light is respected. Toggle manually, verify override persists.

**Commit:** `feat(web): system dark mode detection with manual override`

---

## Task 7: Mobile Graph Page

**Files:**
- Modify: `crates/shabka-web/templates/graph.html`

**What to do:**

1. Add a CSS media query `@media (max-width: 768px)` in the graph page's `<style>` block:
   - Filter sidebar: `display: none` by default, toggled via a hamburger button
   - Graph canvas: `width: 100%` instead of `calc(100% - sidebar)`
   - Detail sidebar: position as a bottom sheet (`position: fixed; bottom: 0; left: 0; right: 0; max-height: 50vh; overflow-y: auto; transform: translateY(100%)`) with slide-up animation when a node is selected

2. Add a filter toggle button (hamburger icon) that shows only on mobile:
   ```html
   <button id="filter-toggle" style="display:none" onclick="toggleFilters()">☰ Filters</button>
   ```
   JS: toggle `display` on filter sidebar. CSS media query shows the button on mobile.

3. Adjust the detail sidebar close behavior: on mobile, swipe down or tap outside to dismiss.

**Verify:** Manual: resize browser to < 768px, verify sidebar collapses, filter toggle works, node click opens bottom sheet.

**Commit:** `feat(web): responsive graph page with mobile bottom sheet`

---

## Task 8: Accessibility Quick Wins

**Files:**
- Modify: `crates/shabka-web/templates/base.html`

**What to do:**

1. Add skip-to-content link as first element in `<body>`:
   ```html
   <a href="#main-content" class="skip-link">Skip to content</a>
   ```
   CSS: visually hidden until focused (`:focus` shows it).

2. Add `id="main-content"` to the `<main>` element.

3. Add `role="search"` to search forms in the nav bar and search page.

4. Add `aria-label` attributes to:
   - Nav links ("Memories", "Timeline", "Graph", "Analytics")
   - Theme toggle button ("Toggle dark/light theme")
   - Search input ("Search memories")

5. Add `aria-live="polite"` to the toast container div so screen readers announce notifications.

6. Add CSS for `:focus-visible` outlines:
   ```css
   :focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
   ```

**Verify:** Manual: Tab through the page, verify focus rings visible. Screen reader announces toasts.

**Commit:** `feat(web): accessibility improvements (skip-link, ARIA labels, focus rings)`

---

## Task 9: Tests for New Endpoints

**Files:**
- Modify: `crates/shabka-web/src/routes/api.rs` (test module)

**What to do:**

Add tests for the new functionality:

1. `test_search_partial_htmx` — Send GET `/search?q=test` with `HX-Request: true` header. Verify response does NOT contain `<!doctype html>` (it's a partial, not full page).

2. `test_search_full_page` — Send GET `/search?q=test` without HX-Request header. Verify response contains `<!doctype html>`.

3. `test_patch_memory` — Create a memory, send PATCH `/api/v1/memories/{id}` with `{"title": "New Title"}`. Verify 200 and title updated.

4. `test_archive_stale` — POST `/analytics/archive-stale`. Verify 200.

5. `test_list_quality_filter` — GET `/?quality=generic_title`. Verify 200 (HTML page loads, even if no results).

**Verify:** `cargo test -p shabka-web --no-default-features` — all tests pass including new ones.

**Commit:** `test(web): add tests for HTMX partials, PATCH, and new endpoints`

---

## Verification

```bash
cargo check --workspace --no-default-features
cargo test -p shabka-web --no-default-features
cargo clippy --workspace --no-default-features -- -D warnings

# Manual E2E
just web
# Open http://localhost:37737
# Test: type in search bar → results update live
# Test: click memory title → edit in place → press Enter → saves
# Test: click Verify button → confirms → status updates
# Test: Analytics → Archive All Stale → count drops to 0
# Test: Analytics → click "Generic titles" → filtered list
# Test: Resize to mobile → graph sidebar collapses
# Test: Tab through page → focus rings visible
```
