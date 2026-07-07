# OG TestDesk — Rich Feature Design Doc (SQL Workspace + Requests)

## Context

The current `sql_tab`/`requests_tab` in OG TestDesk are intentionally bare
MVPs — they proved the iced architecture and the `core` API surface work,
but they're nowhere near what OG-devDesk (the original web/Tauri version)
actually did, and nowhere near what would make OG TestDesk a real
replacement for **Postico** (SQL) and **Postman** (API client).

This doc catalogs what the original app did (backend + visual/UX), compares
that against what Postico and Postman do well, and lays out a concrete,
phased plan for rebuilding both tools to that standard natively in iced —
including which parts are easy, which need custom widget work, and which
community crates close the gap.

Research basis: full read-through of `OG-devDesk`'s `src/pages/sql/*.rs`,
`src/pages/request.rs`, `static/sql.js` (~5000 lines), `static/sql_connections.js`,
`static/requests.js` (~3200 lines), and their CSS, cross-referenced against
Postico 2 and Postman's known feature sets.

## Guiding principles

1. **Reuse `core` aggressively.** Nearly all backend logic (query execution,
   schema introspection, connection pooling, request proxying, Postman
   import, saved-item CRUD) is already ported and framework-agnostic. This
   is a UI rebuild, not a backend rebuild — call `core::sql::engine` /
   `core::requests` directly, extend them only where a genuinely new
   capability is needed (e.g. per-column sort, FK-click navigation).
