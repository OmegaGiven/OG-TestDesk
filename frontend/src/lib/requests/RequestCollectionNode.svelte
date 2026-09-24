<script>
  import { getContext } from 'svelte';
  import { ICONS } from '../icons.js';

  export let collectionId;
  export let depth = 0;

  const ctx = getContext('reqtree');
  const { collections, requests, collapsed, dragOverKey, selectedIds } = ctx;

  $: col = $collections.find((c) => c.id === collectionId);
  $: subs = $collections
    .filter((c) => c.parent_id === collectionId)
    .sort((a, b) => a.name.localeCompare(b.name));
  $: items = $requests
    .filter((r) => r.collection_id === collectionId)
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.name.localeCompare(b.name));
  $: open = !$collapsed.has(collectionId);
  $: isOver = $dragOverKey === 'c:' + collectionId;
</script>

{#if col}
  <div class="cnode">
    <div
      class="chead"
      class:over={isOver}
      style="padding-left: {6 + depth * 12}px"
      draggable="true"
      on:dragstart|stopPropagation={(e) => ctx.beginDragCollection(e, collectionId)}
      on:click={() => ctx.toggle(collectionId)}
      on:dragover|preventDefault|stopPropagation={() => ctx.setDragOverKey('c:' + collectionId)}
      on:dragleave|stopPropagation={() => ctx.setDragOverKey(null)}
      on:drop|preventDefault|stopPropagation={() => ctx.dropOnCollection(collectionId)}
    >
      <span class="chev">{@html open ? ICONS.expandOpen.svg : ICONS.expandClosed.svg}</span>
      <button class="cname" title="Rename" on:click|stopPropagation={() => ctx.renameCollection(col)}>{col.name}</button>
      <span class="ccnt">{subs.length + items.length}</span>
      <span class="cacts">
        <button
          class="icon-btn sm"
          title="New subfolder"
          on:click|stopPropagation={() => ctx.newSubcollection(collectionId)}
        >{@html ICONS.newSubfolder.svg}</button>
        <button
          class="icon-btn sm danger"
          title="Delete collection"
          on:click|stopPropagation={() => ctx.deleteCollection(col)}
        >{@html ICONS.delete.svg}</button>
      </span>
    </div>
    {#if open}
      {#each subs as s (s.id)}
        <svelte:self collectionId={s.id} depth={depth + 1} />
      {/each}
      {#each items as r (r.id)}
        <div
          class="req-item"
          style="padding-left: {6 + (depth + 1) * 12}px"
          draggable="true"
          on:dragstart|stopPropagation={(e) => ctx.beginDragReq(e, r.id)}
        >
          <input
            type="checkbox"
            class="ri-check"
            checked={$selectedIds.has(r.id)}
            on:click|stopPropagation={() => ctx.toggleSelect(r.id)}
          />
          <button class="ri-main" on:click={() => ctx.openRequest(r)}>
            <span class="mm" style="color:var(--m-{r.method.toLowerCase()})">{r.method}</span>
            <span class="rn">{r.name}</span>
          </button>
          <button class="del" on:click={() => ctx.deleteRequest(r)}>{@html ICONS.delete.svg}</button>
        </div>
      {/each}
    {/if}
  </div>
{/if}

<style>
  .chead {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 6px 8px 6px 0;
    margin-top: 2px;
    cursor: pointer;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .chead:hover {
    background: var(--surface-3);
  }
  .chead.over {
    background: color-mix(in srgb, var(--tool-requests-text) 22%, transparent);
    box-shadow: inset 0 0 0 1px var(--tool-requests-text);
  }
  .chev {
    font-size: 9px;
    width: 10px;
    text-align: center;
    flex-shrink: 0;
  }
  .cname {
    flex: 1;
    text-align: left;
    background: none;
    border: none;
    color: inherit;
    font: inherit;
    text-transform: inherit;
    letter-spacing: inherit;
    cursor: pointer;
    padding: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ccnt {
    font-weight: 400;
    flex-shrink: 0;
  }
  .cacts {
    margin-left: auto;
    display: flex;
    gap: 1px;
    opacity: 0;
  }
  .chead:hover .cacts {
    opacity: 1;
  }
  .req-item {
    display: flex;
    align-items: center;
  }
  .ri-check {
    flex-shrink: 0;
    margin-left: 8px;
    cursor: pointer;
  }
  .ri-main {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 7px;
    background: none;
    border: none;
    color: var(--text-primary);
    padding: 5px 8px;
    cursor: pointer;
    text-align: left;
    overflow: hidden;
  }
  .mm {
    font-size: 9px;
    font-weight: 800;
    width: 42px;
    flex-shrink: 0;
  }
  .rn {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .del {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px 7px;
    font-size: 10px;
  }
  .del:hover {
    color: var(--danger);
  }
</style>
