# OG TestDesk — context for Claude Code

Native desktop dev tool: SQL client + HTTP request client + shared JSON
inspector, in one app. Think TablePlus + Postman + a JSON viewer, unified,
with color-coded navigation so it's obvious which tool/connection/tab
you're looking at.

## Stack

- **`core/`** — Rust, UI-agnostic. SQLite (app metadata) via sqlx, DB
  driver trait abstraction (Postgres/MySQL/SQLite), HTTP request runner,
  OS keychain secrets (`keyring` crate).
- **`src-tauri/`** — Tauri v2 backend. Thin command layer wrapping
  `core/`. Do not put business logic here — call into `core/`.
- **`frontend/`** — SvelteKit (static adapter, required for Tauri).

Run: `cd frontend && pnpm install && cd .. && cargo tauri dev`
(needs `cargo install tauri-cli --version "^2"` once).

## History — why this exists

This replaced an earlier prototype in the same repo built with Rust +
`iced` (a native, non-webview GUI toolkit). That approach was abandoned
because styling in iced is Rust structs, not CSS — too slow to iterate
on visual design. Full rewrite: only `src-tauri/icons/icon.png` survived
from the old repo. Do not reference or resurrect old `iced` code/patterns
if you see them anywhere — they're not the direction.

## Conventions — don't relitigate these

- **Color system is locked.** SQL = blue, Requests = green, Inspector =
  violet (see `frontend/src/lib/styles/tokens.css` — `--tool-*-tint` /
  `--tool-*-text` vars). Any new UI for these tools reuses its tool's
  hue — buttons, badges, selected states, everything. Don't introduce a
  fourth ad-hoc color for a one-off element.
- **Driver trait, not per-DB special-casing.** New DB engines get a new
  `impl DbDriver` in `core/src/drivers/`, nothing else changes. See
  `docs/design-decisions.md` for the full rationale and the nav/color
  hierarchy spec.
- **Secrets never touch the metadata SQLite file.** Passwords go through
  `SecretsStore` (OS keychain) keyed by `connection.id`. The metadata DB
  only ever stores the id reference.

## Current state (what's real vs stubbed)

| Area | Status |
|---|---|
| `core::drivers` | Trait + 3 impls scaffolded, every method is `todo!()`. Nothing runs a real query yet. |
| `core::storage::metadata` | Schema is real and complete (see file for all tables). `open()` works. No query helpers written yet — callers use `pool()` directly. |
| `core::storage::secrets` | Fully implemented, should work as-is. |
| `core::requests` | `send()` and `apply_environment()` fully implemented. Untested against a real server. |
| `src-tauri` commands | All 4 commands exist but `list_connections`/`save_connection` are no-ops (don't touch the DB yet). `run_query`/`send_request` call into `core` correctly but will hit `todo!()` for query. |
| `frontend` | Only `TopNav.svelte` exists, with hardcoded placeholder data in `+page.svelte`. No SQL editor, no data grid, no request builder, no Inspector UI at all. |

## Suggested first task

Pick ONE, don't try to scope the whole app:

1. **`SqliteDriverImpl::run_query`** — simplest driver (no network), good
   first win, unblocks testing the whole pipe end-to-end (Tauri command →
   core → sqlx → back to frontend).
2. **Wire `list_connections`/`save_connection` to the metadata DB** —
   needed before (1) is useful from the UI.

Either is a reasonable starting prompt: "implement SqliteDriverImpl::run_query using sqlx, map rows to QueryResult, keep types matching core/src/drivers/mod.rs exactly."
