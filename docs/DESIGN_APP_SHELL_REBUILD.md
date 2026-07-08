# OG TestDesk — App Shell & SQL Layout Redesign

## Context

The rich-feature rebuild (`docs/DESIGN_RICH_REBUILD.md`) made SQL and
Requests functionally deep, but the app shell itself is still the original
Phase-1 scaffold: a flat row of 8 nav buttons, one tab body at a time, no
visual hierarchy. The user provided a written UI spec (reproduced/organized
below) describing how the whole app should actually look and flow. This
doc translates that spec into concrete iced mechanics and a phased build
plan, the same way `DESIGN_RICH_REBUILD.md` did for SQL/Requests features.

Decision locked in with the user: the four "floating window" tools
(Appearance, JWT Decoder, AI Assistant, Scratch Pad) must support **both**
real OS windows and in-app floating panels, switchable via a setting — not
a one-time architectural choice.

## Part 1 — Top nav carousel

Three fixed sections: **SQL**, **Requests**, **Inspector**. Each section
holds its own set of tab groups:
- **SQL**: one tab *group* per open database connection (a connection can
  have multiple query tabs inside its group — mirrors how a real DB client
  lets you have several query windows open against the same connection).
- **Requests**: flat tabs, no grouping (matches what's already built in
  `requests_tab.rs`'s multi-tab workspace).
- **Inspector**: flat tabs (currently single-instance — extend to
  multi-tab to match, low effort since it's already a simple text-in/
  pretty-out tool).

Hovering the active section's tab strip reveals a "+" to add a new tab (in
SQL's case, a new tab within the currently-open connection's group; a
*different* action — opening a whole new connection — happens from the SQL
connection-manager landing screen, not from the "+").

