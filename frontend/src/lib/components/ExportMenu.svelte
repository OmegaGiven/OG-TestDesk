<script>
  import { createEventDispatcher } from 'svelte';
  import { ICONS } from '../icons.js';

  // {key, label} entries — e.g. [{key:'csv',label:'CSV'},{key:'tsv',label:'TSV'},{key:'json',label:'JSON'}]
  export let formats = [];
  export let showCopy = true;
  export let copyTitle = 'Copy as TSV (paste into a spreadsheet)';
  export let label = 'Export';

  const dispatch = createEventDispatcher();
  let open = false;
  let withHeaders = true;

  function pick(fmt) {
    open = false;
    dispatch('export', { format: fmt, withHeaders });
  }
  function copy() {
    open = false;
    dispatch('copy', { withHeaders });
  }
</script>

<span class="export-menu">
  <button class="btn ghost sm" on:click={() => (open = !open)} class:on={open}>
    {label} {@html ICONS.expandOpen.svg}
  </button>
  {#if open}
    <div class="backdrop" on:click={() => (open = false)} role="presentation" />
    <div class="menu">
      <label class="hdr-toggle">
        <input type="checkbox" bind:checked={withHeaders} />
        Include headers
      </label>
      <div class="sep" />
      {#each formats as f (f.key)}
        <button class="item" on:click={() => pick(f.key)}>Save as {f.label}…</button>
      {/each}
      {#if showCopy}
        <button class="item" title={copyTitle} on:click={copy}>{@html ICONS.copy.svg} Copy to clipboard</button>
      {/if}
    </div>
  {/if}
</span>

<style>
  .export-menu {
    position: relative;
  }
  .btn.on {
    background: var(--surface-3);
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 800;
  }
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 801;
    min-width: 190px;
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-pop);
    padding: 6px;
  }
  .hdr-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 6px;
    font-size: 11.5px;
    color: var(--text-secondary);
    cursor: pointer;
  }
  .sep {
    height: 1px;
    background: var(--border);
    margin: 4px 0;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    cursor: pointer;
    padding: 6px;
    font-size: 12px;
    color: var(--text-primary);
    border-radius: 4px;
  }
  .item:hover {
    background: var(--surface-3);
  }
</style>
