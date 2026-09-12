<script>
  import { onMount } from 'svelte';
  import ConnectionModal from './ConnectionModal.svelte';
  import { api } from '../api.js';
  import {
    connections,
    connMenuOpen,
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

  let modal = null; // null | { existing }
  let testing = {}; // connId -> 'ok' | 'err' | undefined

  function close() {
    connMenuOpen.set(false);
  }
  function onKey(e) {
    if (e.key === 'Escape' && !modal) close();
  }

  async function openNewRequest() {
    activeTool.set('requests');
    close();
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
    close();
  }

  async function open(conn) {
    activeTool.set('sql');
    close();
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

<svelte:window on:keydown={onKey} />

{#if $connMenuOpen}
  <div class="backdrop" on:mousedown|self={close} role="presentation">
    <div class="menu">
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
              <span class="meta">
                {c.kind}{c.host ? ` · ${c.host}` : ''}{c.database ? `/${c.database}` : ''}
              </span>
              {#if testing[c.id] === 'ok'}<span class="st ok">{ICONS.connOk.glyph}</span>{/if}
              {#if testing[c.id] === 'err'}<span class="st err">{ICONS.connErr.glyph}</span>{/if}
            </button>
            <button class="icon-btn sm" title="Edit" on:click={() => (modal = { existing: c })}>{ICONS.rename.glyph}</button>
          </div>
        {/each}
        {#if $connections.length === 0}
          <div class="empty">No connections. Add one to start querying.</div>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if modal}
  <ConnectionModal
    existing={modal.existing}
    on:close={() => (modal = null)}
    on:saved={() => (modal = null)}
    on:deleted={() => (modal = null)}
  />
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 800;
  }
  .menu {
    position: absolute;
    top: 46px;
    left: 10px;
    width: 320px;
    max-height: calc(100vh - 80px);
    display: flex;
    flex-direction: column;
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
  }
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
