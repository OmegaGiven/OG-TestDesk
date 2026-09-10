<script>
  import { createEventDispatcher } from 'svelte';
  import { api } from '../api.js';
  import { loadSchemas, toastError } from '../stores.js';

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
    <button class="btn ghost sm" title="Refresh" on:click={() => load(true)}>⟳</button>
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
            <span class="chev">{openSchemas.has(schema.name) ? '▾' : '▸'}</span>
            {schema.name}
            <span class="count">{schema.relations.length}</span>
          </button>
          {#if openSchemas.has(schema.name)}
            {#each visibleRels(schema) as rel (rel.name)}
              <div class="rel-wrap">
                <div class="rel-row">
                  <button class="node rel" on:click={() => toggleRel(schema.name, rel.name)}>
                    <span class="chev">{openRels.has(`${schema.name}.${rel.name}`) ? '▾' : '▸'}</span>
                    <span class="ico">{rel.kind === 'view' ? '◇' : '▦'}</span>
                    {rel.name}
                  </button>
                  <button
                    class="peek"
                    title="SELECT * (100 rows)"
                    on:click={() => dispatch('open', { schema: schema.name, relation: rel.name })}
                  >↵</button>
                </div>
                {#if openRels.has(`${schema.name}.${rel.name}`)}
                  <div class="cols">
                    {#each cols[`${schema.name}.${rel.name}`] || [] as c}
                      <div class="col">
                        <span class="cn">{c.primary_key ? '🔑 ' : ''}{c.name}</span>
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
  .input.sm {
    padding: 5px 7px;
    font-size: 11px;
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
    gap: 5px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    cursor: pointer;
    font-size: 12px;
    color: var(--text-primary);
    padding: 4px 8px;
  }
  .node:hover {
    background: var(--surface-3);
  }
  .schema {
    font-weight: 600;
    color: var(--text-secondary);
  }
  .rel {
    padding-left: 18px;
  }
  .chev {
    font-size: 9px;
    width: 10px;
    color: var(--text-muted);
  }
  .ico {
    font-size: 10px;
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
  .peek {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px 8px;
    font-size: 12px;
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
