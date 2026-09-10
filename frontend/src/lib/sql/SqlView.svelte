<script>
  import { get } from 'svelte/store';
  import CodeEditor from '../components/CodeEditor.svelte';
  import ResultGrid from './ResultGrid.svelte';
  import SchemaTree from './SchemaTree.svelte';
  import SavedQueries from './SavedQueries.svelte';
  import ConnectionModal from './ConnectionModal.svelte';
  import { api } from '../api.js';
  import {
    connections,
    sqlTabs,
    activeSqlTab,
    activeSqlTabId,
    newSqlTab,
    touchSqlTab,
    persistSqlTab,
    toast,
    toastError,
    sendToInspector,
    historyLoad,
    appearance,
    savedQueries,
    reloadSavedQueries,
    connMenuOpen
  } from '../stores.js';

  let sqOpen =
    typeof location !== 'undefined' && new URLSearchParams(location.search).has('savedqueries');
  let consumedHistory = null;

  $: if ($historyLoad && $historyLoad.kind === 'sql' && $historyLoad.at !== consumedHistory) {
    consumedHistory = $historyLoad.at;
    openFromHistory($historyLoad.entry, $historyLoad.resolved);
  }

  async function openFromHistory(entry, resolved) {
    const connId =
      ($connections.find((c) => c.id === entry.connection_id) || {}).id ||
      sidebarConnId ||
      $connections[0]?.id;
    if (!connId) return;
    const t = await newSqlTab(connId, entry.sql_text);
    touchSqlTab(t.id, { title: 'History', dirty: false });
    if (resolved) {
      try {
        touchSqlTab(t.id, { result: JSON.parse(resolved) });
      } catch {}
    } else if (entry.error) {
      touchSqlTab(t.id, { error: entry.error });
    }
    persistSqlTab(t.id, true);
  }
  let modal = null; // null | {existing}
  let splitPct = 55;
  let dragging = false;

  $: conns = $connections;
  $: tab = $activeSqlTab;
  $: tabConn = tab ? conns.find((c) => c.id === tab.connection_id) : null;
  // schema tree + fallbacks follow the active tab's connection
  $: sidebarConn = tabConn || conns[0];
  $: sidebarConnId = sidebarConn?.id;

  // ---- SQL variables: {{name}} tokens filled from slots above the editor
  const VAR_RE = /\{\{\s*([A-Za-z_]\w*)\s*\}\}/g;
  let varValues = {}; // { [tabId]: { [name]: value } }
  let loadedVarsFor = null;
  let varTimer;

  $: vars = tab
    ? [...new Set([...tab.sql_text.matchAll(VAR_RE)].map((m) => m[1]))]
    : [];

  $: if (tab && tab.id !== loadedVarsFor) {
    loadedVarsFor = tab.id;
    loadVars(tab.id);
  }

  async function loadVars(id) {
    try {
      const raw = await api.stateGet('sqlvars:' + id);
      varValues = { ...varValues, [id]: raw ? JSON.parse(raw) : {} };
    } catch {
      varValues = { ...varValues, [id]: {} };
    }
  }
  function setVar(name, value) {
    const id = tab.id;
    varValues = { ...varValues, [id]: { ...(varValues[id] || {}), [name]: value } };
    clearTimeout(varTimer);
    varTimer = setTimeout(
      () => api.stateSet('sqlvars:' + id, JSON.stringify(varValues[id] || {})).catch(() => {}),
      500
    );
  }
  function applyVars(sql, id) {
    const vals = varValues[id] || {};
    return sql.replace(VAR_RE, (m, name) => (name in vals && vals[name] !== '' ? vals[name] : m));
  }
  $: missingVars = tab ? vars.filter((v) => !((varValues[tab.id] || {})[v] ?? '')) : [];

  function quote(conn, ident) {
    if (conn.kind === 'mysql') return '`' + ident.replace(/`/g, '``') + '`';
    return '"' + ident.replace(/"/g, '""') + '"';
  }

  async function openRelation(e) {
    const { schema, relation } = e.detail;
    const c = sidebarConn;
    const q =
      c.kind === 'sqlite'
        ? `SELECT * FROM ${quote(c, relation)} LIMIT 100;`
        : `SELECT * FROM ${quote(c, schema)}.${quote(c, relation)} LIMIT 100;`;
    const t = await newSqlTab(c.id, q);
    touchSqlTab(t.id, { title: relation });
    persistSqlTab(t.id, true);
    setTimeout(() => run(), 30);
  }

  function onChange(v) {
    if (!tab) return;
    touchSqlTab(tab.id, { sql_text: v, dirty: true });
    persistSqlTab(tab.id);
  }

  // page < 0 → full result (no pagination wrapper, capped by max rows)
  async function run(page = 0) {
    const t = get(activeSqlTab);
    if (!t || t.running) return;
    const conn = get(connections).find((c) => c.id === t.connection_id);
    if (!conn) return;

    let sql;
    if (page > 0 && t.execSql) {
      sql = t.execSql; // paging — reuse the exact statement page 0 ran
    } else {
      sql = applyVars(selectionOrAll(t.sql_text), t.id);
      if (!sql.trim()) return;
      if (VAR_RE.test(sql)) {
        VAR_RE.lastIndex = 0;
        toast('Unfilled variables — set values in the bar above the editor', 'error', 4000);
        return;
      }
      touchSqlTab(t.id, { execSql: sql });
    }

    const full = page < 0;
    const size = get(appearance).pageSize ?? 500;
    touchSqlTab(t.id, { running: true, error: null });
    try {
      const result = await api.queryRun(
        conn,
        sql,
        full ? null : Math.max(0, page),
        full ? null : size,
        !full && page === 0
      );
      touchSqlTab(t.id, { result, running: false });
      if (result.is_select) {
        const shown = result.total != null ? ` of ${result.total.toLocaleString()}` : '';
        toast(`${result.row_count.toLocaleString()} rows${shown} · ${result.duration_ms} ms`, 'success', 2500);
      }
    } catch (e) {
      touchSqlTab(t.id, { error: String(e), running: false });
    }
  }

  function nextPage() {
    const r = tab?.result;
    if (r?.has_more) run(r.page + 1);
  }
  function prevPage() {
    const r = tab?.result;
    if (r && r.page > 0) run(r.page - 1);
  }

  function selectionOrAll(text) {
    const sel = window.getSelection()?.toString();
    return sel && sel.trim() ? sel : text;
  }

  async function saveQuery() {
    if (!tab) return;
    const raw = prompt('Save query as (use folder/name to group):', tab.title);
    if (!raw || !raw.trim()) return;
    const i = raw.lastIndexOf('/');
    const folder = i >= 0 ? raw.slice(0, i).trim() || null : null;
    const name = (i >= 0 ? raw.slice(i + 1) : raw).trim();
    // update in place if a query with the same folder/name/connection exists
    const existing = get(savedQueries).find(
      (s) => s.name === name && (s.folder || null) === folder && s.connection_id === tab.connection_id
    );
    try {
      await api.savedQuerySave({
        id: existing?.id || '',
        connection_id: tab.connection_id,
        folder,
        name,
        sql_text: tab.sql_text,
        created_at: existing?.created_at || 0
      });
      await reloadSavedQueries();
      sqOpen = true;
      toast(existing ? 'Saved query updated' : 'Query saved', 'success', 1800);
    } catch (e) {
      toastError(e);
    }
  }

  async function openSavedQuery(e) {
    const s = e.detail;
    const connId =
      $connections.find((c) => c.id === s.connection_id)?.id || sidebarConnId || $connections[0]?.id;
    if (!connId) return;
    const t = await newSqlTab(connId, s.sql_text);
    touchSqlTab(t.id, { title: s.name, dirty: false });
    persistSqlTab(t.id, true);
  }

  function inspectResult() {
    const r = tab?.result;
    if (!r || !r.is_select) return;
    const objs = r.rows.map((row) => Object.fromEntries(r.columns.map((c, i) => [c.name, row[i]])));
    sendToInspector('sql', `${tab.title} (${r.row_count} rows)`, objs);
  }

  function inspectCell(e) {
    sendToInspector('sql', `${tab.title} · ${e.detail.column}`, e.detail.value);
  }

  // splitter drag
  function startDrag() {
    dragging = true;
  }
  function onMove(e) {
    if (!dragging) return;
    const host = document.querySelector('.workarea');
    if (!host) return;
    const rect = host.getBoundingClientRect();
    splitPct = Math.min(85, Math.max(15, ((e.clientY - rect.top) / rect.height) * 100));
  }
  function endDrag() {
    dragging = false;
  }
</script>

<svelte:window on:mousemove={onMove} on:mouseup={endDrag} />

<div class="sql">
  <aside class="sidebar">
    <div class="sb-head">
      {#if sidebarConn}
        <span class="dot" style="background:{sidebarConn.color || 'var(--conn-slate)'}" />
        <span class="sb-conn">{sidebarConn.nickname}</span>
      {/if}
      <button class="btn ghost sm" title="Switch / manage connections" on:click={() => connMenuOpen.set(true)}>
        {sidebarConn ? 'Switch ▾' : 'Connections ▾'}
      </button>
    </div>

    <div class="sq-section" class:open={sqOpen}>
      <button class="sec-head sq-toggle" on:click={() => (sqOpen = !sqOpen)}>
        <span class="chev">{sqOpen ? '▾' : '▸'}</span>
        <span>Saved queries</span>
        <span class="sq-badge">{$savedQueries.length}</span>
      </button>
      {#if sqOpen}
        <div class="sq-body">
          <SavedQueries on:open={openSavedQuery} />
        </div>
      {/if}
    </div>

    {#if sidebarConn}
      <div class="schema-host">
        <SchemaTree conn={sidebarConn} on:open={openRelation} />
      </div>
    {/if}
  </aside>

  <section class="main">
    {#if !tab}
      <div class="empty">
        {#if conns.length === 0}
          <p>Create a connection to start querying.</p>
          <button class="btn primary" on:click={() => (modal = { existing: null })}>New connection</button>
        {:else}
          <p>Open a query tab from a connection in the sidebar.</p>
          <button class="btn primary" on:click={() => newSqlTab(sidebarConnId || conns[0].id)}>
            New query
          </button>
        {/if}
      </div>
    {:else}
      <div class="toolbar">
        <button class="btn primary sm" on:click={() => run()} disabled={tab.running}>
          {tab.running ? 'Running…' : '▶ Run'}
        </button>
        <button class="btn sm" on:click={saveQuery}>Save</button>
        <span class="tb-conn">
          <span class="dot" style="background:{tabConn?.color || 'var(--conn-slate)'}" />
          {tabConn?.nickname}
        </span>
        {#if tab.result?.is_select && tab.result.page_size > 0}
          {@const r = tab.result}
          {@const from = r.page * r.page_size + 1}
          {@const to = r.page * r.page_size + r.row_count}
          <span class="pager">
            <button class="pg" on:click={prevPage} disabled={r.page === 0 || tab.running}>◀</button>
            <span class="pg-info">
              {from.toLocaleString()}–{to.toLocaleString()}{r.total != null
                ? ` of ${r.total.toLocaleString()}`
                : r.has_more
                  ? ' of many'
                  : ''}
            </span>
            <button class="pg" on:click={nextPage} disabled={!r.has_more || tab.running}>▶</button>
            {#if r.count_ms != null}<span class="pg-ct">count {r.count_ms}ms</span>{/if}
          </span>
        {/if}
        <span style="flex:1" />
        {#if tab.result?.is_select}
          {#if tab.result.page_size > 0 && (tab.result.has_more || tab.result.page > 0)}
            <button class="btn ghost sm" on:click={() => run(-1)}>Load all</button>
          {/if}
          <button class="btn ghost sm" on:click={inspectResult}>→ Inspector</button>
        {/if}
      </div>

      {#if vars.length}
        <div class="varbar">
          <span class="vb-label">Variables</span>
          {#each vars as v (v)}
            <label class="vb-slot" class:missing={missingVars.includes(v)}>
              <span class="vb-name">{`{{${v}}}`}</span>
              <input
                class="vb-input"
                value={(varValues[tab.id] || {})[v] ?? ''}
                on:input={(e) => setVar(v, e.target.value)}
                placeholder="value"
              />
            </label>
          {/each}
        </div>
      {/if}

      <div class="workarea">
        <div class="pane editor-pane" style="height:{splitPct}%">
          <CodeEditor
            value={tab.sql_text}
            language="sql"
            on:change={(e) => onChange(e.detail)}
            on:run={() => run()}
            on:save={saveQuery}
          />
        </div>
        <div class="splitter" on:mousedown={startDrag} role="separator" tabindex="-1"></div>
        <div class="pane result-pane" style="height:{100 - splitPct}%">
          {#if tab.error}
            <div class="err">{tab.error}</div>
          {:else}
            <ResultGrid result={tab.result} name={tab.title} on:inspect={inspectCell} />
          {/if}
        </div>
      </div>

      <div class="statusbar">
        {#if tab.error}
          <span class="s-err">error</span>
        {:else if tab.result}
          {tab.result.is_select
            ? tab.result.total != null
              ? `${tab.result.total.toLocaleString()} rows total`
              : `${tab.result.row_count.toLocaleString()} rows`
            : `${tab.result.rows_affected} affected`} · {tab.result.duration_ms} ms{tab.result.truncated
            ? ' · capped'
            : ''}
        {:else}
          ready · ⌘↵ run
        {/if}
      </div>
    {/if}
  </section>
</div>

{#if modal}
  <ConnectionModal
    existing={modal.existing}
    on:close={() => (modal = null)}
    on:saved={(e) => {
      sidebarConnId = e.detail.id;
      modal = null;
    }}
    on:deleted={() => {
      sidebarConnId = null;
      modal = null;
    }}
  />
{/if}

<style>
  .sql {
    display: flex;
    height: 100%;
    overflow: hidden;
  }
  .sidebar {
    width: 260px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    background: var(--surface-1);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .sb-head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 8px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .sb-conn {
    font-size: 12px;
    font-weight: 700;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }
  .sb-head .btn {
    flex-shrink: 0;
  }
  .sec-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 8px 6px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-secondary);
  }
  .sq-section {
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .sq-section.open {
    max-height: 42%;
  }
  .sq-toggle {
    width: 100%;
    background: none;
    border: none;
    cursor: pointer;
    justify-content: flex-start;
    gap: 5px;
  }
  .sq-toggle .chev {
    font-size: 11px;
    width: 12px;
    text-align: center;
    color: var(--text-muted);
  }
  .sq-badge {
    margin-left: auto;
    font-size: 9px;
    font-weight: 400;
    color: var(--text-muted);
  }
  .sq-body {
    flex: 1;
    overflow: hidden;
    border-top: 1px solid var(--border);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .schema-host {
    flex: 1;
    overflow: hidden;
  }
  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text-muted);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-1);
  }
  .tb-conn {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: var(--text-secondary);
    font-weight: 600;
  }
  .workarea {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .varbar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    padding: 6px 8px;
    background: var(--tool-sql-tint);
    border-bottom: 1px solid var(--border);
  }
  .vb-label {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--tool-sql-text);
  }
  .vb-slot {
    display: flex;
    align-items: center;
    gap: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    padding: 2px 4px 2px 7px;
  }
  .vb-slot.missing {
    border-color: var(--warn);
  }
  .vb-name {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--tool-sql-text);
  }
  .vb-input {
    font: inherit;
    font-size: 11px;
    border: none;
    background: none;
    color: var(--text-primary);
    width: 120px;
    padding: 3px 4px;
    outline: none;
  }
  .pane {
    overflow: hidden;
  }
  .result-pane {
    border-top: 1px solid var(--border);
    background: var(--surface-2);
  }
  .splitter {
    height: 6px;
    background: var(--surface-1);
    cursor: row-resize;
    flex-shrink: 0;
  }
  .splitter:hover {
    background: var(--tool-sql-text);
  }
  .err {
    padding: 14px;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--danger);
    white-space: pre-wrap;
    overflow: auto;
    height: 100%;
  }
  .pager {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-secondary);
  }
  .pg {
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    color: var(--text-primary);
    border-radius: var(--radius-sm);
    width: 22px;
    height: 22px;
    cursor: pointer;
    font-size: 9px;
  }
  .pg:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .pg-info {
    font-family: var(--font-mono);
    padding: 0 4px;
  }
  .pg-ct {
    color: var(--text-muted);
    font-size: 10px;
  }
  .statusbar {
    padding: 4px 10px;
    font-size: 10.5px;
    color: var(--text-muted);
    border-top: 1px solid var(--border);
    background: var(--surface-1);
  }
  .s-err {
    color: var(--danger);
  }
</style>
