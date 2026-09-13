<script>
  import JsonNode from './JsonNode.svelte';
  import ChartView from './ChartView.svelte';
  import { inspectorPayload, toast } from '../stores.js';
  import { ICONS } from '../icons.js';
  import { downloadText, copyText, rowsToDelimited } from '../export.js';

  let mode = 'tree'; // tree | table | summary | raw | chart
  let consumedChartOpen = null;
  let filter = '';
  let expandSet = new Set();
  let bump = 0; // force reactivity when expandSet mutates
  let selected = null; // {path, value, type}
  let rawMode = false;
  let rawText = '';

  // ---- "Selected node" detail panel: draggable/resizable width, same
  // pattern as the SQL view's sidebar resizer
  const DETAIL_W_KEY = 'ogtestdesk.inspector.detailW';
  function loadDetailW() {
    try {
      const n = Number(localStorage.getItem(DETAIL_W_KEY));
      return n >= 220 && n <= 640 ? n : 320;
    } catch {
      return 320;
    }
  }
  let detailW = loadDetailW();
  let draggingDetail = false;
  let bodyEl;
  function startDetailDrag() {
    draggingDetail = true;
  }
  function onDetailMove(e) {
    if (!draggingDetail || !bodyEl) return;
    const rect = bodyEl.getBoundingClientRect();
    detailW = Math.min(640, Math.max(220, rect.right - e.clientX));
  }
  function endDetailDrag() {
    if (!draggingDetail) return;
    draggingDetail = false;
    try {
      localStorage.setItem(DETAIL_W_KEY, String(Math.round(detailW)));
    } catch {}
  }

  if (typeof location !== 'undefined') {
    const q = new URLSearchParams(location.search);
    const raw = q.get('inspectraw');
    if (raw) {
      rawMode = true;
      rawText = raw;
    }
    if (q.has('inspectmode')) mode = q.get('inspectmode');
  }
  let rawError = '';

  $: payload = $inspectorPayload;
  $: root = rawMode ? parseRaw(rawText) : payload?.json;
  $: if (payload?.source === 'chart-reopen' && payload.at !== consumedChartOpen) {
    consumedChartOpen = payload.at;
    mode = 'chart';
  }
  $: label = rawMode ? 'Pasted JSON' : payload?.label || 'Nothing loaded';

  // Auto-expand the first two levels whenever a new payload loads.
  let autoExpandedFor = null;
  $: {
    const stamp = rawMode ? rawText : payload?.at;
    if (root && typeof root === 'object' && stamp !== autoExpandedFor) {
      autoExpandedFor = stamp;
      const s = new Set();
      walk(root, '$', s, 2);
      expandSet = s;
      bump++;
    }
  }

  function parseRaw(t) {
    rawError = '';
    if (!t.trim()) return null;
    try {
      return JSON.parse(t);
    } catch (e) {
      rawError = String(e);
      return null;
    }
  }
  function prettify() {
    try {
      rawText = JSON.stringify(JSON.parse(rawText), null, 2);
    } catch (e) {
      toast('Not valid JSON: ' + e.message, 'error');
    }
  }
  function minify() {
    try {
      rawText = JSON.stringify(JSON.parse(rawText));
    } catch (e) {
      toast('Not valid JSON: ' + e.message, 'error');
    }
  }

  // ---- edit mode for loaded data (Raw view) — lets you tweak a result
  // or response in place; the edit updates every mode (Tree/Table/
  // Summary/Chart all read the same payload), not just this view.
  let editingRaw = false;
  let rawEditText = '';
  let editError = '';
  function startRawEdit() {
    rawEditText = rawPretty;
    editError = '';
    editingRaw = true;
  }
  function cancelRawEdit() {
    editingRaw = false;
    editError = '';
  }
  function applyRawEdit() {
    try {
      const parsed = JSON.parse(rawEditText);
      inspectorPayload.update((p) => (p ? { ...p, json: parsed } : p));
      editingRaw = false;
      editError = '';
      toast('Edit applied', 'success', 1500);
    } catch (e) {
      editError = 'Invalid JSON: ' + e.message;
    }
  }
  // leaving Raw mode, or a new payload arriving, discards an in-progress edit
  $: if (mode !== 'raw' && editingRaw) cancelRawEdit();
  $: if (payload?.at && editingRaw) cancelRawEdit();

  function expandChange() {
    bump++;
    expandSet = expandSet;
  }
  function collapseAll() {
    expandSet = new Set();
    bump++;
  }
  function expandAll() {
    const s = new Set();
    walk(root, '$', s, Infinity);
    expandSet = s;
    bump++;
  }
  function expandOne() {
    const s = new Set(expandSet);
    walk(root, '$', s, 1, true);
    expandSet = s;
    bump++;
  }
  function walk(v, path, set, maxDepth, oneLevel = false, depth = 0) {
    if (v === null || typeof v !== 'object') return;
    if (depth > maxDepth) return;
    if (!oneLevel || !set.has(path) || depth === 0) set.add(path);
    const ents = Array.isArray(v) ? v.map((x, i) => [i, x]) : Object.entries(v);
    for (const [k, child] of ents) {
      const p = Array.isArray(v) ? `${path}[${k}]` : `${path}.${k}`;
      walk(child, p, set, maxDepth, oneLevel, depth + 1);
    }
  }

  // search match count
  $: matchCount = filter ? countMatches(root, '$') : 0;
  function countMatches(v, key) {
    let n = 0;
    const f = filter.toLowerCase();
    if (key !== '$' && key.split(/[.[\]]/).pop().toLowerCase().includes(f)) n++;
    if (v !== null && typeof v === 'object') {
      const ents = Array.isArray(v) ? v.map((x, i) => [i, x]) : Object.entries(v);
      for (const [k, c] of ents) n += countMatches(c, String(k));
    } else if (String(v).toLowerCase().includes(f)) n++;
    return n;
  }
  $: if (filter) {
    // auto-expand to reveal matches
    const s = new Set();
    walk(root, '$', s, Infinity);
    expandSet = s;
  }

  function onSelect(e) {
    selected = e.detail;
    editingNode = false;
  }

  // ---- resolve the selected node's value FRESH from `root` by path,
  // every time, instead of trusting the {value} snapshot JsonNode
  // captured at click time. That snapshot goes stale the moment `root`
  // changes under it (an edit applied elsewhere, a new page of results,
  // a re-run) — the detail panel would keep showing the old value for
  // whatever was selected even though the tree itself re-rendered
  // correctly, which is exactly the "selected value doesn't populate
  // right" symptom. Resolving by path self-heals that.
  function parsePath(path) {
    const segs = [];
    const re = /\.([^.[\]]+)|\[(\d+)\]/g;
    let m;
    while ((m = re.exec(path))) segs.push(m[1] !== undefined ? m[1] : Number(m[2]));
    return segs;
  }
  function getAtPath(obj, path) {
    let cur = obj;
    for (const seg of parsePath(path)) {
      if (cur == null) return undefined;
      cur = cur[seg];
    }
    return cur;
  }
  /** Returns a new root with the value at `path` replaced — clones only
   * along the path, the rest of the structure is shared. */
  function setAtPath(obj, path, next) {
    const segs = parsePath(path);
    if (segs.length === 0) return next;
    const root2 = Array.isArray(obj) ? [...obj] : { ...obj };
    let cur = root2;
    for (let i = 0; i < segs.length - 1; i++) {
      const seg = segs[i];
      const child = cur[seg];
      const clone = Array.isArray(child) ? [...child] : { ...(child ?? {}) };
      cur[seg] = clone;
      cur = clone;
    }
    cur[segs[segs.length - 1]] = next;
    return root2;
  }

  $: selectedValue = selected ? getAtPath(root, selected.path) : undefined;
  $: selectedExists = selected ? selectedValue !== undefined || selected.path === '$' : false;
  $: selectedType = selected
    ? selectedValue === null
      ? 'null'
      : Array.isArray(selectedValue)
        ? 'array'
        : typeof selectedValue
    : null;

  function subtreePretty() {
    if (!selected) return '';
    try {
      return JSON.stringify(selectedValue, null, 2);
    } catch {
      return String(selectedValue);
    }
  }
  function sizeOf(v) {
    if (v === null || typeof v !== 'object') return String(v ?? '').length + ' chars';
    return Array.isArray(v) ? `${v.length} items` : `${Object.keys(v).length} keys`;
  }

  // ---- edit the selected node's value in place
  let editingNode = false;
  let nodeEditText = '';
  let nodeEditError = '';
  function startNodeEdit() {
    nodeEditText = subtreePretty();
    nodeEditError = '';
    editingNode = true;
  }
  function cancelNodeEdit() {
    editingNode = false;
    nodeEditError = '';
  }
  function applyNodeEdit() {
    try {
      const parsed = JSON.parse(nodeEditText);
      const nextRoot = setAtPath(root, selected.path, parsed);
      inspectorPayload.update((p) => (p ? { ...p, json: nextRoot } : p));
      editingNode = false;
      nodeEditError = '';
      toast('Edit applied', 'success', 1500);
    } catch (e) {
      nodeEditError = 'Invalid JSON: ' + e.message;
    }
  }
  // a whole new payload arriving discards an in-progress node edit
  // (selecting a different node already does, via onSelect above)
  $: if (payload?.at && editingNode) cancelNodeEdit();
  async function copy(text) {
    try {
      await navigator.clipboard.writeText(text);
      toast('Copied', 'success', 1500);
    } catch {}
  }

  // Table mode
  $: tableRows = Array.isArray(root) && root.every((r) => r && typeof r === 'object' && !Array.isArray(r)) ? root : null;
  $: tableCols = tableRows ? [...new Set(tableRows.flatMap((r) => Object.keys(r)))] : [];

  // Summary
  $: summary = root && typeof root === 'object' ? summarize(root) : null;
  function summarize(v) {
    const stat = { keys: 0, depth: 0, types: {} };
    const rec = (x, d) => {
      stat.depth = Math.max(stat.depth, d);
      const t = x === null ? 'null' : Array.isArray(x) ? 'array' : typeof x;
      stat.types[t] = (stat.types[t] || 0) + 1;
      if (x !== null && typeof x === 'object') {
        const ents = Array.isArray(x) ? x : Object.values(x);
        if (!Array.isArray(x)) stat.keys += Object.keys(x).length;
        ents.forEach((c) => rec(c, d + 1));
      }
    };
    rec(v, 0);
    return stat;
  }
  $: topLevel =
    root && typeof root === 'object' && !Array.isArray(root)
      ? Object.entries(root).map(([k, v]) => ({ k, t: v === null ? 'null' : Array.isArray(v) ? 'array' : typeof v, s: sizeOf(v) }))
      : [];

  // Raw mode — the whole loaded payload, pretty-printed, read-only.
  $: rawPretty =
    root === undefined || root === null
      ? ''
      : (() => {
          try {
            return JSON.stringify(root, null, 2);
          } catch {
            return String(root);
          }
        })();

  // Export the loaded payload — JSON always, CSV when it's an array of
  // objects (same shape Table mode needs).
  $: fileBase = (label || 'inspector').replace(/[^\w.-]+/g, '_').slice(0, 60) || 'inspector';
  function doExport(fmt) {
    if (root === undefined || root === null) return;
    if (fmt === 'json') {
      downloadText(`${fileBase}.json`, rawPretty, 'application/json');
    } else if (fmt === 'csv') {
      if (!tableRows) {
        toast('CSV needs an array of objects — switch to Table mode to check the shape', 'error', 3000);
        return;
      }
      const csvCols = tableCols.map((name) => ({ name }));
      const csvRows = tableRows.map((r) => tableCols.map((c) => r[c]));
      downloadText(`${fileBase}.csv`, rowsToDelimited(csvCols, csvRows, ','), 'text/csv');
    } else if (fmt === 'copy') {
      copyText(rawPretty).then((ok) => toast(ok ? 'Copied' : 'Copy blocked', ok ? 'success' : 'error', 1500));
    }
  }
