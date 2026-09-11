<script>
  import { onMount } from 'svelte';
  import ConnectionModal from './ConnectionModal.svelte';
  import { api } from '../api.js';
  import {
    connections,
    connMenuOpen,
    activeTool,
    newSqlTab,
    toast,
    toastError
  } from '../stores.js';

  let modal = null; // null | { existing }
  let testing = {}; // connId -> 'ok' | 'err' | undefined

  function close() {
    connMenuOpen.set(false);
  }
  function onKey(e) {
    if (e.key === 'Escape' && !modal) close();
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
      <header>
        <span>Connections</span>
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
              {#if testing[c.id] === 'ok'}<span class="st ok">●</span>{/if}
              {#if testing[c.id] === 'err'}<span class="st err">●</span>{/if}
            </button>
            <button class="edit" title="Edit" on:click={() => (modal = { existing: c })}>✎</button>
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
  .edit {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 6px 9px;
    font-size: 11px;
  }
  .edit:hover {
    color: var(--text-primary);
  }
  .empty {
    padding: 14px;
    font-size: 11px;
    color: var(--text-muted);
  }
</style>
