<script>
  import { createEventDispatcher } from 'svelte';
  export let result;
  const dispatch = createEventDispatcher();

  let sortCol = -1;
  let sortDir = 1;
  let selected = null; // [r,c]

  const ROW_H = 23;
  const OVERSCAN = 12;
  let scrollEl;
  let scrollTop = 0;
  let viewH = 400;

  $: cols = result?.columns ?? [];
  $: baseRows = result?.rows ?? [];
  $: rows =
    sortCol < 0
      ? baseRows
      : [...baseRows].sort((a, b) => cmp(a[sortCol], b[sortCol]) * sortDir);

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
  }

  function cmp(a, b) {
    if (a === null || a === undefined) return -1;
    if (b === null || b === undefined) return 1;
    if (typeof a === 'number' && typeof b === 'number') return a - b;
    return String(a).localeCompare(String(b), undefined, { numeric: true });
  }
  function sortBy(i) {
    if (sortCol === i) sortDir *= -1;
    else {
      sortCol = i;
      sortDir = 1;
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
    const v = rows[selected[0]][selected[1]];
    try {
      await navigator.clipboard.writeText(
        typeof v === 'object' ? JSON.stringify(v, null, 2) : String(v)
      );
    } catch {}
  }
  function key(e) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'c') copyCell();
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
{:else if rows.length === 0}
  <div class="empty">0 rows · {result.duration_ms} ms</div>
{:else}
  {#if result.truncated}
    <div class="trunc">
      Showing first {total.toLocaleString()} rows — capped to protect memory. Add a
      <code>LIMIT</code>, narrow the query, or raise the cap in Settings → Query limits.
    </div>
  {/if}
  <div
    class="grid"
    bind:this={scrollEl}
    on:scroll={onScroll}
    bind:clientHeight={viewH}
    tabindex="0"
  >
    <table>
      <thead>
        <tr>
          <th class="rownum">#</th>
          {#each cols as c, i}
            <th on:click={() => sortBy(i)} title={c.type_name}>
              {c.name}
              <span class="ty">{c.type_name}</span>
              {#if sortCol === i}<span class="arr">{sortDir === 1 ? '▲' : '▼'}</span>{/if}
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
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
