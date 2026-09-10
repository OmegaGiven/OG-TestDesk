<script>
  import { createEventDispatcher } from 'svelte';
  export let result;
  const dispatch = createEventDispatcher();

  let sortCol = -1;
  let sortDir = 1;
  let selected = null; // [r,c]

  $: cols = result?.columns ?? [];
  $: baseRows = result?.rows ?? [];
  $: rows =
    sortCol < 0
      ? baseRows
      : [...baseRows].sort((a, b) => cmp(a[sortCol], b[sortCol]) * sortDir);

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
    const value = rows[r][c];
    dispatch('inspect', { column: cols[c].name, value });
  }
  async function copyCell() {
    if (!selected) return;
    const v = rows[selected[0]][selected[1]];
    try {
      await navigator.clipboard.writeText(typeof v === 'object' ? JSON.stringify(v, null, 2) : String(v));
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
  <div class="grid" tabindex="0">
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
        {#each rows as row, r}
          <tr>
            <td class="rownum">{r + 1}</td>
            {#each row as v, c}
              <td
                class="{cls(v)}"
                class:sel={selected && selected[0] === r && selected[1] === c}
                on:click={() => pick(r, c)}
                title={display(v)}
              >
                {display(v)}
              </td>
            {/each}
          </tr>
        {/each}
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
    padding: 4px 8px;
    max-width: 380px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  tbody tr:hover td {
    background: color-mix(in srgb, var(--tool-sql-text) 7%, transparent);
  }
  tbody tr:hover .rownum {
    background: var(--surface-3);
  }
</style>
