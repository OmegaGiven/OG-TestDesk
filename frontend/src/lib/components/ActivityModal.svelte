<script>
  import Modal from './Modal.svelte';
  import { onMount, createEventDispatcher } from 'svelte';
  import { api } from '../api.js';
  import { ICONS } from '../icons.js';
  import {
    connections,
    savedRequests,
    savedQueries,
    savedCharts,
    reloadSavedCharts,
    loadFromHistory,
    sendToInspector,
    toast,
    toastError
  } from '../stores.js';

  const dispatch = createEventDispatcher();

  let tab = 'history'; // history | schedules | charts | errors
  let histKind = 'sql'; // sql | request
  let sqlHist = [];
  let reqHist = [];
  let schedules = [];
  let editing = null;
  let errors = [];

  onMount(() => {
    const q = new URLSearchParams(location.search);
    const at = q.get('atab');
    if (at === 'schedules') tab = 'schedules';
    if (at === 'charts') tab = 'charts';
    if (at === 'errors') tab = 'errors';
    if (at === 'requests') histKind = 'request';
    if (q.has('newschedule')) {
      tab = 'schedules';
      newSchedule();
    }
    reload();
  });
  async function reload() {
    try {
      [sqlHist, reqHist, schedules, errors] = await Promise.all([
        api.historyRecent(150),
        api.historyRequestRecent(150),
        api.schedulesList(),
        api.errorLogList(150)
      ]);
      await reloadSavedCharts();
    } catch (e) {
      toastError(e);
    }
  }

  async function clearErrors() {
    try {
      await api.errorLogClear();
      errors = [];
    } catch (e) {
      toastError(e);
    }
  }
  function copyErrorLog() {
    const text = errors
      .map((e) => `[${new Date(e.ts * 1000).toISOString()}] ${e.source}: ${e.message}`)
      .join('\n');
    navigator.clipboard?.writeText(text).then(
      () => toast('Error log copied', 'success', 1500),
      () => toastError('Copy blocked')
    );
  }

  async function openChart(c) {
    let rows = [];
    try {
      const dataJson = c.has_data ? await api.savedChartData(c.id) : null;
      rows = dataJson ? JSON.parse(dataJson) : [];
    } catch (e) {
      toastError(e);
    }
    sendToInspector('chart-reopen', c.name, rows, { existingChart: c });
    dispatch('close');
  }
  async function deleteChart(c) {
    if (!confirm(`Delete chart "${c.name}"?`)) return;
    try {
      await api.savedChartDelete(c.id);
      await reloadSavedCharts();
    } catch (e) {
      toastError(e);
    }
  }
  function fmtRunAt(ts) {
    return ts ? new Date(ts * 1000).toLocaleString() : 'never';
  }

  function connName(id) {
    return $connections.find((c) => c.id === id)?.nickname || id || '—';
  }
  function ago(ts) {
    if (!ts) return '—';
    const s = Math.floor(Date.now() / 1000 - ts);
    if (s < 60) return `${s}s ago`;
    if (s < 3600) return `${Math.floor(s / 60)}m ago`;
    if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
    return new Date(ts * 1000).toLocaleDateString();
  }
  function whenNext(ts) {
    if (!ts) return '—';
    const s = Math.floor(ts - Date.now() / 1000);
    if (s <= 0) return 'due';
    if (s < 60) return `in ${s}s`;
    if (s < 3600) return `in ${Math.floor(s / 60)}m`;
    if (s < 86400) return `in ${Math.floor(s / 3600)}h`;
    return new Date(ts * 1000).toLocaleString();
  }

  async function openSql(e) {
    let resolved = null;
    if (e.has_result) {
      try {
        resolved = await api.historyResult(e.id);
      } catch {}
    }
    loadFromHistory('sql', e, resolved);
    dispatch('close');
  }
  async function openReq(e) {
    let resolved = null;
    if (e.has_response) {
      try {
        resolved = await api.historyRequestResult(e.id);
      } catch {}
    }
    loadFromHistory('request', e, resolved);
    dispatch('close');
  }

  // ---- schedule editor
  function newSchedule() {
    editing = {
      id: '',
      name: 'New schedule',
      kind: 'sql',
      connection_id: $connections[0]?.id || null,
      sql_text: 'SELECT 1;',
      saved_request_id: null,
      request_method: 'GET',
      request_url: '',
      request_headers_json: null,
      request_body: null,
      schedule_expr: 'every:3600',
      enabled: true,
      last_run: null,
      last_status: null,
      next_run: null,
      created_at: 0
    };
  }
  function editSchedule(s) {
    editing = { ...s };
  }
  async function saveSchedule() {
    try {
      await api.scheduleSave(editing);
      toast('Schedule saved', 'success');
      editing = null;
      await reload();
    } catch (e) {
      toastError(e);
    }
  }
  async function delSchedule(s) {
    if (!confirm(`Delete schedule "${s.name}"?`)) return;
    try {
      await api.scheduleDelete(s.id);
      await reload();
    } catch (e) {
      toastError(e);
    }
  }
  async function runNow(s) {
    try {
      const status = await api.scheduleRunNow(s.id);
      toast(`${s.name}: ${status}`, status.startsWith('error') ? 'error' : 'success');
      await reload();
    } catch (e) {
      toastError(e);
    }
  }
  async function toggleEnabled(s) {
    try {
      await api.scheduleSave({ ...s, enabled: !s.enabled });
      await reload();
    } catch (e) {
      toastError(e);
    }
  }

  const EXPR_PRESETS = [
    ['every:300', 'every 5 min'],
    ['every:3600', 'hourly'],
    ['0 * * * *', 'top of every hour'],
    ['0 9 * * *', 'daily 09:00'],
    ['0 9 * * 1', 'Mondays 09:00']
  ];
