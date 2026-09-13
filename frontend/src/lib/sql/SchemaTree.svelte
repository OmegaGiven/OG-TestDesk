<script>
  import { createEventDispatcher } from 'svelte';
  import { api } from '../api.js';
  import { loadSchemas, toast, toastError } from '../stores.js';
  import { ICONS } from '../icons.js';

  export let conn;
  const dispatch = createEventDispatcher();

  let schemas = [];
  let loading = false;
  let filter = '';
  let openSchemas = new Set();
  let openRels = new Set(); // key `${schema}.${rel}`
  let cols = {}; // key -> Column[]
  let lastLoadConn = null; // guard `load()` by id, not object identity —
  // `conn` gets a brand-new object every time the connections list is
  // refetched (e.g. on window focus), which would otherwise re-run this
  // on every focus even though nothing about the connection changed

  // "Tables" vs "Functions" tab
  let browseTab = 'tables';
  let funcs = null; // null = not loaded yet for this connection
  let funcsLoading = false;
  let lastFuncsConn = null;

  $: if (conn && conn.id !== lastFuncsConn) {
    funcs = null;
    lastFuncsConn = conn.id;
  }
  $: if (conn && browseTab === 'functions' && funcs === null && !funcsLoading) loadFunctions();

  async function loadFunctions() {
    funcsLoading = true;
    try {
      funcs = await api.functionsList(conn);
    } catch (e) {
      toastError(e);
      funcs = [];
    } finally {
      funcsLoading = false;
    }
  }
  $: funcsBySchema = (() => {
    const q = filter.trim().toLowerCase();
    const rows = (funcs || []).filter((f) => !q || f.name.toLowerCase().includes(q) || f.schema.toLowerCase().includes(q));
    const bySchema = new Map();
    for (const f of rows) {
      if (!bySchema.has(f.schema)) bySchema.set(f.schema, []);
      bySchema.get(f.schema).push(f);
    }
    return [...bySchema.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  })();
  async function copyFunctionCall(f) {
    const text = `${f.schema}.${f.name}(${f.arguments})`;
    try {
      await navigator.clipboard.writeText(text);
      toast(`Copied ${f.name}(...)`, 'success', 1500);
    } catch {}
  }

  // relationships popup (foreign keys) — position:fixed anchored under the
  // button so it isn't clipped by the sidebar's overflow:hidden
  let fkMenuOpen = false;
  let fks = null; // null = not loaded yet for this connection
  let fkLoading = false;
  let fkFilter = '';
  let lastFkConn = null;
  let fkBtn;
  let fkPos = { top: 0, left: 0 };

  const devFkMenu = typeof location !== 'undefined' && new URLSearchParams(location.search).has('fkmenu');
  if (typeof location !== 'undefined' && new URLSearchParams(location.search).get('browsetab') === 'functions') {
    browseTab = 'functions';
  }
  $: if (conn && conn.id !== lastLoadConn) {
    lastLoadConn = conn.id;
    load();
  }
  $: if (conn && devFkMenu && !fkMenuOpen) openFks();
  $: if (conn && conn.id !== lastFkConn) {
    fks = null;
    lastFkConn = conn.id;
  }

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

  async function openFks() {
    fkMenuOpen = !fkMenuOpen;
    if (fkMenuOpen && fkBtn) {
      const r = fkBtn.getBoundingClientRect();
      fkPos = { top: r.bottom + 4, left: r.left };
    }
    if (fkMenuOpen && fks === null && !fkLoading) {
      fkLoading = true;
      try {
        fks = await api.foreignKeysList(conn);
      } catch (e) {
        toastError(e);
        fks = [];
      } finally {
        fkLoading = false;
      }
    }
  }
  $: fkByTable = (() => {
    const q = fkFilter.trim().toLowerCase();
    const rows = (fks || []).filter(
      (f) =>
        !q ||
        f.table.toLowerCase().includes(q) ||
        f.ref_table.toLowerCase().includes(q) ||
        f.column.toLowerCase().includes(q)
    );
    const byTable = new Map();
    for (const f of rows) {
      if (!byTable.has(f.table)) byTable.set(f.table, []);
      byTable.get(f.table).push(f);
    }
    return [...byTable.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  })();

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
    <input
      class="input sm"
      placeholder={browseTab === 'functions' ? 'Filter functions…' : 'Filter tables…'}
      bind:value={filter}
    />
    <span class="fk-wrap">
      <button
        class="icon-btn"
        class:on={fkMenuOpen}
        title={ICONS.relationships.label}
        bind:this={fkBtn}
        on:click={openFks}>{@html ICONS.relationships.svg}</button
      >
      {#if fkMenuOpen}
        <div class="backdrop" on:click={() => (fkMenuOpen = false)} role="presentation" />
        <div class="fk-menu" style="top:{fkPos.top}px; left:{fkPos.left}px">
          <div class="fk-head">
            <input class="input sm" placeholder="Filter relationships…" bind:value={fkFilter} />
          </div>
          <div class="fk-list">
            {#if fkLoading}
              <div class="msg">Reading foreign keys…</div>
            {:else if fkByTable.length === 0}
              <div class="msg">
                {fks && fks.length === 0 ? 'No foreign keys found.' : 'No matches.'}
              </div>
            {:else}
              {#each fkByTable as [table, rows] (table)}
                <div class="fk-table">{table}</div>
                {#each rows as f}
                  <div class="fk-row">
                    <span class="fk-col">{f.column}</span>
                    <span class="fk-arrow">→</span>
                    <span class="fk-ref">{f.ref_table}.{f.ref_column}</span>
                  </div>
                {/each}
              {/each}
            {/if}
          </div>
        </div>
      {/if}
    </span>
    <button
      class="icon-btn"
      title="Refresh"
      on:click={() => (browseTab === 'functions' ? loadFunctions() : load(true))}
      >{@html ICONS.refresh.svg}</button
    >
  </div>

  <div class="browse-tabs">
    <button class:active={browseTab === 'tables'} on:click={() => (browseTab = 'tables')}>
      Tables <span class="count">{schemas.reduce((n, s) => n + s.relations.length, 0)}</span>
    </button>
    <button class:active={browseTab === 'functions'} on:click={() => (browseTab = 'functions')}>
      Functions {#if funcs}<span class="count">{funcs.length}</span>{/if}
    </button>
  </div>

  {#if browseTab === 'functions'}
    {#if funcsLoading && funcs === null}
      <div class="msg">Reading functions…</div>
    {:else if funcsBySchema.length === 0}
      <div class="msg">
        {funcs && funcs.length === 0
          ? conn?.kind === 'sqlite'
            ? 'SQLite has no user-defined function catalog to browse.'
            : 'No functions found.'
          : 'No matches.'}
      </div>
    {:else}
      <div class="scroll">
        {#each funcsBySchema as [schema, rows] (schema)}
          <div class="fn-schema">{schema}</div>
          {#each rows as f (f.schema + '.' + f.name)}
            <button class="fn-row" title="Copy {f.name}({f.arguments})" on:click={() => copyFunctionCall(f)}>
              <span class="fn-kind">{f.kind}</span>
              <span class="fn-name">{f.name}</span>
              <span class="fn-sig">({f.arguments})</span>
              {#if f.return_type}<span class="fn-ret">→ {f.return_type}</span>{/if}
            </button>
          {/each}
        {/each}
      </div>
    {/if}
  {:else if loading && schemas.length === 0}
    <div class="msg">Loading schema…</div>
  {:else if schemas.length === 0}
    <div class="msg">No tables.</div>
  {:else}
    <div class="scroll">
      {#each schemas as schema (schema.name)}
        {#if visibleRels(schema).length || !filter}
          <button class="node schema" on:click={() => toggleSchema(schema.name)}>
            <span class="chev">{@html openSchemas.has(schema.name) ? ICONS.expandOpen.svg : ICONS.expandClosed.svg}</span>
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
                        ? ICONS.expandOpen.svg
                        : ICONS.expandClosed.svg}</span
                    >
                    <span class="ico">{@html rel.kind === 'view' ? ICONS.view.svg : ICONS.table.svg}</span>
                    {rel.name}
                  </button>
                  <button
                    class="icon-btn peek"
                    title="Open the full table, paginated"
                    on:click={() => dispatch('open', { schema: schema.name, relation: rel.name })}
                  >{@html ICONS.insertName.svg}</button>
                  {#if rel.kind !== 'view'}
                    <button
                      class="icon-btn peek"
                      title={ICONS.structure.label}
                      on:click={() => dispatch('structure', { schema: schema.name, relation: rel.name })}
                    >{@html ICONS.structure.svg}</button>
                  {/if}
                </div>
                {#if openRels.has(`${schema.name}.${rel.name}`)}
                  <div class="cols">
                    {#each cols[`${schema.name}.${rel.name}`] || [] as c}
                      <div class="col">
                        <span class="cn">
                          {#if c.primary_key}<span class="pk-ico" title="Primary key">{@html ICONS.primaryKey.svg}</span>{/if}{c.name}
                        </span>
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
  .browse-tabs {
    display: flex;
    border-bottom: 1px solid var(--border);
  }
  .browse-tabs button {
    flex: 1;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    padding: 6px 4px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
  }
  .browse-tabs button:hover {
    color: var(--text-secondary);
  }
  .browse-tabs button.active {
    color: var(--tool-sql-text);
    border-bottom-color: var(--tool-sql-text);
  }
  .browse-tabs .count {
    font-size: 9px;
    font-weight: 400;
    color: var(--text-muted);
  }
  .fn-schema {
    padding: 8px 8px 3px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-secondary);
  }
  .fn-row {
    display: flex;
    align-items: baseline;
    gap: 6px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    cursor: pointer;
    padding: 4px 8px 4px 16px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-primary);
    overflow: hidden;
  }
  .fn-row:hover {
    background: var(--surface-3);
  }
  .fn-kind {
    flex-shrink: 0;
    font-size: 8px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--tool-sql-text);
    background: var(--tool-sql-tint);
    border-radius: 3px;
    padding: 1px 4px;
    font-family: var(--font-sans);
  }
  .fn-name {
    flex-shrink: 0;
    font-weight: 600;
  }
  .fn-sig {
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fn-ret {
    flex-shrink: 0;
    margin-left: auto;
    color: var(--j-key);
    font-size: 10px;
  }
  .fk-wrap {
    position: relative;
  }
  .fk-wrap .backdrop {
    position: fixed;
    inset: 0;
    z-index: 800;
  }
  .fk-menu {
    position: fixed;
    z-index: 801;
    width: 280px;
    max-height: 360px;
    display: flex;
    flex-direction: column;
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
  }
  .fk-head {
    padding: 6px;
    border-bottom: 1px solid var(--border);
  }
  .fk-head .input.sm {
    width: 100%;
    padding: 6px 8px;
    font-size: 11.5px;
  }
  .fk-list {
    overflow: auto;
    padding: 4px 0 8px;
  }
  .fk-table {
    padding: 6px 10px 2px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-secondary);
  }
  .fk-row {
    display: flex;
    align-items: baseline;
    gap: 5px;
    padding: 2px 10px 2px 16px;
    font-size: 11.5px;
    font-family: var(--font-mono);
  }
  .fk-col {
    color: var(--text-primary);
  }
  .fk-arrow {
    color: var(--text-muted);
    font-size: 10px;
  }
  .fk-ref {
    color: var(--tool-sql-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  .pk-ico {
    display: inline-flex;
    vertical-align: -1px;
    margin-right: 3px;
    color: var(--j-number);
  }
  .ct {
    color: var(--text-muted);
  }
</style>
