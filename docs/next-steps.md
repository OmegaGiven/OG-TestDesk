# Next steps, roughly in order

Not a rigid sequence — but later items generally depend on earlier ones
being at least stubbed out.

## 1. Get one query executing end-to-end
- [ ] `MetadataStore`: add query helpers (insert/select connections,
      query_tabs) — currently only `pool()` is exposed, commands have
      nothing to call.
- [ ] Wire `list_connections` / `save_connection` in `src-tauri/src/main.rs`
      to actually hit the metadata DB.
- [ ] Implement `SqliteDriverImpl::run_query` (see CLAUDE.md — suggested
      first task, no network dependency, fastest path to a working
      pipe).
- [ ] Frontend: replace `+page.svelte`'s hardcoded `tools` array with a
      real `invoke('list_connections')` call on mount.

## 2. SQL module UI
- [ ] Query editor (syntax highlight — check what's available for
      Svelte, e.g. CodeMirror 6 has a Svelte-friendly API).
- [ ] Results grid (sortable/filterable — don't hand-roll, look at
      existing Svelte table components before building one).
- [ ] Schema browser sidebar (tables/views tree, feeds `list_schemas`).

## 3. Postgres + MySQL drivers
- [ ] Same shape as SQLite, now with real network connections + auth.
- [ ] Pull password via `SecretsStore::get` before connecting (already
      wired in the `run_query` Tauri command, just needs the driver impl
      to do something with it).

## 4. Requests module
- [ ] Request builder UI (method/url/headers/body) — `HttpRequest`
      type already exists in `core::requests`.
- [ ] Wire to `send_request` command (already implemented, untested).
- [ ] Collections/folders — `request_collections` + `saved_requests`
      tables already exist in the metadata schema, no UI yet.

## 5. Inspector module
- [ ] Full spec in `docs/design-decisions.md` — build the shared tree
      component first, wire SQL results in, then HTTP responses.

## 6. Polish / cross-cutting
- [ ] Connection color picker (see design-decisions.md — must be
      user-assignable, not auto-rotated).
- [ ] Tab persistence across app restarts (query_tabs table has
      `position`/`is_active` columns for this, unused so far).
- [ ] Error handling / toasts — nothing currently surfaces `Result::Err`
      to the user anywhere in the frontend.
