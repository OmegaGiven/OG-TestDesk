<script>
  import { createEventDispatcher, setContext } from 'svelte';
  import { writable } from 'svelte/store';
  import { api } from '../api.js';
  import FolderNode from './FolderNode.svelte';
  import {
    savedQueries,
    savedQueryFolders,
    connections,
    reloadSavedQueries,
    toast,
    toastError
  } from '../stores.js';

  const dispatch = createEventDispatcher();

  let filter = '';
  const collapsed = writable(new Set());
  const dragOver = writable(null);
  let drag = null; // { kind: 'q'|'f', id }

  $: rootFolders = $savedQueryFolders
    .filter((f) => !f.parent_id)
    .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name));
  $: rootQueries = $savedQueries
    .filter((q) => !q.folder_id)
    .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name));

  // flat filtered view
  $: matches = filter.trim()
    ? $savedQueries.filter((s) =>
        `${folderPath(s.folder_id)} ${s.name} ${s.sql_text}`
          .toLowerCase()
          .includes(filter.trim().toLowerCase())
      )
    : null;

  function folderPath(id) {
    const parts = [];
    let cur = id;
    const seen = new Set();
    while (cur && !seen.has(cur)) {
      seen.add(cur);
      const f = $savedQueryFolders.find((x) => x.id === cur);
      if (!f) break;
      parts.unshift(f.name);
      cur = f.parent_id;
    }
    return parts.join(' / ');
  }
  function connName(id) {
    return $connections.find((c) => c.id === id)?.nickname;
  }
  function toggle(id) {
    collapsed.update((s) => {
      s.has(id) ? s.delete(id) : s.add(id);
      return new Set(s);
    });
  }
  function descendants(fid) {
    const out = new Set([fid]);
    let grew = true;
    while (grew) {
      grew = false;
      for (const f of $savedQueryFolders) {
        if (f.parent_id && out.has(f.parent_id) && !out.has(f.id)) {
          out.add(f.id);
          grew = true;
        }
      }
    }
    return out;
  }

  async function reload() {
    await reloadSavedQueries();
  }

  // -- folder ops
  async function newFolder(parentId) {
    const name = prompt(parentId ? 'New subfolder name:' : 'New folder name:');
    if (!name || !name.trim()) return;
    try {
      await api.savedQueryFolderSave({ id: '', name: name.trim(), parent_id: parentId || null, sort_order: 0 });
      if (parentId) collapsed.update((s) => (s.delete(parentId), new Set(s)));
      await reload();
    } catch (e) {
      toastError(e);
    }
  }
  async function renameFolder(f) {
    const name = prompt('Rename folder:', f.name);
    if (!name || !name.trim() || name.trim() === f.name) return;
    try {
      await api.savedQueryFolderSave({ ...f, name: name.trim() });
      await reload();
    } catch (e) {
      toastError(e);
    }
  }
  async function deleteFolder(f) {
    if (!confirm(`Delete folder "${f.name}"? Subfolders are removed; queries inside move to the top level.`))
      return;
    try {
      await api.savedQueryFolderDelete(f.id);
      await reload();
    } catch (e) {
      toastError(e);
    }
  }

  // -- query ops
  async function newQuery(folderId) {
    const name = prompt('New query name:');
    if (!name || !name.trim()) return;
    try {
      const saved = await api.savedQuerySave({
        id: '',
        connection_id: null,
        folder_id: folderId || null,
        name: name.trim(),
        sql_text: '',
        sort_order: 0,
        created_at: 0
      });
      await reload();
      dispatch('open', saved);
    } catch (e) {
      toastError(e);
    }
  }
  function openQuery(q) {
    dispatch('open', q);
  }
  async function renameQuery(q) {
    const name = prompt('Rename query:', q.name);
    if (!name || !name.trim() || name.trim() === q.name) return;
    try {
      await api.savedQuerySave({ ...q, name: name.trim() });
      await reload();
    } catch (e) {
      toastError(e);
    }
  }
  async function deleteQuery(q) {
    if (!confirm(`Delete saved query "${q.name}"?`)) return;
    try {
      await api.savedQueryDelete(q.id);
      await reload();
    } catch (e) {
      toastError(e);
    }
  }

  // -- drag & drop
  function beginDrag(e, kind, id) {
    drag = { kind, id };
    e.dataTransfer.effectAllowed = 'move';
    try {
      e.dataTransfer.setData('text/plain', `${kind}:${id}`);
    } catch {}
  }
  function setDragOver(key) {
    dragOver.set(key);
  }
  async function drop(targetFolderId) {
    dragOver.set(null);
    const d = drag;
    drag = null;
    if (!d) return;
    try {
      if (d.kind === 'q') {
        const q = $savedQueries.find((x) => x.id === d.id);
        if (!q || q.folder_id === (targetFolderId || null)) return;
        await api.savedQuerySave({ ...q, folder_id: targetFolderId || null });
      } else {
        const f = $savedQueryFolders.find((x) => x.id === d.id);
        if (!f || f.parent_id === (targetFolderId || null)) return;
        if (targetFolderId && descendants(d.id).has(targetFolderId)) {
          toast('Cannot move a folder into itself', 'error', 2000);
          return;
        }
        await api.savedQueryFolderSave({ ...f, parent_id: targetFolderId || null });
      }
      await reload();
    } catch (e) {
      toastError(e);
    }
  }

  setContext('sqtree', {
    folders: savedQueryFolders,
    queries: savedQueries,
    collapsed,
    dragOver,
    toggle,
    connName,
    beginDrag,
    setDragOver,
    drop,
    newFolder,
    renameFolder,
    deleteFolder,
    newQuery,
    renameQuery,
    deleteQuery,
    openQuery
  });
