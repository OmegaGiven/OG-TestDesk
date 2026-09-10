<script>
  import { createEventDispatcher } from 'svelte';
  import { api } from '../api.js';
  import { savedQueries, connections, reloadSavedQueries, toast, toastError } from '../stores.js';

  const dispatch = createEventDispatcher();

  let filter = '';
  let collapsedFolders = new Set();

  $: groups = groupByFolder($savedQueries, filter);
  function groupByFolder(list, f) {
    const q = f.trim().toLowerCase();
    const map = new Map();
    for (const s of list) {
      if (q && !(`${s.folder || ''} ${s.name} ${s.sql_text}`.toLowerCase().includes(q))) continue;
      const key = s.folder || '';
      if (!map.has(key)) map.set(key, []);
      map.get(key).push(s);
    }
    return [...map.entries()]
      .sort((a, b) => (a[0] === '' ? 1 : b[0] === '' ? -1 : a[0].localeCompare(b[0])))
      .map(([folder, items]) => ({
        folder,
        items: items.sort((a, b) => a.name.localeCompare(b.name))
      }));
  }

  function connName(id) {
    return $connections.find((c) => c.id === id)?.nickname;
  }
  function toggle(folder) {
    collapsedFolders.has(folder) ? collapsedFolders.delete(folder) : collapsedFolders.add(folder);
    collapsedFolders = collapsedFolders;
  }

  async function del(s, e) {
    e.stopPropagation();
    if (!confirm(`Delete saved query "${s.name}"?`)) return;
    try {
      await api.savedQueryDelete(s.id);
      await reloadSavedQueries();
    } catch (err) {
      toastError(err);
    }
  }
  async function rename(s, e) {
    e.stopPropagation();
    const next = prompt('Rename (use folder/name to move):', s.folder ? `${s.folder}/${s.name}` : s.name);
    if (!next) return;
    const i = next.lastIndexOf('/');
    const folder = i >= 0 ? next.slice(0, i).trim() || null : null;
    const name = (i >= 0 ? next.slice(i + 1) : next).trim();
    try {
      await api.savedQuerySave({ ...s, folder, name });
      await reloadSavedQueries();
      toast('Saved query updated', 'success', 1500);
    } catch (err) {
      toastError(err);
    }
  }
</script>

<div class="sq">
  <div class="sq-tools">
    <input class="input sm" placeholder="Filter saved queries…" bind:value={filter} />
    <button class="btn ghost sm" title="Refresh" on:click={reloadSavedQueries}>⟳</button>
  </div>

  <div class="sq-scroll">
    {#each groups as g (g.folder)}
      <button class="folder" on:click={() => toggle(g.folder)}>
        <span class="chev">{collapsedFolders.has(g.folder) ? '▸' : '▾'}</span>
        {g.folder || 'Ungrouped'}
        <span class="count">{g.items.length}</span>
      </button>
      {#if !collapsedFolders.has(g.folder)}
        {#each g.items as s (s.id)}
          <div class="q-row">
            <button
              class="q-main"
              title={s.sql_text}
              on:click={() => dispatch('open', s)}
            >
              <span class="q-name">{s.name}</span>
              {#if connName(s.connection_id)}<span class="q-conn">{connName(s.connection_id)}</span>{/if}
            </button>
            <button class="q-act" title="Rename / move" on:click={(e) => rename(s, e)}>✎</button>
            <button class="q-act" title="Delete" on:click={(e) => del(s, e)}>✕</button>
          </div>
        {/each}
      {/if}
    {/each}
    {#if groups.length === 0}
      <div class="none">{filter ? 'No matches.' : 'No saved queries. Hit Save in a query tab.'}</div>
    {/if}
  </div>
</div>

<style>
  .sq {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    height: 100%;
  }
  .sq-tools {
    display: flex;
    gap: 4px;
    padding: 6px;
    border-bottom: 1px solid var(--border);
  }
  .sq-tools .input.sm {
    padding: 5px 7px;
    font-size: 11px;
  }
  .sq-scroll {
    overflow: auto;
    padding: 4px 0 10px;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    cursor: pointer;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-secondary);
    padding: 5px 8px 3px;
  }
  .folder:hover {
    background: var(--surface-3);
  }
  .chev {
    font-size: 9px;
    color: var(--text-muted);
  }
  .count {
    margin-left: auto;
    font-size: 9px;
    color: var(--text-muted);
    font-weight: 400;
  }
  .q-row {
    display: flex;
    align-items: center;
  }
  .q-row:hover {
    background: var(--surface-3);
  }
  .q-main {
    flex: 1;
    display: flex;
    align-items: baseline;
    gap: 6px;
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    padding: 4px 8px 4px 20px;
    overflow: hidden;
  }
  .q-name {
    font-size: 12px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .q-conn {
    font-size: 9px;
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .q-act {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px 6px;
    font-size: 10px;
  }
  .q-act:hover {
    color: var(--text-primary);
  }
  .none {
    padding: 10px;
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.5;
  }
</style>
