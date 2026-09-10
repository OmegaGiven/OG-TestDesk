# Design decisions

Context that doesn't show up in the code but shaped it. Read this before
changing the nav, color system, or driver architecture — these were
worked out over several iterations and the reasoning matters as much as
the result.

## Why a driver trait instead of per-DB UI branches

```rust
trait DbDriver {
    async fn test_connection(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<()>;
    async fn list_schemas(&self, cfg: &ConnConfig, password: Option<&str>) -> Result<Vec<Schema>>;
    async fn run_query(&self, cfg: &ConnConfig, password: Option<&str>, sql: &str) -> Result<QueryResult>;
}
```

Adding Mongo/Redis/whatever later should mean one new file in
`core/src/drivers/`, zero changes to `src-tauri` commands or the
frontend. If you find yourself writing `if kind == Postgres` anywhere
outside `drivers/mod.rs`'s `driver_for()` dispatcher, that's a sign the
abstraction is leaking — push the logic into the trait instead.

## Nav hierarchy — three levels, each with its own job

```
Tool (SQL / Requests / Inspector)
  └─ Connection (a saved DB, e.g. "PROD")       [SQL only — Requests/Inspector are flatter]
       └─ Tab (an open query / request / response)
```

Everything lives on **one horizontal line**, not stacked rows — the
whole point was minimizing vertical space eaten by chrome (see the
screenshots that drove this: the old app split "tool tabs" and
"connection tabs" onto two separate rows, which we deliberately moved
away from). If a redesign is tempted to add a second row back, that's
regressing a decision made after seeing it didn't work in practice.

Sections and their open tabs stay visible simultaneously (not just the
active section's tabs) — switching to Inspector shouldn't hide that SQL
still has PROD open with an unsaved query. Row scrolls horizontally
when it doesn't fit; it does not wrap.

## Color system — the actual spec

Three-tier, and the tiers mean different things:

1. **Tool hue** (fixed, one per tool, reused everywhere that tool's UI
   appears — not just the nav):
   - SQL → blue (`--tool-sql-tint` / `--tool-sql-text`)
   - Requests → green (`--tool-requests-tint` / `--tool-requests-text`)
   - Inspector → violet (`--tool-inspector-tint` / `--tool-inspector-text`)
2. **Connection accent** (SQL only, one dot color per saved connection,
   **should be user-assignable**, not auto-rotated by order). Rationale:
   if PROD's color could shift when you reorder tabs, the color stops
   being trustworthy as a "which DB am I about to run this against"
   signal — the whole reason it exists. This is not yet implemented
   (currently hardcoded in `+page.svelte` placeholder data); when built,
   store `color` on the `connections` table (already has the column) and
   expose a picker, not an algorithm.
3. **Tab tint** — inherits its parent connection's accent at reduced
   opacity when active (see `.tab.active` in `TopNav.svelte`, uses
   `color-mix()` against `--dot`).

Do not add a 4th ad-hoc color for some new element ("let's make errors
orange") without checking whether it collides with a connection's
existing dot color first — the whole system breaks if two unrelated
things share a hue.

## Inspector — spec for the not-yet-built UI

Reference target (from an existing internal tool, description only —
no assets to carry over):

- Three view modes: **Tree**, **Summary**, **Table** — segmented control,
  top right of the panel.
- Toolbar: Expand one level / Expand all / Collapse all, search box with
  live match count + prev/next navigation.
- Tree rows: expand/collapse chevron, key name (colored per tool hue —
  violet, since this is Inspector), type + size badge (muted, not
  competing with the value), value.
- **Selecting a row populates a right-side panel**: path (`$.foo.bar`
  style), type, size, a pretty-printed JSON snippet of just that
  subtree, and Copy path / Copy value / Copy pretty buttons.
- Search highlights matches inline in the tree, not just a result list.
- Feeds from **two sources**: SQL query results (structured rows — Table
  mode is probably primary there) and Requests responses (raw JSON —
  Tree mode primary). Same component either way; don't build two
  inspectors.

## What was explicitly rejected

- **iced (native Rust GUI)** — first attempt at this app. Dropped because
  styling iteration was too slow (Rust structs vs CSS). See `CLAUDE.md`
  history section — don't resurrect this pattern.
- **Electron** — considered, rejected for bundle size (100MB+ vs Tauri's
  3-10MB) and RAM overhead; Tauri's OS-webview approach was preferred.
- **Auto-rotating connection colors by tab order** — see color system
  section above, breaks the trust signal the colors exist for.