</script>

<svelte:window
  on:mousemove={onDetailMove}
  on:mouseup={endDetailDrag}
/>

<div class="inspector">
  <div class="toolbar">
    <div class="modes">
      {#each ['tree', 'table', 'summary', 'raw', 'chart'] as m}
        <button class:active={mode === m} on:click={() => (mode = m)}>{m}</button>
      {/each}
    </div>
    {#if mode === 'tree'}
      <button class="btn ghost sm" on:click={expandOne}>+1</button>
      <button class="btn ghost sm" on:click={expandAll}>Expand all</button>
      <button class="btn ghost sm" on:click={collapseAll}>Collapse</button>
      <input class="input sm search" placeholder="Search keys / values…" bind:value={filter} />
      {#if filter}<span class="mc">{matchCount} match{matchCount === 1 ? '' : 'es'}</span>{/if}
    {/if}
    <span style="flex:1" />
    {#if root !== undefined && root !== null}
      <span class="export">
        <span class="ex-label">Export</span>
        <button class="btn ghost sm" on:click={() => doExport('csv')}>CSV</button>
        <button class="btn ghost sm" on:click={() => doExport('json')}>JSON</button>
        <button class="btn ghost sm" title="Copy pretty JSON" on:click={() => doExport('copy')}>{@html ICONS.copy.svg}</button>
      </span>
    {/if}
    <button class="btn ghost sm" class:active={rawMode} on:click={() => (rawMode = !rawMode)}>
      {rawMode ? '← Loaded data' : 'Paste JSON'}
    </button>
  </div>

  <div class="body" bind:this={bodyEl}>
    <div class="content">
      {#if rawMode}
        <div class="raw-tools">
          <button class="btn ghost sm" on:click={prettify} disabled={!rawText.trim()}>Prettify</button>
          <button class="btn ghost sm" on:click={minify} disabled={!rawText.trim()}>Minify</button>
          <button class="btn ghost sm" on:click={() => (rawText = '')} disabled={!rawText}>Clear</button>
          {#if rawText.trim() && !rawError}<span class="ok-tag">valid JSON</span>{/if}
        </div>
        <textarea class="raw" bind:value={rawText} placeholder={'{ "paste": "any JSON here" }'} spellcheck="false"></textarea>
        {#if rawError}<div class="raw-err">{rawError}</div>{/if}
      {/if}

      {#if root === undefined || root === null}
        {#if !rawMode}
          <div class="empty">
            Run a SQL query or send a request, then choose <b>→ Inspector</b>.<br />
            Or use <b>Paste JSON</b> above.
          </div>
        {/if}
      {:else if mode === 'tree'}
        <div class="tree-scroll">
          {#key bump + filter}
            <JsonNode
              value={root}
              path="$"
              {expandSet}
              selectedPath={selected?.path}
              {filter}
              on:select={onSelect}
              on:expandchange={expandChange}
            />
          {/key}
        </div>
      {:else if mode === 'table'}
        {#if tableRows}
          <div class="table-scroll">
            <table>
              <thead>
                <tr><th class="rn">#</th>{#each tableCols as c}<th>{c}</th>{/each}</tr>
              </thead>
              <tbody>
                {#each tableRows as row, i}
                  <tr>
                    <td class="rn">{i + 1}</td>
                    {#each tableCols as c}
                      <td>{row[c] === undefined ? '' : typeof row[c] === 'object' ? JSON.stringify(row[c]) : String(row[c])}</td>
                    {/each}
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <div class="empty">Table mode needs an array of objects.</div>
        {/if}
      {:else if mode === 'summary'}
        <div class="summary">
          <div class="stat-row">
            <div class="stat"><b>{summary?.depth ?? 0}</b><span>max depth</span></div>
            <div class="stat"><b>{summary?.keys ?? 0}</b><span>total keys</span></div>
            {#each Object.entries(summary?.types ?? {}) as [t, n]}
              <div class="stat"><b>{n}</b><span>{t}</span></div>
            {/each}
          </div>
          {#if topLevel.length}
            <table class="kv">
              <tbody>
                {#each topLevel as row}
                  <tr><td class="k">{row.k}</td><td class="t">{row.t}</td><td class="s">{row.s}</td></tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>
      {:else if mode === 'raw'}
        <div class="raw-view">
          <div class="raw-view-tools">
            <button class="btn ghost sm" on:click={() => copy(rawPretty)} disabled={!rawPretty}>{@html ICONS.copy.svg} Copy</button>
            {#if !rawMode}
              {#if editingRaw}
                <button class="btn primary sm" on:click={applyRawEdit}>Apply</button>
                <button class="btn ghost sm" on:click={cancelRawEdit}>Cancel</button>
                {#if editError}<span class="raw-err inline">{editError}</span>{/if}
              {:else}
                <button class="btn ghost sm" on:click={startRawEdit} disabled={!rawPretty}
                  >{@html ICONS.editCells.svg} Edit</button
                >
              {/if}
            {/if}
            <span class="mc">{rawPretty.length.toLocaleString()} chars</span>
          </div>
          {#if editingRaw}
            <textarea class="raw" bind:value={rawEditText} spellcheck="false"></textarea>
          {:else}
            <pre class="raw-pretty">{rawPretty}</pre>
          {/if}
        </div>
      {:else if mode === 'chart'}
        {#if tableRows}
          {#key payload?.at}
            <ChartView
              rows={tableRows}
              cols={tableCols}
              meta={payload?.source === 'sql' ? payload.meta : null}
              existing={payload?.source === 'chart-reopen' ? payload.meta?.existingChart : null}
            />
          {/key}
        {:else}
          <div class="empty">Chart mode needs an array of objects — try Table mode first to check the shape.</div>
        {/if}
      {/if}
    </div>

    <div class="detail-resizer" on:mousedown={startDetailDrag} role="separator" tabindex="-1"></div>

    <aside class="detail" style="width:{detailW}px">
      {#if selected}
        <div class="d-head">Selected node</div>
        {#if !selectedExists}
          <div class="empty small">
            This node no longer exists in the loaded data (it may have been edited or replaced).
          </div>
        {:else}
          <div class="d-field"><span>Path</span><code>{selected.path}</code></div>
          <div class="d-field"><span>Type</span><code>{selectedType}</code></div>
          <div class="d-field"><span>Size</span><code>{sizeOf(selectedValue)}</code></div>
          {#if editingNode}
            <textarea class="d-json-edit" bind:value={nodeEditText} spellcheck="false"></textarea>
            {#if nodeEditError}<div class="raw-err inline">{nodeEditError}</div>{/if}
            <div class="d-actions">
              <button class="btn primary sm" on:click={applyNodeEdit}>Apply</button>
              <button class="btn sm" on:click={cancelNodeEdit}>Cancel</button>
            </div>
          {:else}
            <pre class="d-json">{subtreePretty()}</pre>
            <div class="d-actions">
              <button class="btn sm" on:click={startNodeEdit}>{@html ICONS.editCells.svg} Edit</button>
              <button class="btn sm" on:click={() => copy(selected.path)}>Copy path</button>
              <button class="btn sm" on:click={() => copy(String(selectedValue))}>Copy value</button>
              <button class="btn sm" on:click={() => copy(subtreePretty())}>Copy pretty</button>
            </div>
          {/if}
        {/if}
      {:else}
        <div class="empty small">Select a node to inspect it.</div>
      {/if}
    </aside>
  </div>
</div>

<style>
  .inspector {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-1);
    flex-wrap: wrap;
  }
  .modes {
    display: flex;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .modes button {
    border: none;
    background: var(--surface-2);
    padding: 5px 10px;
    font-size: 11px;
    text-transform: capitalize;
    cursor: pointer;
    color: var(--text-secondary);
  }
  .modes button.active {
    background: var(--tool-inspector-tint);
    color: var(--tool-inspector-text);
    font-weight: 600;
  }
  .input.sm.search {
    width: 200px;
    padding: 5px 8px;
    font-size: 11px;
  }
  .mc {
    font-size: 10px;
    color: var(--text-muted);
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
  .btn.ghost.sm.active {
    background: var(--tool-inspector-tint);
    color: var(--tool-inspector-text);
  }
  .body {
    flex: 1;
    display: flex;
    overflow: hidden;
  }
  .content {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .detail-resizer {
    width: 5px;
    flex-shrink: 0;
    cursor: col-resize;
    background: var(--border);
  }
  .detail-resizer:hover {
    background: var(--tool-inspector-text);
  }
  .tree-scroll,
  .table-scroll {
    overflow: auto;
    flex: 1;
    padding: 6px 0;
  }
  .raw {
    width: 100%;
    height: 160px;
    border: none;
    border-bottom: 1px solid var(--border);
    padding: 10px;
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--surface-2);
    color: var(--text-primary);
    resize: vertical;
  }
  .raw-err {
    padding: 6px 10px;
    color: var(--danger);
    font-size: 11px;
    font-family: var(--font-mono);
  }
  .raw-err.inline {
    padding: 0;
  }
  .raw-tools {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  .ok-tag {
    font-size: 10px;
    color: var(--ok);
  }
  .raw-view {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .raw-view-tools {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  .raw-pretty {
    flex: 1;
    margin: 0;
    padding: 10px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-primary);
    white-space: pre;
  }
  .raw-view .raw {
    flex: 1;
    height: auto;
    border-bottom: none;
    resize: none;
  }
  .empty {
    padding: 30px;
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
    line-height: 1.6;
  }
  .empty.small {
    padding: 16px;
    font-size: 11px;
  }
  table {
    border-collapse: separate;
    border-spacing: 0;
    font-family: var(--font-mono);
    font-size: 11px;
    width: max-content;
    min-width: 100%;
  }
  th,
  td {
    border-bottom: 1px solid var(--border);
    border-right: 1px solid var(--border);
    padding: 4px 8px;
    text-align: left;
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  thead th {
    position: sticky;
    top: 0;
    background: var(--surface-1);
    font-family: var(--font-sans);
    font-weight: 600;
  }
  .rn {
    color: var(--text-muted);
    text-align: right;
  }
  .summary {
    padding: 14px;
    overflow: auto;
  }
  .stat-row {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 16px;
  }
  .stat {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 10px 14px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .stat b {
    font-size: 18px;
    color: var(--tool-inspector-text);
  }
  .stat span {
    font-size: 10px;
    color: var(--text-muted);
    text-transform: uppercase;
  }
  table.kv td.k {
    color: var(--j-key);
    font-family: var(--font-mono);
  }
  table.kv td.t {
    color: var(--text-secondary);
  }
  table.kv td.s {
    color: var(--text-muted);
  }
  .detail {
    width: 320px;
    flex-shrink: 0;
    background: var(--surface-1);
    overflow: auto;
    padding: 10px;
  }
  .d-head {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--text-secondary);
    margin-bottom: 8px;
  }
  .d-field {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 11px;
    padding: 3px 0;
  }
  .d-field span {
    color: var(--text-muted);
  }
  .d-field code {
    font-family: var(--font-mono);
    word-break: break-all;
    text-align: right;
  }
  .d-json {
    margin: 8px 0;
    padding: 8px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 11px;
    max-height: 300px;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .d-actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .d-json-edit {
    margin: 8px 0;
    padding: 8px;
    width: 100%;
    min-height: 160px;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-primary);
    resize: vertical;
  }
</style>
