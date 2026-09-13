<script>
  import { onDestroy } from 'svelte';
  import { get } from 'svelte/store';
  import { Chart, registerables } from 'chart.js';
  import { api } from '../api.js';
  import { connections, toast, toastError, reloadSavedCharts, promptDialog } from '../stores.js';

  Chart.register(...registerables);

  export let rows = []; // array of objects
  export let cols = []; // field names
  export let meta = null; // { connectionId, sql } — lets the saved chart be re-run later
  export let existing = null; // SavedChart when reopened from Activity → Charts, else null

  const PALETTE = [
    '#3b82c4', '#1d9e75', '#d99114', '#8b5cf6', '#d4537e',
    '#84a417', '#e24b4a', '#5ea3e0', '#ef9f27', '#a888f5'
  ];

  function safeArr(json) {
    try {
      const v = JSON.parse(json || '[]');
      return Array.isArray(v) ? v : [];
    } catch {
      return [];
    }
  }

  let chartType = existing?.chart_type || 'bar';
  let xField = existing?.x_field || cols[0] || '';
  let yFields = existing ? safeArr(existing.y_fields_json) : cols[1] ? [cols[1]] : [];
  let name = existing?.name || '';
  let canvasEl;
  let chartInstance;
  let saving = false;

  $: numericCols = cols.filter((c) => rows.some((r) => typeof r[c] === 'number'));
  $: canChart = xField && yFields.length > 0 && rows.length > 0;
  $: canRerun = !!(meta?.connectionId && meta?.sql) || !!(existing?.connection_id && existing?.sql_text);

  function toggleY(field) {
    if (chartType === 'pie') {
      yFields = [field];
      return;
    }
    yFields = yFields.includes(field) ? yFields.filter((f) => f !== field) : [...yFields, field];
  }

  function render() {
    if (chartInstance) {
      chartInstance.destroy();
      chartInstance = null;
    }
    if (!canvasEl || !canChart) return;

    const labels = rows.map((r) => String(r[xField] ?? ''));
    let data;
    if (chartType === 'pie') {
      const f = yFields[0];
      data = {
        labels,
        datasets: [
          {
            data: rows.map((r) => Number(r[f]) || 0),
            backgroundColor: labels.map((_, i) => PALETTE[i % PALETTE.length])
          }
        ]
      };
    } else {
      data = {
        labels,
        datasets: yFields.map((f, i) => ({
          label: f,
          data: rows.map((r) => Number(r[f]) || 0),
          backgroundColor: chartType === 'line' ? 'transparent' : PALETTE[i % PALETTE.length],
          borderColor: PALETTE[i % PALETTE.length],
          tension: 0.25
        }))
      };
    }

    chartInstance = new Chart(canvasEl, {
      type: chartType,
      data,
      options: {
        responsive: true,
        maintainAspectRatio: false,
        animation: false,
        plugins: { legend: { display: chartType === 'pie' || yFields.length > 1 } }
      }
    });
  }

  $: if (canvasEl) {
    // re-render whenever the shape of the chart changes
    chartType, xField, yFields, rows;
    render();
  }
  onDestroy(() => chartInstance?.destroy());

  async function save() {
    let n = name.trim();
    if (!n) {
      n = await promptDialog('Chart name:', 'New chart');
      if (!n) return;
      n = n.trim();
      if (!n) return;
    }
    name = n;
    saving = true;
    try {
      const saved = await api.savedChartSave({
        id: existing?.id || '',
        name,
        connection_id: meta?.connectionId || existing?.connection_id || null,
        saved_query_id: existing?.saved_query_id || null,
        sql_text: meta?.sql || existing?.sql_text || null,
        chart_type: chartType,
        x_field: xField,
        y_fields_json: JSON.stringify(yFields),
        options_json: '{}',
        data_json: JSON.stringify(rows),
        row_count: rows.length,
        last_run_at: Math.floor(Date.now() / 1000),
        sort_order: existing?.sort_order || 0,
        created_at: existing?.created_at || 0
      });
      existing = saved;
      await reloadSavedCharts();
      toast('Chart saved', 'success', 1800);
    } catch (e) {
      toastError(e);
    } finally {
      saving = false;
    }
  }

  async function rerun() {
    const connId = meta?.connectionId || existing?.connection_id;
    const sql = meta?.sql || existing?.sql_text;
    if (!connId || !sql) return;
    saving = true;
    try {
      const conn = get(connections).find((c) => c.id === connId);
      if (!conn) throw new Error('That connection no longer exists');
      const result = await api.queryRun(conn, sql, null, null, false);
      rows = result.rows.map((row) => Object.fromEntries(result.columns.map((c, i) => [c.name, row[i]])));
      cols = result.columns.map((c) => c.name);
      toast(`Refreshed — ${rows.length.toLocaleString()} rows`, 'success', 1800);
      if (existing) await save();
    } catch (e) {
      toastError(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="chart-view">
  <div class="chart-tools">
    <div class="field">
      <label for="ct">Type</label>
      <select id="ct" class="select sm" bind:value={chartType}>
        <option value="bar">Bar</option>
        <option value="line">Line</option>
        <option value="pie">Pie</option>
      </select>
    </div>
    <div class="field">
      <label for="xf">{chartType === 'pie' ? 'Labels' : 'X axis'}</label>
      <select id="xf" class="select sm" bind:value={xField}>
        {#each cols as c}<option value={c}>{c}</option>{/each}
      </select>
    </div>
    <div class="field grow">
      <label>{chartType === 'pie' ? 'Value' : 'Y values'}</label>
      <div class="y-picks">
        {#each numericCols as c (c)}
          <button
            class="y-pick"
            class:on={yFields.includes(c)}
            type="button"
            on:click={() => toggleY(c)}>{c}</button
          >
        {/each}
        {#if numericCols.length === 0}<span class="muted">No numeric columns found.</span>{/if}
      </div>
    </div>
    <input class="input sm chart-name" placeholder="Chart name" bind:value={name} />
    <button class="btn sm" on:click={save} disabled={saving || !canChart}>
      {existing ? 'Update' : 'Save'} chart
    </button>
    {#if canRerun}
      <button class="btn ghost sm" on:click={rerun} disabled={saving} title="Re-run the source query">
        ⟳ Refresh
      </button>
    {/if}
  </div>

  {#if !canChart}
    <div class="empty">
      {rows.length === 0
        ? 'No rows to plot.'
        : 'Pick an X field and at least one numeric Y field.'}
    </div>
  {:else}
    <div class="chart-canvas-wrap">
      <canvas bind:this={canvasEl}></canvas>
    </div>
  {/if}
</div>

<style>
  .chart-view {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .chart-tools {
    display: flex;
    align-items: flex-end;
    flex-wrap: wrap;
    gap: 10px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .field.grow {
    flex: 1;
    min-width: 200px;
  }
  .field label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-muted);
  }
  .select.sm {
    min-width: 110px;
  }
  .y-picks {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    min-height: 22px;
    align-items: center;
  }
  .y-pick {
    font-size: 11px;
    padding: 2px 8px;
    height: 22px;
    border-radius: 10px;
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    color: var(--text-secondary);
    cursor: pointer;
  }
  .y-pick.on {
    background: var(--tool-inspector-tint);
    color: var(--tool-inspector-text);
    border-color: var(--tool-inspector-text);
    font-weight: 600;
  }
  .chart-name {
    width: 160px;
  }
  .chart-canvas-wrap {
    flex: 1;
    min-height: 0;
    min-width: 0;
    padding: 16px;
    position: relative;
  }
  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    font-size: 12px;
  }
  .muted {
    font-size: 11px;
    color: var(--text-muted);
  }
</style>
