<script>
  import JsonNode from './JsonNode.svelte';
  import { inspectorPayload, toast } from '../stores.js';
  import { ICONS } from '../icons.js';
  import { downloadText, copyText, rowsToDelimited } from '../export.js';

  let mode = 'tree'; // tree | table | summary
  let filter = '';
  let expandSet = new Set();
  let bump = 0; // force reactivity when expandSet mutates
  let selected = null; // {path, value, type}
  let rawMode = false;
  let rawText = '';

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
  }

  function subtreePretty() {
    if (!selected) return '';
    try {
      return JSON.stringify(selected.value, null, 2);
    } catch {
      return String(selected.value);
    }
  }
  function sizeOf(v) {
    if (v === null || typeof v !== 'object') return String(v ?? '').length + ' chars';
    return Array.isArray(v) ? `${v.length} items` : `${Object.keys(v).length} keys`;
  }
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

<div class="inspector">
  <div class="toolbar">
    <div class="modes">
      {#each ['tree', 'table', 'summary', 'raw'] as m}
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
        <button class="btn ghost sm" title="Copy pretty JSON" on:click={() => doExport('copy')}>{ICONS.copy.glyph}</button>
      </span>
    {/if}
    <button class="btn ghost sm" class:active={rawMode} on:click={() => (rawMode = !rawMode)}>
      {rawMode ? '← Loaded data' : 'Paste JSON'}
    </button>
  </div>

  <div class="body">
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
            <button class="btn ghost sm" on:click={() => copy(rawPretty)} disabled={!rawPretty}>{ICONS.copy.glyph} Copy</button>
            <span class="mc">{rawPretty.length.toLocaleString()} chars</span>
          </div>
          <pre class="raw-pretty">{rawPretty}</pre>
        </div>
      {/if}
    </div>

    <aside class="detail">
      {#if selected}
        <div class="d-head">Selected node</div>
        <div class="d-field"><span>Path</span><code>{selected.path}</code></div>
        <div class="d-field"><span>Type</span><code>{selected.type}</code></div>
        <div class="d-field"><span>Size</span><code>{sizeOf(selected.value)}</code></div>
        <pre class="d-json">{subtreePretty()}</pre>
        <div class="d-actions">
          <button class="btn sm" on:click={() => copy(selected.path)}>Copy path</button>
          <button class="btn sm" on:click={() => copy(String(selected.value))}>Copy value</button>
          <button class="btn sm" on:click={() => copy(subtreePretty())}>Copy pretty</button>
        </div>
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
    overflow: hidden;
    display: flex;
    flex-direction: column;
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
    border-left: 1px solid var(--border);
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
</style>
