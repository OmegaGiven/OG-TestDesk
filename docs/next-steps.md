# Next steps

MVP is built and has grown well past MVP — see `../CLAUDE.md` for the
current feature set (MCP server w/ OAuth, on-disk history, saved-query
folders, themes, split-screen SQL, error log, app-state debugger,
etc.). This file tracks what's still rough or missing before a wider
release, roughly in priority order.

## Before a beta release

- [ ] Result grid virtualization — still renders every row into the
      DOM. Fine to ~10k rows; windowing matters for larger sets.
- [ ] `docs/design-decisions.md` — reread and update against what
      actually shipped; some early decisions (e.g. the old tool-group
      top-bar design) have since been superseded.
- [ ] Code signing / notarization — `release.yml` builds mac/Windows/
      Linux on a `v*` tag push via `tauri-action`, but unsigned: macOS
      Gatekeeper and Windows SmartScreen will both warn on install.
      Needs an Apple Developer ID cert + notarization credentials and a
      Windows code-signing cert, wired in as repo secrets.
- [ ] `cargo fmt` / `cargo clippy` are wired into CI as informational
      only (`continue-on-error`) since the codebase isn't currently
      clean under either — decide whether to actually run
      `cargo fmt --all` once and make it a real gate, or leave it loose.

## Drivers

- [ ] `NUMERIC`/`DECIMAL` precision: currently parsed to JS number when
      it round-trips, else kept as string. Consider always-string for
      money.
- [ ] Postgres arrays / composite types fall back to `<TYPE>` — decode
      the common ones (`_int4`, `_text`, `_uuid`).
- [ ] Connection pool eviction on connection edit/delete (pool cache in
      `core/src/drivers/pool.rs` keys on the conn string, so a changed
      password makes a new pool but the old one lingers until process
      exit — acceptable, revisit if it matters).
- [ ] `EXPLAIN`/`ANALYZE` result rendering (plain text panel).

## Requests

- [ ] Response body: syntax highlight for XML/HTML, image preview.
- [ ] Import from `curl` (Postman collection import already exists).
- [ ] Per-request environment override + variable autocomplete.

## Inspector

- [ ] Search prev/next navigation (match count is shown; no jump yet).
- [ ] Tree virtualization for very large payloads.
- [ ] "Diff two payloads" mode.

## MCP

- [ ] `release.yml`/distribution note: the OAuth flow's "Approve
      access?" page has no branding beyond plain text — fine
      functionally, worth a pass once the app has real visual identity
      to reuse there.