2. **Don't reimplement what the ecosystem already solved — but verify the
   `iced_core` version actually matches, not just that `cargo add`
   succeeds.** `iced_table` targets our `iced = "0.13"` correctly and is
   the answer for the data grid. `iced_split`/`iced-resizable-split` *look*
   compatible (they add and even `cargo check` clean, since two `iced_core`
   versions can coexist in the dependency graph) but actually depend on
   `iced_core 0.14` — their widgets produce a different `Element` type than
   ours and can't be used in `view()`. Confirmed during Phase 1: the
   resizable editor/results split was hand-rolled instead (`mouse_area` +
   a raw `CursorMoved`/`ButtonReleased` subscription). Don't re-attempt the
   split crates unless the project's `iced` dependency is bumped past 0.13.
   **Same finding for `iced_aw`** during Requests Phase 1: `cargo add
   iced_aw --features tabs,tab_bar` and `cargo check` both succeed, but an
   actual `iced_aw::widget::TabBar` value fails to convert into this
   project's `Element<'_, Message>` with `E0277` — rustc names it
   explicitly: multiple `iced_core` versions in the graph. Tabs were
   hand-rolled (button row + active-index enum, same pattern as the
   top-level app nav). Treat every other `iced_aw` widget referenced below
   (`context_menu`, `menu`, `drop_down`, `date_picker`, `selection_list`)
   as **unconfirmed** until someone does the same real-compile smoke test
   — don't assume the crate works just because one widget from it doesn't;
   equally don't assume the rest works either. Test before building on it.
3. **Ship in vertical slices.** Each phase below should leave the app in a
   runnable, demoable state — mirrors how the MVP tabs were built.
4. **Close real gaps, not just parity.** Both research passes found things
   the *original* app never had that Postico/Postman users expect (NULL
   rendering, status-code color coding, keyboard shortcuts, env scoping).
   Fold those in as part of the rebuild, not as a separate future pass.

---

## Part 1 — SQL Workspace (target: Postico-class)

### 1.1 Feature set to (re)build

**Connections**
- Add/edit/delete, Postgres vs SQLite conditional fields, SQLite "create
  new file" flow, encrypted secrets (already in `core::sql::crypto`),
  connected/disconnected status indicator, explicit disconnect action.

**Query editor**
- Multi-line editor (`iced::widget::text_editor`, not `text_input` —
  already a known constraint from the MVP).
- **Real syntax highlighting** via iced's `Highlighter` trait: keyword /
  string / number / comment tokens, carry over the original's Dracula-ish
  palette (pink keywords, yellow strings, purple numbers, blue-gray
  comments) as the default theme's syntax colors.
- Multi-statement execution: split on statement boundaries, run
  sequentially, render each as its own stacked result panel (not a single
  flat table) — matches original behavior, keep it.
- `{{variable}}` scanning with per-variable inputs **and a format mode**
  (raw / list / array — controls how multi-line/space-separated pasted
  values get joined for `IN (...)` / `ARRAY[...]`). This was a genuinely
  good original feature — keep the format-dropdown-per-variable UX.
- Autocomplete overlay keyed off table aliases in the current SQL (iced
  `overlay`/`stack` composition — no native combobox widget, build it as a
  positioned popup under the cursor).
- Find-in-editor (match count, prev/next).
- Comment-toggle shortcut (`Cmd/Ctrl+/`), run shortcut (`Cmd/Ctrl+Enter`).

**Results grid — build on `iced_table`**
- Sticky header, per-column resize, column-visibility toggle menu, "fit
  columns" action.
- Cell selection (click, drag-range, keyboard nav), copy as tab-delimited.
- Inline cell editing → diff against original values → generate UPDATE
  statements via `core::sql::engine::build_table_update_sql` (already
  exists) → run → append to history.
- Add-row flow → `build_table_insert_sql` (already exists).
- **New vs original:** clickable column-header sort, distinct NULL
  rendering (gray "NULL" pill, not blank), type-aware cell editors
  (checkbox for bool, date picker for timestamp — `iced_aw::date_picker`
  if it turns out compatible, unconfirmed, see Guiding Principles),
  click a FK cell to jump to the referenced row
  inline (not just in the separate ERD view).

**Schema browser**
- Tree sidebar: tables → columns, PK/FK markers, click to browse a table.
- Function/view list, click to load definition into the editor.

**Relationship diagram (ERD)**
- Build with `iced::widget::canvas`: table cards positioned by a simple
  layout pass, bezier connector curves between FK/PK points (canvas paths
  are a very natural fit here — likely ends up cleaner than the original's
  DOM+SVG approach), click a table to focus, click an FK column to jump,
  live search/filter over tables and columns.
- Render inside a floating iced window (multi-window support) or an
  in-window modal overlay — see §3.4 for the shared modal decision.

**Saved queries**
- Nested folder tree, rename/move, search/filter.
- v1: **move-to-folder via menu** instead of drag-and-drop (iced 0.13 has
  no native DnD-list primitive; original's hand-rolled `dataTransfer` drag
  logic would need a from-scratch pointer-tracking reimplementation —
  defer true drag-and-drop to a later polish pass, ship move-menu first).
- Export/import as JSON bundle (`core::sql::engine::export_queries` /
  `import_queries` already exist).

**Run history**
- Every run (manual, background job, table edit, cron) recorded — backend
  already does this (`SqlRunHistoryRecord`).
- **New vs original:** replace the tiny `<select>` dropdown with a real
  searchable/filterable list (the original's UX gap, not just a port).

**Scheduled queries (cron) + alerts**
- Named interval tasks with live SQL preview, alert rule (comparator +
  threshold against row count) that flags a run.
- This is the lowest-priority feature in the whole doc — defer to last.

**Export**
- CSV export of current result set, selection-aware (all vs
  selected rows/columns). `core::sql::engine::export_results_csv` exists.

**Timezone indicator** — small status pill, already backed by
`fetch_timezone`.

### 1.2 New capabilities beyond the original (Postico-parity gaps)

From the research's gap list — build these in, don't treat as optional:
- SSH tunnel support for remote DB connections (new, not in `core` yet —
  needs a tunnel-establishing step before the `sqlx` pool connects; scope
  as its own small design pass when reached, likely via the `russh` or
  shelling out to system `ssh -L`).
- Type-aware cell editors and NULL-vs-empty visual distinction.
- Inline FK-click navigation in the results grid (not just the ERD modal).
- Column-header click-to-sort.
- Connection favorites/grouping beyond a flat list.
- EXPLAIN/query-plan visualization — nice-to-have, low priority.

### 1.3 Suggested phase breakdown

1. **Data grid + highlighted editor**: swap the MVP's plain text/table for
   `text_editor` + custom `Highlighter` + `iced_table` results grid
   (column resize, sort, NULL rendering). This alone is the single biggest
   visible upgrade.
2. **Schema browser + table browse/edit**: tree sidebar, click-to-browse,
   inline cell editing → UPDATE/INSERT, FK-click navigation.
3. **Saved queries + folders + run history**: tree UI, move-menu
   reorganization, searchable history list.
4. **Variables bar + autocomplete + find-in-editor**: the editor-adjacent
   power-user features.
5. **Relationship diagram (canvas ERD)**.
6. **Cron/alerts + CSV export + timezone pill**: the remaining backend-ready
   features that are lower priority UX-wise.
7. **Postico-gap closers**: SSH tunnels, type-aware editors, connection
   favorites, EXPLAIN viewer — pick up as time allows, not blocking.

---

## Part 2 — Requests (target: Postman-class)

### 2.1 Feature set to (re)build

**Multi-tab request workspace**
- Each open request is its own tab with independent state (method, URL,
  params, headers, body, auth). `iced_aw`'s tabs turned out incompatible
  (see Guiding Principles) — built with hand-rolled tab chrome (button row
  + active-index state) instead, done in Phase 1.
- Unsaved-changes indicator per tab.

**Request builder tabs** (Params / Path / Auth / Headers / Body / Curl)
- **Params**: key/value row editor, bidirectionally synced with the URL
  bar (editing a row rewrites the URL query string and vice versa).
- **Path variables**: auto-detect `{token}` patterns in the URL path,
  render one input per token, block send with a clear message if any are
  empty. Keep this as a mechanism distinct from `{{env_var}}` substitution
  — the original correctly kept these separate; preserve that.
- **Auth**: type selector (None/Bearer/Basic/API Key/OAuth2) swapping the
  field set below it; OAuth2 "fetch token" flow (call token URL with
  client credentials, store the fetched token for reuse).
- **Headers**: generic key/value row editor with enable/disable per row
  (build this as one reusable component — see §3.2 — since Params reuses
  the identical interaction pattern).
- **Body**: mode switch (raw / form-data / x-www-form-urlencoded / binary
  / GraphQL). Raw and GraphQL panels get **real syntax highlighting** this
  time (JSON/GraphQL highlighter via the same `Highlighter` framework as
  the SQL editor — this was the original's biggest visible weakness, a
  plain unstyled textarea). "Format JSON" button stays.
- **Curl import/export**: paste-a-curl-command parser populates the
  builder; "View Curl" reconstructs the equivalent command from the
  current builder state. Keep both directions.

**Environments / variables**
- Named variable sets, one active at a time, `{{var}}` substitution into
  URL/headers/body at send time, unresolved-variable names block send with
  an explicit list.
- **New vs original**: add real **scoping** (global → environment →
  request-local override), which the original never had — this is one of
  the clearest Postman-parity gaps and is pure data-model work in `core`,
  not UI-hard.

**Response viewer**
- Status line with **graduated color coding** (2xx green, 3xx blue/gray,
  4xx orange, 5xx red) — replaces the original's binary success/fail
  styling, a clear improvement.
- Headers tab, body tab (pretty-printed JSON via the same logic as the
  Inspector tab — share code, don't duplicate), time/size display.
- Resizable split between request builder and response (hand-rolled
  divider, see §3.1), and between response headers/body panels.
- "Open in Inspector" cross-tool action (hand the response body to the
  Inspector tab's state).

**Collections sidebar**
- Nested folder tree of saved requests, search/filter box.
- v1: move-to-folder via menu (same DnD deferral rationale as SQL saved
  queries — see §1.3 step 3).
- Rename/delete/duplicate via context menu (`iced_aw::context_menu` if
  confirmed compatible when this phase is reached, else inline buttons).

**History**
- **New vs original**: raise the cap past 12 entries and present it as a
  searchable list, not a `<select>` dropdown — the original's history UX
  was its weakest saved-state feature.

**Postman import** — backend (`core::requests::import_postman_collection`)
is complete; build the file-picker → preview-counts → duplicate-mode
selector → import → warnings-display flow.

**Keyboard shortcuts** — `Cmd/Ctrl+Enter` to send, `Cmd/Ctrl+S` to save.
The original had **none** of these; add them from the start.

### 2.2 New capabilities beyond the original (Postman-parity gaps)

- Environment scoping chain (global/environment/local) — see above.
- Code-generation snippets (curl exists; add at least one or two more,
  e.g. a plain Rust `reqwest` snippet, low effort high value for this
  project's audience).
- Cookie jar / cookie inspector tab on the response side.
- Response diffing between two history entries — nice-to-have, defer.
- Request chaining (use a prior response value as a later request's
  input) — nice-to-have, defer, real design work when reached.

### 2.3 Suggested phase breakdown

1. **Multi-tab workspace + builder tabs** (Params/Path/Auth/Headers/Body) —
   done: shared key/value row component (`request_kv_editor.rs`) plus
   hand-rolled tabs (`iced_aw` confirmed incompatible, see Guiding
   Principles).
2. **Syntax-highlighted body editor + response viewer** with graduated
   status coloring and a hand-rolled resizable split (see §3.1).
3. **Environments with real scoping** + `{{var}}` substitution.
4. **Collections sidebar** (folders, search, context-menu actions) +
   history as a searchable list.
5. **Curl import/export + Postman import UI** (backend-ready, UI work
   only) + keyboard shortcuts.
6. **Postman-gap closers**: code-gen snippets, cookie jar; response
   diffing and request chaining as stretch goals.

---

## Part 3 — Shared infrastructure (build once, use in both tools)

These come up in both Part 1 and Part 2 — build them as standalone
components so neither tool reinvents the other's solution.

### 3.1 Resizable split panes
`iced_split`/`iced-resizable-split` do not actually target `iced 0.13`
(see the note in Guiding Principles) — hand-roll instead: a thin draggable
divider (`mouse_area` + a window-level `CursorMoved`/`ButtonReleased`
subscription converting drag delta into a split-ratio state, applied via
`Length::FillPortion`). Built once in SQL Phase 1 (`sql_tab.rs`'s
editor/results split) — extract it into a shared component before reusing
for: SQL sidebar sections, Requests builder/response split, Requests
headers/body split.

### 3.2 Key/value row editor
One component (add row / remove row / enable-disable checkbox per row),
used for: Requests headers, Requests params, Requests form-data fields,
(and reusable later for any future header-like list). Build it generic
over a `Vec<(bool, String, String)>` state shape.

### 3.3 Syntax highlighting framework
A small `Highlighter` implementation per language (SQL, JSON, GraphQL)
sharing a common tokenizer shape, feeding `iced::widget::text_editor`'s
highlighting API. Build the framework once, add languages as needed.

### 3.4 Modal / floating-window pattern
Both tools need modals (connection add/edit, cron/alert editor, save-query
dialog, Postman-import flow, ERD diagram). Decide once: either (a) iced's
multi-window support (a real OS window per modal, closer to the original's
`<dialog>` semantics) or (b) an in-window overlay/stack-based modal. Given
this project already uses iced's async `Task` machinery and a single main
window today, **recommend starting with in-window overlays** (simpler
state management, no cross-window message routing) and revisiting
multi-window only if a specific case (e.g. the ERD diagram, which benefits
from being a large freely-resizable surface) demands it.

### 3.5 Data grid
Use the `iced_table` crate (confirmed compatible) for the SQL results grid.
Evaluate whether it's also a good fit for a tabular response-body view in
Requests (e.g. when a response is a JSON array of objects) as a later
enhancement — not required for parity, but a natural extension once the
grid component exists.

### 3.6 Searchable/filterable list component
Used by: SQL run history, SQL saved queries, Requests history, Requests
collections sidebar. Build one filterable-list component (text input +
live-filtered `column` of rows) rather than four bespoke ones.

---

## Part 4 — Technical foundation summary

Confirmed available and compatible with this project's `iced = "0.13"`:

| Need | Solution |
|---|---|
| Tabs (request tabs, SQL tab bar) | Hand-rolled (button row + active-index state) — `iced_aw::tabs`/`tab_bar` confirmed incompatible (`iced_core` version mismatch, same failure mode as `iced_split`) |
| Context menus (right-click actions) | `iced_aw::context_menu` — **unconfirmed**, test before use |
| Dropdown/combobox-style menus | `iced_aw::menu`, `iced_aw::drop_down` — **unconfirmed**, test before use |
| Date picker (timestamp cell editor) | `iced_aw::date_picker` — **unconfirmed**, test before use |
| Selection list | `iced_aw::selection_list` — **unconfirmed**, test before use |
| Resizable split panes | Hand-rolled (`mouse_area` + subscription) — `iced_split` targets `iced_core 0.14`, incompatible |
| Data grid (SQL results) | `iced_table` |
| Multi-line code editor | `iced::widget::text_editor` (built in) |
| Syntax highlighting | `iced`'s `Highlighter` trait on `text_editor` (custom impl per language) |
| ERD diagram / custom drawing | `iced::widget::canvas` (built in) |
| Multiple OS windows (if needed for modals) | `iced`'s multi-window feature (built in) |

No changes needed to `core` for most of this work — it's additive UI. The
exceptions called out above (SSH tunnels, environment scoping chain, code
snippet generation) need small, scoped `core` additions when their phase
is reached.

## Verification approach per phase

Same discipline as the MVP build: after each phase, `cargo check`, run the
app, exercise the new feature's golden path manually (e.g. phase 1 SQL:
open a connection, run a multi-statement query, confirm syntax highlighting
renders and the results grid sorts/resizes), and commit before moving to
the next phase.
