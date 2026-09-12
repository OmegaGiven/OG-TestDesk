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

## Current state (MVP built — all three modules functional end to end)

| Area | Status |
|---|---|
| `core::drivers` | Trait + Postgres/MySQL/SQLite impls all real: `test_connection` (returns `ServerInfo`), `list_schemas`, `list_columns`, `run_query` with dynamic row→JSON decoding (`drivers/decode.rs`). Pool cache in `drivers/pool.rs`. Non-SELECT statements report `rows_affected`. |
| `core::storage::metadata` | Full CRUD: connections (+reorder), query_tabs, query_history (auto-trimmed to 500), saved_queries, request_collections, saved_requests, environments, app_state. Models defined in the same file. |
| `core::storage::secrets` | Unchanged, works. Passwords keyed by `connection.id`. |
| `core::requests` | `send()` returns status text, header list, content-type + `is_json` sniff, timing. `apply_environment()` substitutes `{{var}}` in url/headers/body. |
| `src-tauri` | ~27 commands wrapping all of the above (`main.rs`). `query_run` records history. `request_send` applies the active environment. IDs minted server-side (uuid v4). |
| `frontend` | Real app. `stores.js` holds all state; `api.js` is the single command surface. **SQL**: connection manager modal w/ test + accent picker, schema tree (lazy columns), CodeMirror 6 editor (⌘↵ run, ⌘S save, run-selection), sortable result grid, CSV/JSON export, → Inspector. **Requests**: collections sidebar, method/url bar, Params⇄URL sync, headers, JSON body w/ beautify, response pane (status/time/size, body + headers tabs), environments modal. **Inspector**: Tree / Table / Summary modes, recursive `JsonNode`, search w/ match count + auto-expand, right-side detail panel (path/type/size/pretty + copy), "Paste JSON" standalone mode. Fed by SQL results and HTTP responses via `sendToInspector`. Theme toggle (system/light/dark), toasts, ⌘1/2/3 tool switch. |

## Build / run

- `cd frontend && pnpm install` then `cargo tauri dev` (cargo-tauri v2 + pnpm required — both installed).
- `pnpm build` in `frontend/` produces `build/`; `cargo check` passes for the whole workspace.
- Icons regenerated as RGBA (scaffold's `icon.png` was palette-mode and broke `generate_context!`).

## MCP server (`src-tauri/src/mcp.rs`)

The app can host a local MCP server so on-device AI tools use its stored
connections/requests **without seeing secrets** — the server executes
queries and requests itself. HTTP+SSE transport (`GET /sse`, `POST
/message`), bound to `127.0.0.1:<port>` (default 7788). Two auth paths
accepted side by side on every request: the static config token
(`?token=` or `Authorization: Bearer`, shown in Settings — what
`claude mcp add` uses) OR a minimal OAuth 2.0 layer (metadata discovery
at `/.well-known/oauth-authorization-server` + `oauth-protected-resource`,
dynamic client registration at `/register`, authorization-code + PKCE at
`/authorize` + `/token`) for clients that require OAuth for a remote
connector — ChatGPT's connector framework, notably. Since this is a
single-user local app, "authorize" is a plain approve/deny page, no
login. OAuth state (registered clients, codes, tokens) is in-memory
only — resets on restart, which MCP OAuth clients handle transparently
by redoing discovery. A 401 carries a `WWW-Authenticate:
resource_metadata=...` header so OAuth-aware clients auto-discover the
flow. Config + per-connection
ACLs live in `app_state` (`mcp_config`, `mcp_connections`); auto-starts on
launch if left enabled. Tools: `list_connections`, `list_schemas`,
`list_columns`, `run_query` (+ `list_saved_requests`, `run_saved_request`,
`send_request` when `allow_http`; + `open_sql_tab`, `save_query`,
`save_sql_file`, `save_request` when `allow_populate`; + `add_connection`
when `allow_manage_connections`). Safety: a connection is invisible until
explicitly exposed; exposed = read-only unless per-connection **and**
server-wide write flags are both on. The `allow_populate` tools never
execute anything (SQL/HTTP) — they only write into a tab, a saved
item, or a file under a fixed exports dir, for the human to read/run
themselves. `add_connection` writes a real credential to the OS
keychain but never auto-exposes the connection to MCP. Frontend has no
push channel from Rust, so `+page.svelte` refetches everything on
window focus to pick up anything these tools wrote while the app
wasn't in front. UI: gear icon → Settings modal
(`components/SettingsModal.svelte`).

## What's left

See `docs/next-steps.md` — polish (result virtualization, saved-query/history panels, native modals instead of `prompt()`), driver edge cases (PG arrays/composites), and Requests stretch (auth helpers, curl import).
