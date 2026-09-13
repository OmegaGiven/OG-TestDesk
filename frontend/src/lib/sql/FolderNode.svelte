<script>
  import { getContext } from 'svelte';
  import { ICONS } from '../icons.js';

  export let folderId;
  export let depth = 0;

  const ctx = getContext('sqtree');
  const { folders, queries, collapsed, dragOver } = ctx;

  $: folder = $folders.find((f) => f.id === folderId);
  $: subs = $folders
    .filter((f) => f.parent_id === folderId)
    .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name));
  $: qs = $queries
    .filter((q) => q.folder_id === folderId)
    .sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name));
  $: open = !$collapsed.has(folderId);
  $: isOver = $dragOver === 'f:' + folderId;
</script>

{#if folder}
  <div class="fnode">
    <div
      class="frow"
      class:over={isOver}
      style="padding-left: {6 + depth * 12}px"
      draggable="true"
      on:dragstart|stopPropagation={(e) => ctx.beginDrag(e, 'f', folderId)}
      on:dragover|preventDefault={() => ctx.setDragOver('f:' + folderId)}
      on:dragleave={() => ctx.setDragOver(null)}
      on:drop|preventDefault|stopPropagation={() => ctx.drop(folderId)}
    >
      <button class="icon-btn sm chev" on:click={() => ctx.toggle(folderId)}
        >{@html open ? ICONS.expandOpen.svg : ICONS.expandClosed.svg}</button
      >
      <button class="fname" title="Rename" on:click={() => ctx.renameFolder(folder)}>{folder.name}</button>
      <span class="fcount">{subs.length + qs.length}</span>
      <span class="facts">
        <button class="icon-btn sm" title="New subfolder" on:click={() => ctx.newFolder(folderId)}
          >{@html ICONS.newSubfolder.svg}</button
        >
        <button class="icon-btn sm" title="New query here" on:click={() => ctx.newQuery(folderId)}
          >{@html ICONS.newQueryHere.svg}</button
        >
        <button class="icon-btn sm danger" title="Delete folder" on:click={() => ctx.deleteFolder(folder)}
          >{@html ICONS.delete.svg}</button
        >
      </span>
    </div>
    {#if open}
      {#each subs as s (s.id)}
        <svelte:self folderId={s.id} depth={depth + 1} />
      {/each}
      {#each qs as q (q.id)}
        <div
          class="qrow"
          style="padding-left: {6 + (depth + 1) * 12}px"
          draggable="true"
          on:dragstart|stopPropagation={(e) => ctx.beginDrag(e, 'q', q.id)}
        >
          <button class="qmain" title={q.sql_text} on:click={() => ctx.openQuery(q)}>
            <span class="qname">{q.name}</span>
            {#if ctx.connName(q.connection_id)}<span class="qconn">{ctx.connName(q.connection_id)}</span>{/if}
          </button>
          <button class="icon-btn sm" title="Rename" on:click={() => ctx.renameQuery(q)}>{@html ICONS.rename.svg}</button>
          <button class="icon-btn sm danger" title="Delete" on:click={() => ctx.deleteQuery(q)}
            >{@html ICONS.delete.svg}</button
          >
        </div>
      {/each}
    {/if}
  </div>
{/if}

<style>
  .frow {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 6px 3px 0;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.02em;
    color: var(--text-secondary);
  }
  .frow:hover {
    background: var(--surface-3);
  }
  .frow.over {
    background: color-mix(in srgb, var(--tool-sql-text) 22%, transparent);
    box-shadow: inset 0 0 0 1px var(--tool-sql-text);
  }
  .fname {
    background: none;
    border: none;
    cursor: pointer;
    color: inherit;
    font: inherit;
    text-transform: inherit;
    letter-spacing: inherit;
    padding: 2px 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fcount {
    font-size: 9px;
    color: var(--text-muted);
    font-weight: 400;
  }
  .facts {
    margin-left: auto;
    display: flex;
    gap: 1px;
    opacity: 0;
  }
  .frow:hover .facts {
    opacity: 1;
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
  .qrow :global(.icon-btn) {
    opacity: 0;
  }
  .qrow:hover :global(.icon-btn) {
    opacity: 1;
  }
</style>
