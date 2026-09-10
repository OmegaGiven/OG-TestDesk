# Next steps

MVP is built (see the status table in `../CLAUDE.md`). Remaining polish
and stretch items, roughly in priority order.

## Near-term polish

- [ ] Result grid virtualization — currently renders every row into the
      DOM. Fine to ~10k rows; add windowing (e.g. manual slice on scroll)
      for large result sets.
- [ ] Connection reorder UI — backend `connections_reorder` exists, no
      drag handle in the sidebar yet.
- [ ] Saved-query browser — `saved_query_save` is wired (toolbar "Save"),
      but there's no panel to list / open / delete saved queries.
- [ ] Query history panel — `history_recent` command exists and every run
      is recorded; surface it (sidebar tab or ⌘R palette).
- [ ] Move `prompt()` / `confirm()` calls to real modals (they work but
      look non-native).
- [ ] Request "Params" tab: decode/encode edge cases (array params,
      existing fragments).

## Drivers

- [ ] `NUMERIC`/`DECIMAL` precision: currently parsed to JS number when it
      round-trips, else kept as string. Consider always-string for money.
- [ ] Postgres arrays / composite types fall back to `<TYPE>` — decode the
      common ones (`_int4`, `_text`, `_uuid`).
- [ ] Connection pool eviction on connection edit/delete (pool cache in
      `core/src/drivers/pool.rs` keys on the conn string, so a changed
      password makes a new pool but the old one lingers until process
      exit — acceptable, revisit if it matters).
- [ ] `EXPLAIN`/`ANALYZE` result rendering (plain text panel).

## Requests

- [ ] Response body: syntax highlight for XML/HTML, image preview.
- [ ] Cookie jar / auth helpers (Bearer, Basic, API key) as a dedicated
      tab instead of manual headers.
- [ ] Import from `curl` / Postman collection JSON.
- [ ] Per-request environment override + variable autocomplete.

## Inspector

- [ ] Search prev/next navigation (match count is shown; no jump yet).
- [ ] Tree virtualization for very large payloads.
- [ ] "Diff two payloads" mode.

## Cross-cutting

- [ ] `cargo tauri build` bundle: generate `icon.icns` + proper
      multi-size `icon.ico` (the `magick` one-liner in the build notes),
      set up updater endpoint or drop `tauri-plugin-updater`.
- [ ] Persist last active tool + window size via `app_state`.
- [ ] Tests: `core::drivers` against disposable Docker DBs; `stmt_returns_rows`
      unit tests.
