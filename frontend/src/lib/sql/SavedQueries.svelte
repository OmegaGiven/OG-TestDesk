<script>
  import { createEventDispatcher, setContext } from 'svelte';
  import { writable } from 'svelte/store';
  import { api } from '../api.js';
  import FolderNode from './FolderNode.svelte';
  import { ICONS } from '../icons.js';
  import {
    savedQueries,
    savedQueryFolders,
    connections,
    reloadSavedQueries,
    toast,
    toastError,
    promptDialog,
    confirmDialog
  } from '../stores.js';

  const dispatch = createEventDispatcher();

  // Which connection's schema tree is showing next to this panel — a
  // saved query is almost always specific to the DB it was written
  // against (table/column names won't even exist elsewhere), so by
  // default this list only shows queries saved for that connection
  // (plus ones saved with no connection at all — "New query" starts
  // that way until you run/save it against something). "Show all" opts
  // back into the old unfiltered view; remembered per-browser like the
  // sidebar width.
  export let conn = null;

  const SHOW_ALL_KEY = 'ogtestdesk.sql.savedQueriesShowAll';
  let showAll = (() => {
    try {
      return localStorage.getItem(SHOW_ALL_KEY) === '1';
    } catch {
      return false;
    }
  })();
  function toggleShowAll() {
    showAll = !showAll;
    try {
      localStorage.setItem(SHOW_ALL_KEY, showAll ? '1' : '0');
    } catch {}
  }

  let filter = '';
  const collapsed = writable(new Set());
  const dragOver = writable(null);
  let drag = null; // { kind: 'q'|'f', id }

  // The store handed into FolderNode's context — visible queries only.
  const visibleQueries = writable([]);
  $: visibleQueries.set(
    showAll || !conn ? $savedQueries : $savedQueries.filter((q) => !q.connection_id || q.connection_id === conn.id)
  );
  $: hiddenCount = conn && !showAll ? $savedQueries.length - $visibleQueries.length : 0;

  $: rootFolders = $savedQueryFolders
    .filter((f) => !f.parent_id)
    .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name));
  $: rootQueries = $visibleQueries
    .filter((q) => !q.folder_id)
    .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name));

  // flat filtered view
  $: matches = filter.trim()
    ? $visibleQueries.filter((s) =>
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
    const name = await promptDialog(parentId ? 'New subfolder name:' : 'New folder name:');
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
    const name = await promptDialog('Rename folder:', f.name);
    if (!name || !name.trim() || name.trim() === f.name) return;
    try {
      await api.savedQueryFolderSave({ ...f, name: name.trim() });
      await reload();
    } catch (e) {
      toastError(e);
    }
  }
  async function deleteFolder(f) {
    if (
      !(await confirmDialog(`Delete folder "${f.name}"? Subfolders are removed; queries inside move to the top level.`, {
        danger: true
      }))
    )
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
    const name = await promptDialog('New query name:');
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
    const name = await promptDialog('Rename query:', q.name);
    if (!name || !name.trim() || name.trim() === q.name) return;
    try {
      await api.savedQuerySave({ ...q, name: name.trim() });
      await reload();
    } catch (e) {
      toastError(e);
    }
  }
  async function deleteQuery(q) {
    if (!(await confirmDialog(`Delete saved query "${q.name}"?`, { danger: true }))) return;
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
    queries: visibleQueries,
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
    <button class="icon-btn" title="New query" on:click={() => newQuery(null)}>{@html ICONS.newQuery.svg}</button>
    <button class="icon-btn" title="New folder" on:click={() => newFolder(null)}>{@html ICONS.newFolder.svg}</button>
    <button class="icon-btn" title="Refresh" on:click={reload}>{@html ICONS.refresh.svg}</button>
  </div>
  {#if conn && hiddenCount > 0}
    <button class="hidden-hint" on:click={toggleShowAll}>
      {hiddenCount} hidden from other connections — show all
    </button>
  {:else if conn && showAll}
    <button class="hidden-hint" on:click={toggleShowAll}> Showing all connections — show {connName(conn.id) || 'this one'} only </button>
  {/if}

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
          <button class="icon-btn sm" title="Rename" on:click={() => renameQuery(s)}>{@html ICONS.rename.svg}</button>
          <button class="icon-btn sm danger" title="Delete" on:click={() => deleteQuery(s)}>{@html ICONS.delete.svg}</button>
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
          <button class="icon-btn sm" title="Rename" on:click={() => renameQuery(q)}>{@html ICONS.rename.svg}</button>
          <button class="icon-btn sm danger" title="Delete" on:click={() => deleteQuery(q)}>{@html ICONS.delete.svg}</button>
        </div>
      {/each}
      {#if rootFolders.length === 0 && rootQueries.length === 0}
        <div class="none">No saved queries. Hit Save in a query tab, or the folder button to make one.</div>
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
  .hidden-hint {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border);
    cursor: pointer;
    padding: 5px 8px;
    font-size: 10.5px;
    color: var(--text-muted);
  }
  .hidden-hint:hover {
    background: var(--surface-3);
    color: var(--text-secondary);
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
  .none {
    padding: 10px;
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.5;
  }
</style>
