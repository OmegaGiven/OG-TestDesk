<script>
  import { createEventDispatcher } from 'svelte';
  import { downloadText, copyText, rowsToDelimited, rowsToObjects } from '../export.js';
  import { toast } from '../stores.js';
  export let result;
  export let name = 'result';
  const dispatch = createEventDispatcher();

  let sortCol = -1;
  let sortDir = 1;
  let selected = null; // [r,c]

  let search = '';
  let colFilters = {}; // colIndex -> string
  let showColFilters = false;

  if (typeof location !== 'undefined') {
    const q = new URLSearchParams(location.search);
    if (q.has('gridsearch')) search = q.get('gridsearch');
    if (q.has('gridcols')) {
      showColFilters = true;
      q.get('gridcols')
        .split(',')
        .forEach((pair) => {
          const [i, v] = pair.split(':');
          colFilters[+i] = v;
        });
    }
  }

  const ROW_H = 23;
  const OVERSCAN = 12;
  let scrollEl;
  let scrollTop = 0;
  let viewH = 400;

  $: cols = result?.columns ?? [];
  $: baseRows = result?.rows ?? [];

  $: sorted =
    sortCol < 0
      ? baseRows
      : [...baseRows].sort((a, b) => cmp(a[sortCol], b[sortCol]) * sortDir);

  $: activeColFilters = Object.entries(colFilters)
    .filter(([, v]) => v && v.trim())
    .map(([i, v]) => [+i, v.trim()]);
  $: hasFilter = search.trim() || activeColFilters.length > 0;

  $: rows = !hasFilter
    ? sorted
    : sorted.filter((row) => {
        if (search.trim()) {
          const q = search.trim().toLowerCase();
          if (!row.some((v) => display(v).toLowerCase().includes(q))) return false;
        }
        for (const [i, expr] of activeColFilters) {
          if (!matchFilter(row[i], expr)) return false;
        }
        return true;
      });

  $: total = rows.length;
  $: startIdx = Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN);
  $: endIdx = Math.min(total, Math.ceil((scrollTop + viewH) / ROW_H) + OVERSCAN);
  $: visible = rows.slice(startIdx, endIdx);
  $: padTop = startIdx * ROW_H;
  $: padBottom = Math.max(0, (total - endIdx) * ROW_H);

  function onScroll() {
    scrollTop = scrollEl ? scrollEl.scrollTop : 0;
  }
  let lastResult;
  $: if (result !== lastResult) {
    lastResult = result;
    scrollTop = 0;
    if (scrollEl) scrollEl.scrollTop = 0;
    // keep sort + filters across paging; clear only when column set changes
  }

  function cmp(a, b) {
    if (a === null || a === undefined) return -1;
    if (b === null || b === undefined) return 1;
    if (typeof a === 'number' && typeof b === 'number') return a - b;
    return String(a).localeCompare(String(b), undefined, { numeric: true });
  }
  function sortBy(i) {
    if (sortCol === i) {
      if (sortDir === 1) sortDir = -1;
      else {
        sortCol = -1; // 3rd click clears
        sortDir = 1;
      }
    } else {
      sortCol = i;
      sortDir = 1;
    }
  }

  // "> 10", ">=3.5", "!=x", "=abc"  (numeric compare when both sides numeric),
  // otherwise case-insensitive substring. Leading "!" negates a substring.
  const OP_RE = /^\s*(>=|<=|!=|<>|=|>|<)\s*(.*)$/s;
  function matchFilter(value, expr) {
    const m = expr.match(OP_RE);
    if (m) {
      const [, op, rhsRaw] = m;
      const rhs = rhsRaw.trim();
      const ln = typeof value === 'number' ? value : parseFloat(value);
      const rn = parseFloat(rhs);
      const numeric = !Number.isNaN(ln) && !Number.isNaN(rn) && /^-?\d/.test(rhs);
      const ls = display(value).toLowerCase();
      const rs = rhs.toLowerCase();
      switch (op) {
        case '>':
          return numeric ? ln > rn : ls > rs;
        case '<':
          return numeric ? ln < rn : ls < rs;
        case '>=':
          return numeric ? ln >= rn : ls >= rs;
        case '<=':
          return numeric ? ln <= rn : ls <= rs;
        case '=':
          return numeric ? ln === rn : ls === rs;
        case '!=':
        case '<>':
          return numeric ? ln !== rn : !ls.includes(rs);
      }
    }
    if (expr.startsWith('!')) return !display(value).toLowerCase().includes(expr.slice(1).toLowerCase());
    return display(value).toLowerCase().includes(expr.toLowerCase());
  }

  function clearFilters() {
    search = '';
    colFilters = {};
  }

  $: fileBase = (name || 'result').replace(/[^\w.-]+/g, '_').slice(0, 60) || 'result';
  function doExport(fmt) {
    if (fmt === 'csv') {
      downloadText(`${fileBase}.csv`, rowsToDelimited(cols, rows, ','), 'text/csv');
    } else if (fmt === 'tsv') {
      downloadText(`${fileBase}.tsv`, rowsToDelimited(cols, rows, '\t'), 'text/tab-separated-values');
    } else if (fmt === 'json') {
      downloadText(
        `${fileBase}.json`,
        JSON.stringify(rowsToObjects(cols, rows), null, 2),
        'application/json'
      );
    } else if (fmt === 'copy') {
      copyText(rowsToDelimited(cols, rows, '\t')).then((ok) =>
        toast(ok ? `Copied ${rows.length.toLocaleString()} rows` : 'Copy blocked', ok ? 'success' : 'error', 1800)
      );
    }
  }

  function display(v) {
    if (v === null || v === undefined) return 'NULL';
    if (typeof v === 'object') return JSON.stringify(v);
    return String(v);
  }
  function cls(v) {
    if (v === null || v === undefined) return 'null';
    if (typeof v === 'object') return 'json';
    if (typeof v === 'number') return 'num';
    if (typeof v === 'boolean') return 'bool';
    return 'txt';
  }

  function pick(r, c) {
    selected = [r, c];
    dispatch('inspect', { column: cols[c].name, value: rows[r][c] });
  }
  async function copyCell() {
    if (!selected) return;
    const v = rows[selected[0]]?.[selected[1]];
    if (v === undefined) return;
    try {
      await navigator.clipboard.writeText(
        typeof v === 'object' ? JSON.stringify(v, null, 2) : String(v)
      );
    } catch {}
  }
  function key(e) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'c') copyCell();
    if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
      e.preventDefault();
      document.querySelector('.grid-search')?.focus();
    }
  }
