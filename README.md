# OG TestDesk

Native desktop dev desk — SQL workspace, HTTP request client, and a shared
JSON inspector. Built with Tauri (Rust core + native webview) and Svelte.

Rewritten from the earlier iced-based prototype: `core/` was redesigned
around a driver-abstraction trait (Postgres/MySQL/SQLite today, easy to
add more), and the UI moved to a webview so styling iterates in CSS
instead of Rust structs.

## Project layout

```
core/         og_testdesk_core — persistence (SQLite via sqlx), DB driver
              abstraction, HTTP request runner, OS keychain secrets.
              No UI code.
src-tauri/    Tauri backend — thin command layer wrapping core/, window
              config, packaging.
frontend/     SvelteKit UI — SQL / Requests / Inspector modules, the
              color-coded top nav, shared JSON tree viewer.
```

## Color system

Each tool (SQL, Requests, Inspector) has a fixed hue used everywhere its
content appears — top nav, buttons, selection state:

| Tool      | Hue    |
| --------- | ------ |
| SQL       | Blue   |
| Requests  | Green  |
| Inspector | Violet |

Within SQL, each saved connection gets its own accent dot color (user
assignable), which tints that connection's query tabs — so which DB a
tab belongs to is visible at a glance, not just on hover.

## Running from source

Requires Rust, Node 20+, and `pnpm`.

```
cd frontend && pnpm install && cd ..
cargo tauri dev
```

(`cargo tauri` requires `cargo install tauri-cli --version "^2"` once.)

The app stores its local metadata SQLite DB in the OS app-data
directory (`OGTestDesk/og_testdesk.db`); override with
`OGTESTDESK_DB_PATH`. Connection passwords are never written to that
file — they live in the OS keychain (macOS Keychain / Windows
Credential Manager / Linux Secret Service), keyed by connection id.

## Building a release

```
cd frontend && pnpm build && cd ..
cargo tauri build
```

## Feature status

| Feature   | Status                                                                 |
| --------- | ----------------------------------------------------------------------- |
| SQL       | Connection manager, driver trait, metadata schema scaffolded. Query execution (`run_query`) and schema browser are TODO per-driver. |
| Requests  | Request/response types + `send()` implemented. Collections UI and saved-request persistence TODO. |
| Inspector | Not yet started — shared tree/table/summary viewer, consumes both SQL results and HTTP responses. |
| Top nav   | Color-coded tool/connection/tab component implemented (`TopNav.svelte`) with placeholder data. |
