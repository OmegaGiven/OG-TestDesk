<script>
  import { get } from 'svelte/store';
  import CodeEditor from '../components/CodeEditor.svelte';
  import ResultGrid from './ResultGrid.svelte';
  import SchemaTree from './SchemaTree.svelte';
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
    sendToInspector
  } from '../stores.js';

  let sidebarConnId = null;
  let modal = null; // null | {existing}
  let splitPct = 55;
  let dragging = false;

  $: conns = $connections;
  $: if (!sidebarConnId && conns[0]) sidebarConnId = conns[0].id;
  $: sidebarConn = conns.find((c) => c.id === sidebarConnId);
  $: tab = $activeSqlTab;
  $: tabConn = tab ? conns.find((c) => c.id === tab.connection_id) : null;

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

  async function run() {
    const t = get(activeSqlTab);
    if (!t || t.running) return;
    const conn = get(connections).find((c) => c.id === t.connection_id);
    if (!conn) return;
    const sql = selectionOrAll(t.sql_text);
    if (!sql.trim()) return;
    touchSqlTab(t.id, { running: true, error: null });
    try {
      const result = await api.queryRun(conn, sql);
      touchSqlTab(t.id, { result, running: false });
      if (result.is_select)
        toast(`${result.row_count} rows · ${result.duration_ms} ms`, 'success', 2500);
    } catch (e) {
      touchSqlTab(t.id, { error: String(e), running: false });
    }
  }

  function selectionOrAll(text) {
    const sel = window.getSelection()?.toString();
    return sel && sel.trim() ? sel : text;
  }

  async function saveQuery() {
    if (!tab) return;
    const name = prompt('Save query as:', tab.title);
    if (!name) return;
    try {
      await api.savedQuerySave({
        id: '',
        connection_id: tab.connection_id,
        folder: null,
        name,
        sql_text: tab.sql_text,
        created_at: 0
      });
      toast('Query saved', 'success');
    } catch (e) {
      toastError(e);
    }
  }

  function exportData(fmt) {
    const r = tab?.result;
    if (!r || !r.is_select) return;
    let text, mime, ext;
    if (fmt === 'json') {
      const objs = r.rows.map((row) =>
        Object.fromEntries(r.columns.map((c, i) => [c.name, row[i]]))
      );
      text = JSON.stringify(objs, null, 2);
      mime = 'application/json';
      ext = 'json';
    } else {
      const esc = (v) =>
        v === null || v === undefined
          ? ''
          : `"${String(typeof v === 'object' ? JSON.stringify(v) : v).replace(/"/g, '""')}"`;
      text = [
        r.columns.map((c) => c.name).join(','),
        ...r.rows.map((row) => row.map(esc).join(','))
      ].join('\n');
      mime = 'text/csv';
      ext = 'csv';
    }
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([text], { type: mime }));
    a.download = `${tab.title || 'result'}.${ext}`;
    a.click();
    URL.revokeObjectURL(a.href);
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
    <div class="conn-list">
      <div class="sec-head">
        <span>Connections</span>
        <button class="btn ghost sm" on:click={() => (modal = { existing: null })}>+ New</button>
      </div>
      {#each conns as c (c.id)}
        <div class="conn-item" class:active={sidebarConnId === c.id}>
          <button class="ci-main" on:click={() => (sidebarConnId = c.id)}>
            <span class="dot" style="background:{c.color || 'var(--conn-slate)'}" />
            <span class="ci-name">{c.nickname}</span>
            <span class="ci-kind">{c.kind}</span>
          </button>
          <button class="gear" title="Edit" on:click={() => (modal = { existing: c })}>⚙</button>
          <button class="plus" title="New query" on:click={() => newSqlTab(c.id)}>+</button>
        </div>
      {/each}
      {#if conns.length === 0}
        <div class="none">No connections yet.</div>
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
        <button class="btn primary sm" on:click={run} disabled={tab.running}>
          {tab.running ? 'Running…' : '▶ Run'}
        </button>
        <button class="btn sm" on:click={saveQuery}>Save</button>
        <span class="tb-conn">
          <span class="dot" style="background:{tabConn?.color || 'var(--conn-slate)'}" />
          {tabConn?.nickname}
        </span>
        <span style="flex:1" />
        {#if tab.result?.is_select}
          <button class="btn ghost sm" on:click={inspectResult}>→ Inspector</button>
          <button class="btn ghost sm" on:click={() => exportData('csv')}>CSV</button>
          <button class="btn ghost sm" on:click={() => exportData('json')}>JSON</button>
        {/if}
      </div>

      <div class="workarea">
        <div class="pane editor-pane" style="height:{splitPct}%">
          <CodeEditor
            value={tab.sql_text}
            language="sql"
            on:change={(e) => onChange(e.detail)}
            on:run={run}
            on:save={saveQuery}
          />
        </div>
        <div class="splitter" on:mousedown={startDrag} role="separator" tabindex="-1"></div>
        <div class="pane result-pane" style="height:{100 - splitPct}%">
          {#if tab.error}
            <div class="err">{tab.error}</div>
          {:else}
            <ResultGrid result={tab.result} on:inspect={inspectCell} />
          {/if}
        </div>
      </div>

      <div class="statusbar">
        {#if tab.error}
          <span class="s-err">error</span>
        {:else if tab.result}
          {tab.result.is_select
            ? `${tab.result.row_count} rows`
            : `${tab.result.rows_affected} affected`} · {tab.result.duration_ms} ms
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
  .conn-list {
    border-bottom: 1px solid var(--border);
    padding-bottom: 6px;
    max-height: 45%;
    overflow: auto;
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
  .conn-item {
    display: flex;
    align-items: center;
    padding: 0 4px;
  }
  .conn-item.active {
    background: color-mix(in srgb, var(--tool-sql-text) 12%, transparent);
  }
  .ci-main {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
    cursor: pointer;
    padding: 6px 4px;
    text-align: left;
    overflow: hidden;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .ci-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ci-kind {
    font-size: 9px;
    color: var(--text-muted);
    text-transform: uppercase;
  }
  .gear,
  .plus {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px 5px;
    font-size: 12px;
  }
  .gear:hover,
  .plus:hover {
    color: var(--text-primary);
  }
  .none {
    padding: 8px;
    font-size: 11px;
    color: var(--text-muted);
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
