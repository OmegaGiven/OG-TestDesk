<script>
  import { createEventDispatcher } from 'svelte';
  import { downloadText, copyText, rowsToDelimited, rowsToObjects } from '../export.js';
  import { toast } from '../stores.js';
  import { ICONS } from '../icons.js';
  export let result;
  export let name = 'result';
  export let loadingMore = false; // parent is fetching the next page
  export let appending = false; // true while `result` is being replaced by an append (keep scroll/edits)
  const dispatch = createEventDispatcher();

  let sortCol = -1;
  let sortDir = 1;
  let selected = null; // [r,c]

  let search = '';
  let colFilters = {}; // colIndex -> string
  let showColFilters = false;
  let headerRowH = 24; // measured, drives the filter-row's sticky offset

  let hiddenCols = new Set(); // column indices hidden from view
  let colsMenuOpen = false;
  let lastColKey = '';
  $: visibleIdx = cols.map((_, i) => i).filter((i) => !hiddenCols.has(i));
  function toggleCol(i) {
    const s = new Set(hiddenCols);
    s.has(i) ? s.delete(i) : s.add(i);
    hiddenCols = s;
  }
  function showAllCols() {
    hiddenCols = new Set();
  }
  function hideAllCols() {
    // keep at least one column visible
    hiddenCols = new Set(cols.slice(1).map((_, i) => i + 1));
  }

  // ---- inline edits (view/export only — never written back to the DB)
  let editing = false;
  let edits = new Map(); // row (array ref, stable across sort/filter/append) -> {colIndex: value}
  $: editCount = [...edits.values()].reduce((n, e) => n + Object.keys(e).length, 0);

  function coerceEdit(original, str) {
    if (str.trim() === '') return null;
    if (typeof original === 'number') {
      const n = Number(str);
      return Number.isNaN(n) ? str : n;
    }
    if (typeof original === 'boolean') return str.trim().toLowerCase() === 'true';
    return str;
  }
  function cellVal(row, c) {
    const e = edits.get(row);
    return e && Object.prototype.hasOwnProperty.call(e, c) ? e[c] : row[c];
  }
  function hasEdit(row, c) {
    const e = edits.get(row);
    return !!e && Object.prototype.hasOwnProperty.call(e, c);
  }
  function setEdit(row, c, str) {
    const original = row[c];
    const val = coerceEdit(original, str);
    const next = new Map(edits);
    const rowEdits = { ...(next.get(row) || {}) };
    if (val === original) delete rowEdits[c];
    else rowEdits[c] = val;
    if (Object.keys(rowEdits).length) next.set(row, rowEdits);
    else next.delete(row);
    edits = next;
  }
  function discardEdits() {
    edits = new Map();
  }
  $: exportRows =
    edits.size === 0
      ? rows
      : rows.map((row) => {
          const e = edits.get(row);
          if (!e) return row;
          const copy = row.slice();
          for (const c in e) copy[+c] = e[c];
          return copy;
        });

  if (typeof location !== 'undefined') {
    const q = new URLSearchParams(location.search);
    if (q.has('gridsearch')) search = q.get('gridsearch');
    if (q.has('gridcolsmenu')) colsMenuOpen = true;
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

  function maybeLoadMore() {
    if (!scrollEl || loadingMore || !result?.has_more) return;
    const remaining = scrollEl.scrollHeight - scrollEl.scrollTop - scrollEl.clientHeight;
    if (remaining < 400) dispatch('loadmore');
  }
  function onScroll() {
    scrollTop = scrollEl ? scrollEl.scrollTop : 0;
    maybeLoadMore();
  }
  $: if (scrollEl && total >= 0 && viewH && !loadingMore) maybeLoadMore();

  let lastResult;
  $: if (result !== lastResult) {
    lastResult = result;
    if (!appending) {
      scrollTop = 0;
      if (scrollEl) scrollEl.scrollTop = 0;
      edits = new Map();
    }
    // keep sort + filters across paging; clear only when column set changes
    const colKey = cols.map((c) => c.name).join('');
    if (colKey !== lastColKey) {
      lastColKey = colKey;
      hiddenCols = new Set();
    }
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
      downloadText(`${fileBase}.csv`, rowsToDelimited(cols, exportRows, ','), 'text/csv');
    } else if (fmt === 'tsv') {
      downloadText(`${fileBase}.tsv`, rowsToDelimited(cols, exportRows, '\t'), 'text/tab-separated-values');
    } else if (fmt === 'json') {
      downloadText(
        `${fileBase}.json`,
        JSON.stringify(rowsToObjects(cols, exportRows), null, 2),
        'application/json'
      );
    } else if (fmt === 'copy') {
      copyText(rowsToDelimited(cols, exportRows, '\t')).then((ok) =>
        toast(ok ? `Copied ${exportRows.length.toLocaleString()} rows` : 'Copy blocked', ok ? 'success' : 'error', 1800)
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
  }
  async function copyCell() {
    if (!selected) return;
    const row = rows[selected[0]];
    const v = row ? cellVal(row, selected[1]) : undefined;
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
    <div style="font-size:22px">{ICONS.success.glyph}</div>
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
      on:click={() => (showColFilters = !showColFilters)}>{ICONS.columnFilters.glyph} Filters</button
    >
    <span class="cols-menu-wrap">
      <button
        class="btn ghost sm"
        class:on={colsMenuOpen}
        title="Show / hide columns"
        on:click={() => (colsMenuOpen = !colsMenuOpen)}
        >{ICONS.columns.glyph} Columns{hiddenCols.size ? ` (${visibleIdx.length}/${cols.length})` : ''}</button
      >
      {#if colsMenuOpen}
        <div class="backdrop" on:click={() => (colsMenuOpen = false)} role="presentation" />
        <div class="cols-menu">
          <div class="cols-menu-head">
            <button class="btn ghost sm" on:click={showAllCols}>All</button>
            <button class="btn ghost sm" on:click={hideAllCols}>None</button>
          </div>
          <div class="cols-menu-list">
            {#each cols as c, i (i)}
              <label class="cols-menu-item">
                <input type="checkbox" checked={!hiddenCols.has(i)} on:change={() => toggleCol(i)} />
                <span class="cn">{c.name}</span>
                <span class="ty">{c.type_name}</span>
              </label>
            {/each}
          </div>
        </div>
      {/if}
    </span>
    {#if hasFilter}
      <span class="fcount">{total.toLocaleString()} of {baseRows.length.toLocaleString()}</span>
      <button class="btn ghost sm" on:click={clearFilters}>Clear</button>
    {/if}
    <button class="btn ghost sm" class:on={editing} title="Edit cells (view/export only, does not write to the DB)" on:click={() => (editing = !editing)}>
      {ICONS.editCells.glyph} Edit
    </button>
    {#if editCount}
      <span class="fcount">{editCount.toLocaleString()} edited</span>
      <button class="btn ghost sm" on:click={discardEdits}>{ICONS.revert.glyph} Discard</button>
    {/if}
    <span style="flex:1" />
    <span class="export">
      <span class="ex-label"
        >Export{editCount
          ? ' (edited)'
          : hasFilter
            ? ' (filtered)'
            : result.has_more || result.page > 0
              ? ' (loaded)'
              : ''}</span
      >
      <button class="btn ghost sm" on:click={() => doExport('csv')}>CSV</button>
      <button class="btn ghost sm" on:click={() => doExport('tsv')}>TSV</button>
      <button class="btn ghost sm" on:click={() => doExport('json')}>JSON</button>
      <button class="btn ghost sm" title="Copy as TSV (paste into a spreadsheet)" on:click={() => doExport('copy')}>{ICONS.copy.glyph}</button>
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
        <tr bind:clientHeight={headerRowH}>
          <th class="rownum">#</th>
          {#each visibleIdx as i (i)}
            <th on:click={() => sortBy(i)} title="{cols[i].type_name} — click to sort">
              <span class="th-row">
                <span class="cn">{cols[i].name}</span>
                <span class="ty">{cols[i].type_name}</span>
              </span>
              {#if sortCol === i}<span class="arr">{sortDir === 1 ? ICONS.sortAsc.glyph : ICONS.sortDesc.glyph}</span>{/if}
            </th>
          {/each}
        </tr>
        {#if showColFilters}
          <tr class="filter-row" style="--filter-top: {headerRowH}px">
            <th class="rownum"></th>
            {#each visibleIdx as i (i)}
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
          <tr><td colspan={visibleIdx.length + 1} class="no-match">No rows match the filter.</td></tr>
        {/if}
        {#if padTop}
          <tr class="spacer"><td colspan={visibleIdx.length + 1} style="height:{padTop}px"></td></tr>
        {/if}
        {#each visible as row, vi (startIdx + vi)}
          <tr>
            <td class="rownum">{startIdx + vi + 1}</td>
            {#each visibleIdx as c (c)}
              {#if editing}
                <td class="editing-cell" class:edited={hasEdit(row, c)}>
                  <input
                    class="cell-edit"
                    value={cellVal(row, c) === null || cellVal(row, c) === undefined ? '' : display(cellVal(row, c))}
                    on:click|stopPropagation
                    on:change={(e) => setEdit(row, c, e.target.value)}
                    on:keydown={(e) => {
                      if (e.key === 'Enter') e.target.blur();
                    }}
                  />
                </td>
              {:else}
                <td
                  class={cls(cellVal(row, c))}
                  class:sel={selected && selected[0] === startIdx + vi && selected[1] === c}
                  class:edited={hasEdit(row, c)}
                  on:click={() => pick(startIdx + vi, c)}
                  title={display(cellVal(row, c))}
                >
                  {display(cellVal(row, c))}
                </td>
              {/if}
            {/each}
          </tr>
        {/each}
        {#if padBottom}
          <tr class="spacer"><td colspan={visibleIdx.length + 1} style="height:{padBottom}px"></td></tr>
        {/if}
        {#if loadingMore}
          <tr class="loading-more"><td colspan={visibleIdx.length + 1}>Loading more…</td></tr>
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
  .cols-menu-wrap {
    position: relative;
  }
  .cols-menu-wrap .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }
  .cols-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 41;
    width: 220px;
    max-height: 320px;
    display: flex;
    flex-direction: column;
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
  }
  .cols-menu-head {
    display: flex;
    gap: 4px;
    padding: 6px;
    border-bottom: 1px solid var(--border);
  }
  .cols-menu-list {
    overflow: auto;
    padding: 4px 0;
  }
  .cols-menu-item {
    display: flex;
    align-items: baseline;
    gap: 6px;
    padding: 5px 10px;
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
  }
  .cols-menu-item:hover {
    background: var(--surface-3);
  }
  .cols-menu-item .cn {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cols-menu-item .ty {
    font-size: 9px;
    color: var(--text-muted);
    text-transform: lowercase;
    flex-shrink: 0;
  }
  .fcount {
    font-size: 10.5px;
    font-family: var(--font-mono);
    color: var(--text-secondary);
  }
  .filter-bar :global(.btn.ghost.sm) {
    font-size: 12px;
    padding: 5px 9px;
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
    height: auto;
    white-space: normal;
    vertical-align: middle;
    z-index: 2;
  }
  .th-row {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 2px 8px;
  }
  .cn {
    flex-shrink: 0;
  }
  thead tr.filter-row th {
    top: var(--filter-top, 24px);
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
    margin-left: auto;
    font-weight: 400;
    font-size: 9px;
    color: var(--text-muted);
    text-transform: lowercase;
    white-space: nowrap;
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
  td.edited,
  .editing-cell.edited {
    background: color-mix(in srgb, var(--warn) 20%, transparent);
  }
  .editing-cell {
    padding: 1px 2px;
  }
  .cell-edit {
    width: 100%;
    font: inherit;
    font-family: var(--font-mono);
    padding: 2px 5px;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    background: var(--surface-2);
    color: var(--text-primary);
  }
  .loading-more td {
    text-align: center;
    color: var(--text-muted);
    font-family: var(--font-sans);
    font-size: 11px;
    padding: 8px;
    border-right: none;
  }
  tbody tr:not(.spacer):hover td {
    background: color-mix(in srgb, var(--tool-sql-text) 7%, transparent);
  }
  tbody tr:not(.spacer):hover .rownum {
    background: var(--surface-3);
  }
</style>