</script>

<svelte:window on:keydown={key} />

{#if !result}
  <div class="empty">Run a query to see results.</div>
{:else if !result.is_select}
  <div class="empty">
    <div style="font-size:22px">✓</div>
    {result.rows_affected} row{result.rows_affected === 1 ? '' : 's'} affected · {result.duration_ms} ms
  </div>
{:else if baseRows.length === 0}
  <div class="empty">0 rows · {result.duration_ms} ms</div>
{:else}
  <div class="filter-bar">
    <input
      class="input sm grid-search"
      placeholder="Search this page…"
      bind:value={search}
    />
    <button
      class="btn ghost sm"
      class:on={showColFilters}
      title="Per-column filters"
      on:click={() => (showColFilters = !showColFilters)}>⑂ Columns</button
    >
    {#if hasFilter}
      <span class="fcount">{total.toLocaleString()} of {baseRows.length.toLocaleString()}</span>
      <button class="btn ghost sm" on:click={clearFilters}>Clear</button>
    {/if}
    <span style="flex:1" />
    <span class="export">
      <span class="ex-label">Export{hasFilter ? ' (filtered)' : result.has_more || result.page > 0 ? ' (page)' : ''}</span>
      <button class="btn ghost sm" on:click={() => doExport('csv')}>CSV</button>
      <button class="btn ghost sm" on:click={() => doExport('tsv')}>TSV</button>
      <button class="btn ghost sm" on:click={() => doExport('json')}>JSON</button>
      <button class="btn ghost sm" title="Copy as TSV (paste into a spreadsheet)" on:click={() => doExport('copy')}>⧉</button>
    </span>
  </div>

  {#if result.truncated}
    <div class="trunc">
      Showing first {baseRows.length.toLocaleString()} rows — capped to protect memory. Use "Load all",
      add a <code>LIMIT</code>, or raise the cap in Settings.
    </div>
  {/if}

  <div class="grid" bind:this={scrollEl} on:scroll={onScroll} bind:clientHeight={viewH} tabindex="0">
    <table>
      <thead>
        <tr>
          <th class="rownum">#</th>
          {#each cols as c, i}
            <th on:click={() => sortBy(i)} title="{c.type_name} — click to sort">
              {c.name}
              <span class="ty">{c.type_name}</span>
              {#if sortCol === i}<span class="arr">{sortDir === 1 ? '▲' : '▼'}</span>{/if}
            </th>
          {/each}
        </tr>
        {#if showColFilters}
          <tr class="filter-row">
            <th class="rownum"></th>
            {#each cols as c, i}
              <th>
                <input
                  class="col-filter"
                  placeholder="filter"
                  bind:value={colFilters[i]}
                  on:click|stopPropagation
                />
              </th>
            {/each}
          </tr>
        {/if}
      </thead>
      <tbody>
        {#if total === 0}
          <tr><td colspan={cols.length + 1} class="no-match">No rows match the filter.</td></tr>
        {/if}
        {#if padTop}
          <tr class="spacer"><td colspan={cols.length + 1} style="height:{padTop}px"></td></tr>
        {/if}
        {#each visible as row, vi (startIdx + vi)}
          <tr>
            <td class="rownum">{startIdx + vi + 1}</td>
            {#each row as v, c}
              <td
                class={cls(v)}
                class:sel={selected && selected[0] === startIdx + vi && selected[1] === c}
                on:click={() => pick(startIdx + vi, c)}
                title={display(v)}
              >
                {display(v)}
              </td>
            {/each}
          </tr>
        {/each}
        {#if padBottom}
          <tr class="spacer"><td colspan={cols.length + 1} style="height:{padBottom}px"></td></tr>
        {/if}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 100%;
    color: var(--text-muted);
    font-size: 12px;
  }
  .filter-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-1);
  }
  .filter-bar .input.sm {
    width: 200px;
    padding: 4px 8px;
    font-size: 11px;
  }
  .btn.ghost.sm.on {
    background: var(--tool-sql-tint);
    color: var(--tool-sql-text);
  }
  .fcount {
    font-size: 10.5px;
    font-family: var(--font-mono);
    color: var(--text-secondary);
  }
  .export {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .ex-label {
    font-size: 10px;
    color: var(--text-muted);
    margin-right: 3px;
  }
  .trunc {
    padding: 6px 10px;
    font-size: 11px;
    background: color-mix(in srgb, var(--warn) 14%, transparent);
    color: var(--text-primary);
    border-bottom: 1px solid var(--border);
  }
  .grid {
    height: 100%;
    overflow: auto;
    outline: none;
  }
  table {
    border-collapse: separate;
    border-spacing: 0;
    font-family: var(--font-mono);
    font-size: 11.5px;
    width: max-content;
    min-width: 100%;
  }
  th,
  td {
    border-bottom: 1px solid var(--border);
    border-right: 1px solid var(--border);
    padding: 3px 8px;
    height: 23px;
    max-width: 380px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  tr.spacer td {
    border: none;
    padding: 0;
  }
  .no-match {
    text-align: center;
    color: var(--text-muted);
    font-family: var(--font-sans);
    padding: 20px;
  }
  thead th {
    position: sticky;
    top: 0;
    background: var(--surface-1);
    text-align: left;
    cursor: pointer;
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 600;
    z-index: 2;
  }
  thead tr.filter-row th {
    top: 34px;
    cursor: default;
    padding: 2px 4px;
    z-index: 2;
  }
  .col-filter {
    width: 100%;
    min-width: 60px;
    font: inherit;
    font-family: var(--font-mono);
    font-size: 10.5px;
    padding: 2px 5px;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    background: var(--surface-2);
    color: var(--text-primary);
  }
  .ty {
    display: block;
    font-weight: 400;
    font-size: 9px;
    color: var(--text-muted);
    text-transform: lowercase;
  }
  .arr {
    font-size: 8px;
  }
  .rownum {
    position: sticky;
    left: 0;
    background: var(--surface-1);
    color: var(--text-muted);
    text-align: right;
    z-index: 1;
    user-select: none;
  }
  thead .rownum {
    z-index: 3;
  }
  td {
    cursor: default;
  }
  td.sel {
    outline: 2px solid var(--tool-sql-text);
    outline-offset: -2px;
  }
  td.null {
    color: var(--text-muted);
    font-style: italic;
  }
  td.num {
    color: var(--j-number);
    text-align: right;
  }
  td.bool {
    color: var(--j-bool);
  }
  td.json {
    color: var(--j-key);
  }
  tbody tr:not(.spacer):hover td {
    background: color-mix(in srgb, var(--tool-sql-text) 7%, transparent);
  }
  tbody tr:not(.spacer):hover .rownum {
    background: var(--surface-3);
  }
</style>
