<script>
  import { createEventDispatcher } from 'svelte';
  export let name = null; // key or index label (string), null for root
  export let value;
  export let path;
  export let depth = 0;
  export let expandSet;
  export let selectedPath = null;
  export let filter = '';

  const dispatch = createEventDispatcher();

  $: type = value === null ? 'null' : Array.isArray(value) ? 'array' : typeof value;
  $: isContainer = type === 'array' || type === 'object';
  $: entries = !isContainer
    ? []
    : type === 'array'
      ? value.map((v, i) => [String(i), v])
      : Object.entries(value);
  $: open = expandSet.has(path) || depth === 0;
  $: size = type === 'array' ? String(value.length) : type === 'object' ? String(entries.length) : '';
  $: matches =
    !filter ||
    (name && name.toLowerCase().includes(filter.toLowerCase())) ||
    (!isContainer && String(value).toLowerCase().includes(filter.toLowerCase()));
  $: keyHit = filter && name && name.toLowerCase().includes(filter.toLowerCase());

  function toggle() {
    if (!isContainer) return;
    if (open) expandSet.delete(path);
    else expandSet.add(path);
    dispatch('expandchange');
  }
  function select() {
    dispatch('select', { path, value, type });
  }
  function preview(v) {
    if (v === null) return 'null';
    if (typeof v === 'string') return '"' + (v.length > 60 ? v.slice(0, 60) + '…' : v) + '"';
    if (typeof v === 'object')
      return Array.isArray(v) ? `[${v.length}]` : `{${Object.keys(v).length}}`;
    return String(v);
  }
</script>

{#if matches || (isContainer && open)}
  <div class="node" style="--d:{depth}">
    <div
      class="row"
      class:sel={selectedPath === path}
      on:click={select}
      on:dblclick={toggle}
      role="treeitem"
      tabindex="-1"
    >
      <span class="chev" class:hidden={!isContainer} on:click|stopPropagation={toggle}>
        {open ? '▾' : '▸'}
      </span>
      {#if name !== null}
        <span class="key" class:hit={keyHit}>{name}</span>
        <span class="colon">:</span>
      {/if}
      {#if isContainer}
        <span class="brace">{type === 'array' ? '[' : '{'}</span>
        {#if !open}
          <span class="collapsed">… {size} </span>
          <span class="brace">{type === 'array' ? ']' : '}'}</span>
        {/if}
      {:else}
        <span class="val {type}">{preview(value)}</span>
      {/if}
      {#if isContainer}<span class="badge">{type} · {size}</span>{/if}
    </div>

    {#if isContainer && open}
      <div class="children">
        {#each entries as [k, v] (k)}
          <svelte:self
            name={k}
            value={v}
            path={type === 'array' ? `${path}[${k}]` : `${path}.${k}`}
            depth={depth + 1}
            {expandSet}
            {selectedPath}
            {filter}
            on:select
            on:expandchange
          />
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .row {
    display: flex;
    align-items: baseline;
    gap: 4px;
    padding: 1px 6px 1px calc(var(--d) * 14px + 6px);
    font-family: var(--font-mono);
    font-size: 11.5px;
    cursor: pointer;
    white-space: nowrap;
  }
  .row:hover {
    background: color-mix(in srgb, var(--tool-inspector-text) 8%, transparent);
  }
  .row.sel {
    background: color-mix(in srgb, var(--tool-inspector-text) 18%, transparent);
  }
  .chev {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    align-self: stretch;
    width: 16px;
    color: var(--text-muted);
    font-size: 8px;
    cursor: pointer;
  }
  .chev:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--tool-inspector-text) 16%, transparent);
  }
  .chev.hidden {
    visibility: hidden;
    cursor: default;
  }
  .chev.hidden:hover {
    background: none;
  }
  .key {
    color: var(--j-key);
  }
  .key.hit {
    background: var(--warn);
    color: #000;
    border-radius: 2px;
  }
  .colon {
    color: var(--text-muted);
  }
  .brace,
  .collapsed {
    color: var(--text-muted);
  }
  .val.string {
    color: var(--j-string);
  }
  .val.number {
    color: var(--j-number);
  }
  .val.boolean {
    color: var(--j-bool);
  }
  .val.null {
    color: var(--j-null);
  }
  .badge {
    color: var(--text-muted);
    font-size: 9px;
    font-family: var(--font-sans);
  }
</style>