</script>

<div class="sq">
  <div class="sq-tools">
    <input class="input sm" placeholder="Filter saved queries…" bind:value={filter} />
    <button class="btn ghost sm" title="New query" on:click={() => newQuery(null)}>＋≡</button>
    <button class="btn ghost sm" title="New folder" on:click={() => newFolder(null)}>＋⌸</button>
    <button class="btn ghost sm" title="Refresh" on:click={reload}>⟳</button>
  </div>

  <div
    class="sq-scroll"
    class:over={$dragOver === 'root'}
    on:dragover|preventDefault={() => setDragOver('root')}
    on:dragleave={() => setDragOver(null)}
    on:drop|preventDefault={() => drop(null)}
  >
    {#if matches}
      {#each matches as s (s.id)}
        <div class="qrow">
          <button class="qmain" title={s.sql_text} on:click={() => openQuery(s)}>
            <span class="qname">{s.name}</span>
            {#if folderPath(s.folder_id)}<span class="qconn">{folderPath(s.folder_id)}</span>{/if}
          </button>
          <button class="qact" title="Rename" on:click={() => renameQuery(s)}>✎</button>
          <button class="qact" title="Delete" on:click={() => deleteQuery(s)}>✕</button>
        </div>
      {/each}
      {#if matches.length === 0}<div class="none">No matches.</div>{/if}
    {:else}
      {#each rootFolders as f (f.id)}
        <FolderNode folderId={f.id} depth={0} />
      {/each}
      {#each rootQueries as q (q.id)}
        <div class="qrow" style="padding-left:6px" draggable="true" on:dragstart={(e) => beginDrag(e, 'q', q.id)}>
          <button class="qmain" title={q.sql_text} on:click={() => openQuery(q)}>
            <span class="qname">{q.name}</span>
            {#if connName(q.connection_id)}<span class="qconn">{connName(q.connection_id)}</span>{/if}
          </button>
          <button class="qact" title="Rename" on:click={() => renameQuery(q)}>✎</button>
          <button class="qact" title="Delete" on:click={() => deleteQuery(q)}>✕</button>
        </div>
      {/each}
      {#if rootFolders.length === 0 && rootQueries.length === 0}
        <div class="none">No saved queries. Hit Save in a query tab, or ＋⌸ to make a folder.</div>
      {/if}
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
    flex: 1;
    padding: 6px 8px;
    font-size: 11.5px;
  }
  .sq-tools :global(.btn) {
    font-size: 13px;
    line-height: 1;
    padding: 4px 8px;
  }
  .sq-scroll {
    overflow: auto;
    padding: 4px 0 10px;
    flex: 1;
  }
  .sq-scroll.over {
    box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--tool-sql-text) 50%, transparent);
  }
  .qrow {
    display: flex;
    align-items: center;
  }
  .qrow:hover {
    background: var(--surface-3);
  }
  .qmain {
    flex: 1;
    display: flex;
    align-items: baseline;
    gap: 6px;
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    padding: 4px 8px;
    overflow: hidden;
  }
  .qname {
    font-size: 12px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .qconn {
    font-size: 9px;
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .qact {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 5px 6px;
    font-size: 12px;
    line-height: 1;
  }
  .qact:hover {
    color: var(--text-primary);
    background: var(--surface-3);
  }
  .none {
    padding: 10px;
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.5;
  }
</style>
