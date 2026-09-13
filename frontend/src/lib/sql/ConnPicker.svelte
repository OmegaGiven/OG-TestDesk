<script>
  import ConnectionModal from './ConnectionModal.svelte';
  import { api } from '../api.js';
  import {
    connections,
    activeTool,
    newSqlTab,
    newRequestTab,
    inspectorOpen,
    moveTabToGroup,
    INSPECTOR_TAB_ID,
    toast,
    toastError
  } from '../stores.js';
  import { ICONS } from '../icons.js';

  // Shared "pick something to open" content — used both inside the
  // top-bar + menu's dropdown (ConnectionsMenu.svelte) and, embedded
  // directly, as the SQL view's empty state once every tab is closed.
  // `onDone` fires after a selection actually opens something, so the
  // popup version can close itself; the embedded version just leaves it
  // unset since it has nothing to close (the empty state disappears on
  // its own once a tab exists).
  export let onDone = () => {};
  // Bindable so a wrapping popup (ConnectionsMenu) can tell whether the
  // connection-edit modal is open on top of it, to suppress its own
  // Escape-to-close while that's up.
  export let modal = null; // null | { existing }
  let testing = {}; // connId -> 'ok' | 'err' | undefined

  async function openNewRequest() {
    activeTool.set('requests');
    onDone();
    try {
      await newRequestTab();
    } catch (e) {
      toastError(e);
    }
  }
  function openInspector() {
    inspectorOpen.set(true);
    moveTabToGroup('inspector', { id: INSPECTOR_TAB_ID }, null);
    activeTool.set('inspector');
    onDone();
  }

  async function open(conn) {
    activeTool.set('sql');
    onDone();
    try {
      const t = await newSqlTab(conn.id);
      toast(`${conn.nickname} — new query`, 'success', 1400);
      // eagerly probe the connection so the user sees failures fast
      api
        .connectionTest(conn, null)
        .then(() => (testing = { ...testing, [conn.id]: 'ok' }))
        .catch(() => (testing = { ...testing, [conn.id]: 'err' }));
      void t;
    } catch (e) {
      toastError(e);
    }
  }
</script>

<div class="other-tools">
  <button class="tool-row" on:click={openNewRequest}>
    <span class="tool-dot" style="background:var(--tool-requests-text)" />
    New request
  </button>
  <button class="tool-row" on:click={openInspector}>
    <span class="tool-dot" style="background:var(--tool-inspector-text)" />
    Inspector
  </button>
</div>

<header>
  <span>DB Connections</span>
  <button class="btn ghost sm" on:click={() => (modal = { existing: null })}>+ New</button>
</header>

<div class="list">
  {#each $connections as c (c.id)}
    <div class="row">
      <button class="open" on:click={() => open(c)} title="Open a query tab on {c.nickname}">
        <span class="dot" style="background:{c.color || 'var(--conn-slate)'}" />
        <span class="name">{c.nickname}</span>
        {#if c.read_only}<span class="ro" title="Read-only — writes are blocked">RO</span>{/if}
        <span class="meta">
          {c.kind}{c.host ? ` · ${c.host}` : ''}{c.database ? `/${c.database}` : ''}
        </span>
        {#if testing[c.id] === 'ok'}<span class="st ok">{@html ICONS.connOk.svg}</span>{/if}
        {#if testing[c.id] === 'err'}<span class="st err">{@html ICONS.connErr.svg}</span>{/if}
      </button>
      <button class="icon-btn sm" title="Edit" on:click={() => (modal = { existing: c })}>{@html ICONS.rename.svg}</button>
    </div>
  {/each}
  {#if $connections.length === 0}
    <div class="empty">No connections. Add one to start querying.</div>
  {/if}
</div>

{#if modal}
  <ConnectionModal
    existing={modal.existing}
    on:close={() => (modal = null)}
    on:saved={() => (modal = null)}
    on:deleted={() => (modal = null)}
  />
{/if}

<style>
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-secondary);
  }
  .list {
    overflow: auto;
    padding: 4px 0;
  }
  .row {
    display: flex;
    align-items: center;
  }
  .row:hover {
    background: var(--surface-3);
  }
  .open {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 7px;
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    padding: 7px 8px;
    overflow: hidden;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .name {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text-primary);
    flex-shrink: 0;
  }
  .meta {
    font-size: 10px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ro {
    font-size: 9px;
    font-family: var(--font-mono);
    font-weight: 700;
    color: var(--warn);
    border: 1px solid currentColor;
    border-radius: 3px;
    padding: 0 3px;
    flex-shrink: 0;
  }
  .st {
    margin-left: auto;
    font-size: 8px;
  }
  .st.ok {
    color: var(--ok);
  }
  .st.err {
    color: var(--danger);
  }
  .empty {
    padding: 14px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .other-tools {
    border-bottom: 1px solid var(--border);
    padding: 4px 0;
  }
  .tool-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    cursor: pointer;
    padding: 7px 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .tool-row:hover {
    background: var(--surface-3);
  }
  .tool-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
</style>
