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

## Versioning

One version, in one place: the workspace `Cargo.toml`'s
`[workspace.package] version` — `src-tauri`/`core` inherit it via
`version.workspace = true`, and `tauri.conf.json` has no `version` of
its own, so Tauri reads it from `src-tauri`'s Cargo.toml too.

```
scripts/bump-version.sh 0.2.0     # updates Cargo.toml + frontend/package.json
git add -A && git commit -m "Bump version to 0.2.0"
git tag v0.2.0 && git push origin main v0.2.0
```

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which
builds and packages the app for macOS, Windows, and Linux and attaches
the installers to a (draft) GitHub Release. See `CHANGELOG.md`.

## Feature status

| Feature   | Status                                                                 |
| --------- | ----------------------------------------------------------------------- |
| SQL       | Working. Connection manager (test + accent color), lazy schema/column browser, CodeMirror editor (⌘↵ run, run-selection, ⌘S save), sortable result grid, CSV/JSON export, per-connection query tabs persisted across restarts. Postgres / MySQL / SQLite. |
| Requests  | Working. Collections + saved requests, method/URL bar with Params⇄URL sync, headers, JSON body (beautify), response viewer (status/time/size, body + headers), `{{variable}}` environments with an active-env switcher. |
| Inspector | Working. Tree / Table / Summary modes, search with match count + auto-expand, detail panel (path / type / size / pretty-print + copy). Fed by SQL results and HTTP responses, or paste raw JSON. |
| Top nav   | Color-coded tool → connection → tab, live from app state. |

Post-MVP polish is tracked in `docs/next-steps.md`.

## License

[Apache License 2.0](LICENSE) — genuinely open source, free for
anyone, personal or commercial, no strings attached.

## Support the project

If OG TestDesk has replaced Postman, Postico, pgAdmin, or a handful of
troubleshooting tools in your day-to-day — especially if that's at a
company where this now sits in the toolchain — consider
[sponsoring](https://github.com/sponsors/OmegaGiven) its development.
Nothing is gated behind it; it's just the most direct way to help keep
this maintained and moving.