**iced mechanics**: this is a hand-rolled tab strip (per the established
precedent — `iced_aw`'s tabs are confirmed incompatible, see
`DESIGN_RICH_REBUILD.md`'s Guiding Principles). "Carousel" in the user's
spec means a horizontally-scrollable strip of tab groups, not a literal
image carousel — implement as a `scrollable` `row` of group headers, each
expandable to show its tabs underneath or inline.

## Part 2 — Top-right nav cluster (not part of the carousel)

Three fixed controls, right-aligned on the same nav row:

1. **Notifications bell** — shows a badge count. Click opens a popup
   listing finished SQL runs and finished Requests sends, each clickable
   to jump straight to that run's tab. Source data: SQL already has a run
   history mechanism (`sql_tab.rs`'s history panel, Phase 3) and Requests
   has one too (`request_history.rs`, Phase 4) — this notification popup
   is a *new, small, unread-aware view* over data that already exists, not
   a new persistence layer. Track "seen" state in memory (reset per
   session is fine — this is a session-scoped notification tray, not a
   durable inbox).
2. **`?` documentation button** — opens a short in-app help view (or a
   floating window/panel, same toggle as the other tools) explaining how
   to use the app. Content is static text this project already loosely
   has precedent for (the original OG-devDesk had an embedded
   "Documentation" panel in its nav shell) — write fresh, concise content
   covering each of the three main sections.
3. **Hamburger menu** — a dropdown/menu revealing: Appearance, JWT
   Decoder, AI Assistant, Scratch Pad. Each opens per the window/panel
   toggle described in Part 3. All four tools already exist and work
   (`appearance_tab.rs`, `jwt_tab.rs`, `ai_tab.rs`, `scratchpad_tab.rs`
   from the earlier build) — this phase is about *how they're presented*,
   not rebuilding their internals.

## Part 3 — Window/Panel toggle for floating tools

A new setting (persist via `app_db`, same generic `get_json`/`put_json`
pattern used elsewhere — e.g. collection `"settings"`, key
`"window_mode"`) with two values:

- **Native**: opening a tool from the hamburger menu calls
  `iced::window::open(window::Settings { .. })`, which returns a
  `(window::Id, Task<window::Id>)`. Track a `HashMap<window::Id, ToolKind>`
  in `App` state. The top-level `view` function becomes multi-window-aware
  (iced 0.13 supports this natively — a `view(&self, window: window::Id)`
  signature, confirmed via `iced::window::open`/`close` existing in
  `iced_runtime` — the closure-based `iced::application(...)` builder
  dispatches by inspecting the view function's arity, no separate
  "daemon" API needed for this level of multi-window use). The main window
  renders the nav+section content as today; any other tracked window
  renders that tool's `view()` directly, undecorated by the main nav.
  Closing one of these windows (`window::close_requests()` subscription,
  already precedented by nothing yet in this codebase but a
  straightforward addition) removes it from the tracking map.
- **Panel**: opening a tool instead toggles an `open: bool` + a
  `position: (f32, f32)` in `App` state per tool, rendered as a draggable
  overlay **within** the main window using `iced::widget::stack` (layers
  the panel content on top of the main view — check exact API name/import
  in iced 0.13's widget module before relying on it) positioned via the
  stored `position`. Dragging: same `mouse_area` + window-level
  `CursorMoved`/`ButtonReleased` subscription pattern already used four
  times in this codebase (`sql_tab.rs`'s split, `requests_tab.rs`'s split)
  — apply it to panel position instead of a split ratio.

Build this as a small reusable wrapper so all four tools share the same
open/close/drag machinery rather than four bespoke implementations.

## Part 4 — SQL connection landing screen

Currently, selecting the SQL section immediately shows the query editor
against whatever connection happens to be selected in a flat sidebar list.
Per the spec, add a **separate landing screen**, shown when the SQL
section has no open connection tab group yet (or via an explicit "Manage
connections" action once tab groups exist):

- List of saved connections (nickname, type, status).
- "Add connection" (already exists in `sql_tab.rs`, relocate here).
- "Create SQLite database" — a variant of add-connection specifically for
  creating a brand-new local `.sqlite` file rather than pointing at an
  existing one (native save-dialog via `rfd`, already a dependency,
  precedented by the CSV-export flow in `sql_tab.rs`).
- "Edit" a saved connection (currently there's add/delete but no edit —
  `core::sql::engine::update_connection` already exists and is unused by
  the UI, check its signature in `core/src/sql/engine.rs`).
- Clicking "Open" on a connection creates a new tab group in the SQL
  section's carousel (Part 1) and switches into the main SQL editor
  (Part 5) for that connection.

## Part 5 — Main SQL editor layout

### Left sidebar, three fixed sections top-to-bottom (currently the
sidebar is a flatter stack of connections+schema+saved-queries+history in
whatever order Phases 1-6 accumulated them — this phase is a *reorder and
regroup* of mostly-existing pieces, not new backend work, plus a few real
additions called out below):

1. **Tables & Functions** (top): the existing schema tree
   (`fetch_relationship_schema`-backed, from SQL Phase 2) plus the
   function list (`core::sql::engine::fetch_function_list` — check if this
   is already wired into any UI; if not, this phase adds it: a simple list
   of functions, click to load a function's definition into the editor).
   Add an **"Open relationships"** button here that switches to the
   existing ERD diagram view (`sql_erd.rs`, SQL Phase 5) — that feature
   already exists, this just adds the entry point the spec describes.
2. **Saved queries** (middle): the existing folder tree (SQL Phase 3) plus
   two real additions:
   - **Drag-and-drop import/export with the OS file system**: dropping a
     `.sql` file (or a small JSON bundle, matching `export_queries`'s
     existing format) from the OS file manager onto this panel imports it;
     dragging a saved query out of this panel onto the OS desktop/file
     manager exports it as a file. Check what OS-level drag-and-drop
     support iced 0.13 actually has (`iced::event::Event::Window` file-drop
     variants, or check `iced_winit`'s window event handling for a
     `HoveredFile`/`DroppedFile`-equivalent) before committing to this —
     if genuine OS drag-and-drop turns out unsupported by iced 0.13, an
     explicit "Import from file" / "Export to file" button pair (native
     `rfd` dialogs) is an acceptable, clearly-flagged fallback.
   - Rename/delete buttons per saved query (rename already exists from
     Phase 3 as an inline input — check if a discrete "rename" *button*
     matches the spec's expectation better than the current always-visible
     input, adjust if so).
3. **History** (bottom): the existing history panel (SQL Phase 3) — delete
   per-item and clear-all already exist, just needs to be the bottom
   section in this new fixed three-part order.

### Main editor area

- The syntax-highlighted `text_editor` (SQL Phase 1) with **autocomplete**
  (SQL Phase 4, already exists) — no functional change, just confirm it
  lands in the right visual position under this reorg.
- **Below** the editor: a timezone indicator (SQL Phase 6, already
  exists — relocate if needed), a **Schedule** button opening the existing
  cron-task editor (SQL Phase 6, already exists — the spec just wants a
  dedicated "Schedule" entry point next to the editor rather than wherever
  it currently lives).
- A button row: **Run query**, **Save query**, **Save to file** (Run and
  Save already exist; "Save to file" is new — a native `rfd` save dialog
  writing the current editor SQL text to a `.sql` file on disk, trivial
  addition).

### Output area

A toolbar above the results grid with, left to right:
- **Filter/search bar** — live-filters visible rows (client-side, over the
  already-fetched result set — new addition, doesn't need a `core` call).
- **Columns button** — dropdown/popup with a checkbox per column to
  show/hide it in the grid (new addition to `sql_grid.rs`'s `ResultsGrid`
  — add a `visible: bool` per `GridColumn` and skip hidden ones in
  `view()`).
- **Widen columns button** — auto-fits every visible column's width to its
  longest cell value (new addition — compute max content length per
  column from the current row data, set `GridColumn.width` accordingly).
- **Export button** — CSV export already exists (SQL Phase 6), just needs
  to be relocated into this toolbar if it isn't already there.
- **Revert button** — discards any in-progress inline cell edits (table
  browse/edit already exists from SQL Phase 2 with a "Discard changes"
  action — confirm it's surfaced in this toolbar rather than wherever it
  currently sits, or is genuinely a distinct concept for *query-result*
  inline editing vs *table-browse* inline editing; clarify and implement
  whichever the current code actually supports).
- **Running queries dropdown** — the background/streaming job list
  (`core::sql::engine::list_jobs`/`get_job`/`get_job_delta` — these exist
  in `core` but were explicitly marked out-of-scope during SQL Phase 1-6
  in favor of the simpler synchronous `execute_sql` path; this phase is
  the first real UI consumer of the background-job API, or, if job
  tracking isn't wired to the "Run query" button at all yet, this
  dropdown can start as a simple "currently nothing runs in the
  background" placeholder with a note that wiring `run_background` in is
  a further follow-up, not blocking this layout phase).

## Suggested phase breakdown

1. **App shell**: nav carousel (3 sections + tab groups), notifications
   bell + popup, `?` docs button, hamburger menu skeleton (buttons that
   don't yet toggle window/panel mode — just prove the nav layout).
2. **Window/Panel toggle**: the settings flag, the shared open/close/drag
   wrapper, wire all four hamburger tools through it in both modes.
3. **SQL connection landing screen**: separate from the main editor,
   add/edit/create-sqlite/open flow feeding into new tab groups.
4. **SQL sidebar reorg**: three fixed sections in the specified order,
   function list, "Open relationships" entry point, saved-query rename/
   delete buttons, drag-and-drop (or button-fallback) import/export.
5. **SQL editor/output layout**: relocate timezone/schedule/run/save
   buttons per spec, add "Save to file", build the output toolbar (filter,
   columns dropdown, widen, export relocation, revert, running-queries
   dropdown).

## Verification approach per phase

Same discipline as the rich-feature rebuild: after each phase, `cargo
check`, run the app, exercise the new layout/interaction's golden path
manually, commit before moving to the next phase. Where iced 0.13's actual
capability for something in the spec (OS-level drag-and-drop, multi-window
dispatch) is uncertain, **prove it with a minimal standalone smoke test
before building the full feature on top of it** — the same lesson learned
twice already with `iced_split` and `iced_aw`.