</script>

<Modal title="Activity" width="820px" on:close>
  <div class="tabs">
    <button class:active={tab === 'history'} on:click={() => (tab = 'history')}>History</button>
    <button class:active={tab === 'schedules'} on:click={() => (tab = 'schedules')}>
      Schedules {#if schedules.length}<span class="n">{schedules.length}</span>{/if}
    </button>
    <button class:active={tab === 'charts'} on:click={() => (tab = 'charts')}>
      Charts {#if $savedCharts.length}<span class="n">{$savedCharts.length}</span>{/if}
    </button>
    <button class:active={tab === 'errors'} on:click={() => (tab = 'errors')}>
      Errors {#if errors.length}<span class="n">{errors.length}</span>{/if}
    </button>
  </div>

  {#if tab === 'history'}
    <div class="subtabs">
      <button class:active={histKind === 'sql'} on:click={() => (histKind = 'sql')}>
        SQL <span class="n">{sqlHist.length}</span>
      </button>
      <button class:active={histKind === 'request'} on:click={() => (histKind = 'request')}>
        Requests <span class="n">{reqHist.length}</span>
      </button>
      <span style="flex:1" />
      <button class="btn ghost sm" on:click={reload}>Refresh</button>
    </div>

    <div class="rows">
      {#if histKind === 'sql'}
        {#each sqlHist as e (e.id)}
          <button class="row" on:click={() => openSql(e)}>
            <span class="when">{ago(e.ran_at)}</span>
            <span class="tag">{connName(e.connection_id)}</span>
            <code class="sql">{e.sql_text.replace(/\s+/g, ' ').slice(0, 90)}</code>
            <span class="right {e.success ? '' : 'err'}">
              {e.success ? `${e.row_count ?? 0} rows` : 'error'}{e.duration_ms != null ? ` · ${e.duration_ms}ms` : ''}
              {#if e.has_result}<span class="cached" title="result cached — opens with data">◆</span>{/if}
            </span>
          </button>
        {/each}
        {#if sqlHist.length === 0}<div class="empty">No SQL history yet.</div>{/if}
      {:else}
        {#each reqHist as e (e.id)}
          <button class="row" on:click={() => openReq(e)}>
            <span class="when">{ago(e.sent_at)}</span>
            <span class="method m-{e.method.toLowerCase()}">{e.method}</span>
            <code class="sql">{e.url.slice(0, 80)}</code>
            <span class="right {e.success ? '' : 'err'}">
              {e.status ?? 'ERR'}{e.duration_ms != null ? ` · ${e.duration_ms}ms` : ''}
              {#if e.has_response}<span class="cached">◆</span>{/if}
            </span>
          </button>
        {/each}
        {#if reqHist.length === 0}<div class="empty">No request history yet.</div>{/if}
      {/if}
    </div>
  {:else if tab === 'schedules' && editing}
    <div class="field">
      <label>Name</label>
      <input class="input" bind:value={editing.name} />
    </div>
    <div class="row2">
      <div class="field">
        <label>Type</label>
        <select class="select" bind:value={editing.kind}>
          <option value="sql">SQL query</option>
          <option value="request">HTTP request</option>
        </select>
      </div>
      <div class="field" style="flex:2">
        <label>Schedule</label>
        <input class="input mono" bind:value={editing.schedule_expr} placeholder="every:3600 or 0 9 * * *" />
        <div class="presets">
          {#each EXPR_PRESETS as [ex, lbl]}
            <button class="chip" on:click={() => (editing.schedule_expr = ex)}>{lbl}</button>
          {/each}
        </div>
      </div>
    </div>

    {#if editing.kind === 'sql'}
      <div class="field">
        <label>Connection</label>
        <select class="select" bind:value={editing.connection_id}>
          {#each $connections as c}<option value={c.id}>{c.nickname}</option>{/each}
        </select>
      </div>
      {#if $savedQueries.length}
        <div class="field">
          <label>Load from a saved query</label>
          <select
            class="select"
            on:change={(e) => {
              const q = $savedQueries.find((x) => x.id === e.target.value);
              if (q) {
                editing.sql_text = q.sql_text;
                if (q.connection_id) editing.connection_id = q.connection_id;
              }
              e.target.value = '';
            }}
          >
            <option value="">— pick one to fill the SQL box below —</option>
            {#each $savedQueries as q}<option value={q.id}>{q.name}</option>{/each}
          </select>
        </div>
      {/if}
      <div class="field">
        <label>SQL</label>
        <textarea class="textarea" rows="4" bind:value={editing.sql_text}></textarea>
      </div>
    {:else}
      <div class="field">
        <label>Saved request (or fill inline below)</label>
        <select class="select" bind:value={editing.saved_request_id}>
          <option value={null}>— inline —</option>
          {#each $savedRequests as r}<option value={r.id}>{r.name}</option>{/each}
        </select>
      </div>
      {#if !editing.saved_request_id}
        <div class="row2">
          <div class="field">
            <label>Method</label>
            <input class="input" bind:value={editing.request_method} />
          </div>
          <div class="field" style="flex:3">
            <label>URL</label>
            <input class="input mono" bind:value={editing.request_url} />
          </div>
        </div>
      {/if}
    {/if}
    <label class="toggle"><input type="checkbox" bind:checked={editing.enabled} /> Enabled</label>

    <div class="editor-actions">
      <button class="btn" on:click={() => (editing = null)}>Cancel</button>
      <button class="btn primary" on:click={saveSchedule}>Save schedule</button>
    </div>
  {:else if tab === 'schedules'}
    <div class="subtabs">
      <span class="muted">Runs in the background while the app is open. Results land in History.</span>
      <span style="flex:1" />
      <button class="btn primary sm" on:click={newSchedule}>+ Schedule</button>
    </div>
    <div class="rows">
      {#each schedules as s (s.id)}
        <div class="srow">
          <button class="dot-btn" class:on={s.enabled} title="Enable/disable" on:click={() => toggleEnabled(s)}>
            {s.enabled ? '●' : '○'}
          </button>
          <span class="s-name">{s.name}</span>
          <span class="tag">{s.kind}</span>
          <code class="s-expr">{s.schedule_expr}</code>
          <span class="s-meta">
            last {ago(s.last_run)}{s.last_status ? ` · ${s.last_status}` : ''} · next {whenNext(s.next_run)}
          </span>
          <button class="btn ghost sm" on:click={() => runNow(s)}>Run</button>
          <button class="btn ghost sm" on:click={() => editSchedule(s)}>Edit</button>
          <button class="icon-btn sm danger" on:click={() => delSchedule(s)}>{ICONS.delete.glyph}</button>
        </div>
      {/each}
      {#if schedules.length === 0}<div class="empty">No schedules.</div>{/if}
    </div>
  {:else if tab === 'charts'}
    <div class="subtabs">
      <span class="muted">Saved from the Inspector's Chart mode. Click one to reopen it there.</span>
    </div>
    <div class="rows">
      {#each $savedCharts as c (c.id)}
        <div class="srow">
          <button class="ch-open" on:click={() => openChart(c)}>
            <span class="s-name">{c.name}</span>
            <span class="tag">{c.chart_type}</span>
            {#if c.row_count != null}<span class="tag">{c.row_count.toLocaleString()} rows</span>{/if}
          </button>
          <span class="s-meta">
            {c.connection_id && c.sql_text ? 'linked to a query' : 'snapshot only'} · last run {fmtRunAt(c.last_run_at)}
          </span>
          <button class="icon-btn sm danger" on:click={() => deleteChart(c)}>{ICONS.delete.glyph}</button>
        </div>
      {/each}
      {#if $savedCharts.length === 0}
        <div class="empty">
          No saved charts yet. Send a result to the Inspector, switch to Chart mode, and hit Save.
        </div>
      {/if}
    </div>
  {:else if tab === 'errors'}
    <div class="subtabs">
      <span class="muted">
        Backend, MCP, and reported frontend errors — nothing sensitive is ever logged here.
      </span>
      <span style="flex:1" />
      <button class="btn ghost sm" on:click={copyErrorLog} disabled={!errors.length}>Copy</button>
      <button class="btn ghost sm danger" on:click={clearErrors} disabled={!errors.length}>Clear</button>
    </div>
    <div class="rows">
      {#each [...errors].reverse() as e (e.ts + e.source + e.message)}
        <div class="row err-row">
          <span class="when">{ago(e.ts)}</span>
          <span class="tag" title={e.source}>{e.source}</span>
          <code class="sql err-msg">{e.message}</code>
        </div>
      {/each}
      {#if errors.length === 0}<div class="empty">No errors logged. Good sign.</div>{/if}
    </div>
  {/if}
</Modal>

<style>
  .tabs {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--border);
    margin: -14px -14px 12px;
    padding: 0 10px;
  }
  .tabs button {
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    padding: 10px 12px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    cursor: pointer;
  }
  .tabs button.active {
    color: var(--text-primary);
    border-bottom-color: var(--text-primary);
  }
  .subtabs {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .subtabs button:not(.btn) {
    background: none;
    border: none;
    font-size: 11px;
    color: var(--text-muted);
    cursor: pointer;
    padding: 3px 6px;
    border-radius: 4px;
  }
  .subtabs button.active {
    background: var(--surface-3);
    color: var(--text-primary);
    font-weight: 600;
  }
  .n {
    background: var(--surface-3);
    border-radius: 8px;
    padding: 0 5px;
    font-size: 9px;
  }
  .rows {
    max-height: 52vh;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border);
    padding: 6px 4px;
    cursor: pointer;
    font-size: 11px;
  }
  .row:hover {
    background: var(--surface-3);
  }
  .when {
    color: var(--text-muted);
    width: 62px;
    flex-shrink: 0;
  }
  .tag {
    font-size: 9px;
    text-transform: uppercase;
    background: var(--surface-3);
    padding: 1px 5px;
    border-radius: 3px;
    color: var(--text-secondary);
    flex-shrink: 0;
  }
  .method {
    font-weight: 800;
    font-size: 9px;
    width: 44px;
    flex-shrink: 0;
  }
  .m-get { color: var(--m-get); }
  .m-post { color: var(--m-post); }
  .m-put { color: var(--m-put); }
  .m-patch { color: var(--m-patch); }
  .m-delete { color: var(--m-delete); }
  .sql {
    flex: 1;
    font-family: var(--font-mono);
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .err-row {
    cursor: default;
    align-items: flex-start;
  }
  .err-row .tag {
    max-width: 170px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .err-msg {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .right {
    flex-shrink: 0;
    color: var(--text-muted);
    font-family: var(--font-mono);
  }
  .right.err {
    color: var(--danger);
  }
  .cached {
    color: var(--tool-inspector-text);
    margin-left: 4px;
  }
  .empty {
    padding: 20px;
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
  }
  .srow {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 4px;
    border-bottom: 1px solid var(--border);
    font-size: 11px;
  }
  .dot-btn {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-muted);
  }
  .dot-btn.on {
    color: var(--ok);
  }
  .s-name {
    font-weight: 600;
    font-size: 12px;
  }
  .ch-open {
    display: flex;
    align-items: center;
    gap: 8px;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .ch-open:hover .s-name {
    color: var(--tool-inspector-text);
  }
  .s-expr {
    font-family: var(--font-mono);
    background: var(--surface-3);
    padding: 1px 5px;
    border-radius: 3px;
  }
  .s-meta {
    flex: 1;
    color: var(--text-muted);
  }
  .row2 {
    display: flex;
    gap: 10px;
  }
  .row2 > .field {
    flex: 1;
  }
  .presets {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    margin-top: 4px;
  }
  .chip {
    background: var(--surface-3);
    border: none;
    border-radius: 4px;
    padding: 2px 7px;
    font-size: 10px;
    cursor: pointer;
    color: var(--text-secondary);
  }
  .toggle {
    display: flex;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    margin: 8px 0;
  }
  .editor-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 12px;
  }
  .danger {
    color: var(--danger);
  }
</style>
