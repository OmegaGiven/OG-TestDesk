<script>
  import { createEventDispatcher } from 'svelte';
  import { api } from '../api.js';
  import { loadSchemas, toastError } from '../stores.js';
  import { ICONS } from '../icons.js';

  export let conn;
  const dispatch = createEventDispatcher();

  let schemas = [];
  let loading = false;
  let filter = '';
  let openSchemas = new Set();
  let openRels = new Set(); // key `${schema}.${rel}`
  let cols = {}; // key -> Column[]

  $: if (conn) load();

  async function load(force = false) {
    loading = true;
    try {
      schemas = await loadSchemas(conn, force);
      if (schemas.length === 1) openSchemas = new Set([schemas[0].name]);
    } catch (e) {
      toastError(e);
      schemas = [];
    } finally {
      loading = false;
    }
  }

  function toggleSchema(name) {
    openSchemas.has(name) ? openSchemas.delete(name) : openSchemas.add(name);
    openSchemas = openSchemas;
  }

  async function toggleRel(schema, rel) {
    const key = `${schema}.${rel}`;
    if (openRels.has(key)) {
      openRels.delete(key);
    } else {
      openRels.add(key);
      if (!cols[key]) {
        try {
          cols[key] = await api.columnsList(conn, schema, rel);
          cols = cols;
        } catch (e) {
          toastError(e);
        }
      }
    }
    openRels = openRels;
  }

  function match(s) {
    return !filter || s.toLowerCase().includes(filter.toLowerCase());
  }
  function visibleRels(schema) {
    return schema.relations.filter((r) => match(r.name) || match(schema.name));
  }
</script>

<div class="tree">
  <div class="tools">
    <input class="input sm" placeholder="Filter tables…" bind:value={filter} />
    <button class="icon-btn" title="Refresh" on:click={() => load(true)}>{ICONS.refresh.glyph}</button>
  </div>

  {#if loading && schemas.length === 0}
    <div class="msg">Loading schema…</div>
  {:else if schemas.length === 0}
    <div class="msg">No tables.</div>
  {:else}
    <div class="scroll">
      {#each schemas as schema (schema.name)}
        {#if visibleRels(schema).length || !filter}
          <button class="node schema" on:click={() => toggleSchema(schema.name)}>
            <span class="chev">{openSchemas.has(schema.name) ? ICONS.expandOpen.glyph : ICONS.expandClosed.glyph}</span>
            {schema.name}
            <span class="count">{schema.relations.length}</span>
          </button>
          {#if openSchemas.has(schema.name)}
            {#each visibleRels(schema) as rel (rel.name)}
              <div class="rel-wrap">
                <div class="rel-row">
                  <button class="node rel" on:click={() => toggleRel(schema.name, rel.name)}>
                    <span class="chev"
                      >{openRels.has(`${schema.name}.${rel.name}`)
                        ? ICONS.expandOpen.glyph
                        : ICONS.expandClosed.glyph}</span
                    >
                    <span class="ico">{rel.kind === 'view' ? ICONS.view.glyph : ICONS.table.glyph}</span>
                    {rel.name}
                  </button>
                  <button
                    class="icon-btn peek"
                    title="Open the full table, paginated"
                    on:click={() => dispatch('open', { schema: schema.name, relation: rel.name })}
                  >{ICONS.insertName.glyph}</button>
                </div>
                {#if openRels.has(`${schema.name}.${rel.name}`)}
                  <div class="cols">
                    {#each cols[`${schema.name}.${rel.name}`] || [] as c}
                      <div class="col">
                        <span class="cn">{c.primary_key ? ICONS.primaryKey.glyph + ' ' : ''}{c.name}</span>
                        <span class="ct">{c.data_type}{c.nullable ? '' : ' ·'}</span>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/each}
          {/if}
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }
  .tools {
    display: flex;
    gap: 4px;
    padding: 6px;
    border-bottom: 1px solid var(--border);
  }
  .tools .input.sm {
    flex: 1;
    padding: 6px 8px;
    font-size: 11.5px;
  }
  .scroll {
    overflow: auto;
    padding: 4px 0 12px;
  }
  .msg {
    padding: 12px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .node {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    cursor: pointer;
    font-size: 12.5px;
    color: var(--text-primary);
    padding: 5px 8px;
  }
  .node:hover {
    background: var(--surface-3);
  }
  .schema {
    font-weight: 600;
    color: var(--text-secondary);
  }
  .rel {
    padding-left: 16px;
  }
  .chev {
    font-size: 11px;
    width: 12px;
    text-align: center;
    color: var(--text-muted);
  }
  .ico {
    font-size: 13px;
    color: var(--tool-sql-text);
  }
  .count {
    margin-left: auto;
    font-size: 10px;
    color: var(--text-muted);
  }
  .rel-row {
    display: flex;
    align-items: center;
  }
  .rel-row .node {
    flex: 1;
  }
  .peek:hover {
    color: var(--tool-sql-text);
  }
  .cols {
    padding: 2px 8px 6px 40px;
  }
  .col {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 10.5px;
    padding: 2px 0;
    font-family: var(--font-mono);
  }
  .cn {
    color: var(--text-primary);
  }
  .ct {
    color: var(--text-muted);
  }
</style>
