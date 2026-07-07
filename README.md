<p align="center">
  <img src="desktop/icons/icon.png" alt="OG TestDesk icon" width="160" height="160" />
</p>

# OG TestDesk

OG TestDesk is a native desktop developer desk — SQL workspace, HTTP request
client, JSON inspector, scratch pad, appearance editor, calculator, JWT
decoder, and an AI assistant — built in Rust with [iced](https://iced.rs).

It is a from-scratch rewrite (not a fork) of an earlier Tauri/web-based
version of this idea: no bundled webview, no HTML/JS frontend, no HTTP
server — just a single native binary per OS.

## Project layout

```text
core/      og_testdesk_core — persistence (SQLite via sqlx), SQL engine,
           HTTP request runner, AI provider clients, theme model. No UI code.
desktop/   og_testdesk_desktop — the iced application (all UI).
```

## Running from source

```sh
cargo run -p og_testdesk_desktop
```

The app stores its SQLite database and connection secrets in the OS
app-data directory:

- Windows: `%APPDATA%\OGTestDesk\og_testdesk.db`
- macOS: `~/Library/Application Support/OGTestDesk/og_testdesk.db`
- Linux: `$XDG_DATA_HOME/og-testdesk/og_testdesk.db` (defaults to
  `~/.local/share/og-testdesk`)

Override the database path directly with `OGTESTDESK_DB_PATH` if needed.

## Building a release binary

```sh
cargo build -p og_testdesk_desktop --release
```

## Packaging native installers

Packaging uses [`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager)
(config lives in `desktop/Cargo.toml` under `[package.metadata.packager]`).

```sh
cargo install cargo-packager --locked
cd desktop
cargo packager --release -f <format>
```

Available formats per platform:

- Linux: `deb`, `appimage`
- macOS: `app`, `dmg`
- Windows: `nsis`, `wix`

Output lands in `dist/` at the repo root.

> Note: building `appimage` requires a `linuxdeploy` binary compatible with
> your host's `binutils`/glibc. On very new/rolling-release distros the
> prebuilt `linuxdeploy` AppImage's bundled `strip` can fail on newer ELF
> section types (seen during development on an up-to-date Arch Linux host).
> This does not affect `deb` packaging or the GitHub Actions release build
> (which runs on standard `ubuntu-latest`).

### Automated releases

Pushing a tag starting with `v` (e.g. `v0.1.0`) triggers
`.github/workflows/release.yml`, which builds and packages the app on
Linux, macOS, and Windows runners and attaches the artifacts to a GitHub
Release. This requires the repo to have a GitHub remote configured.

## Feature status

| Feature | Status |
|---|---|
| SQL Workspace | Connections, ad-hoc query + results grid, saved queries. Schema browser, background jobs, run history, table editing, CSV export are follow-ups. |
| Requests (API client) | Method/URL/headers/body builder, send, response view, saved requests. Folders, history, variable sets, Postman import, GraphQL/multipart, OAuth are follow-ups. |
| JSON Inspector | Parse, pretty-print, top-level summary. Collapsible tree view, diffing, search are follow-ups. |
| Scratch Pad | Single persisted text pad. |
| Calculator | Expression evaluation. |
| JWT Decoder | Header/payload decode (no signature verification). |
| Appearance | Light/dark/custom theme editor, persisted. Does not yet live-restyle the running app. |
| AI Assistant | Settings + single-profile chat via OpenAI-compatible/Ollama/Gemini providers. |
| Scheduled Jobs | Not yet started. |

Dropped from the original design: the `go/alias` shortcut/redirect feature,
the Actix web server / browser mode, and Docker deployment — this project is
native-desktop-only.
