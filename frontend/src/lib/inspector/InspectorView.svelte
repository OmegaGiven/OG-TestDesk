<script>
  import JsonNode from './JsonNode.svelte';
  import ChartView from './ChartView.svelte';
  import { inspectorPayload, toast, toastError, sendToInspector, connections } from '../stores.js';
  import { ICONS } from '../icons.js';
  import { downloadText, copyText, rowsToDelimited } from '../export.js';
  import { api } from '../api.js';
  import { quote, literal } from '../sqlIdent.js';
  import { decodeJwt } from '../jwt.js';
  import ExportMenu from '../components/ExportMenu.svelte';

  let mode = 'tree'; // tree | table | summary | raw | chart
  let consumedChartOpen = null;
  let filter = '';
  let expandSet = new Set();
  let bump = 0; // force reactivity when expandSet mutates
  let selected = null; // {path, value, type}
  let rawMode = false;
  let rawText = '';

  // ---- Decode JWT — a small paste-and-decode utility that hands the
  // result to the normal sendToInspector() pipeline, so it gets Tree/
  // Table/Summary/Raw and edit for free like any other loaded payload.
  let jwtMode = false;
  let jwtInput = '';
  let jwtError = '';
  function decodeJwtInput() {
    jwtError = '';
    try {
      const decoded = decodeJwt(jwtInput);
      sendToInspector('jwt', 'Decoded JWT', decoded);
      // keep the token pasted above so it can be tweaked and re-decoded;
      // the decoded tree renders right below it
      rawMode = false;
      mode = 'tree';
    } catch (e) {
      jwtError = e.message;
    }
  }

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

  // ---- write edits back to the database, when this payload came from a
  // SQL result on a plain single-table SELECT with a primary key (the
  // same check SqlView's own results grid uses for its Edit mode —
  // carried over via meta.editableTable, see SqlView's inspectResult()).
  // A pristine snapshot of the row data as it arrived, so edits made in
  // any view (Tree node edit, Raw edit, Table cell edit) can be diffed
  // against it row-by-row (matched by array index) to build UPDATEs.
  let originalRoot = null;
  let originalAt = null;
  $: if (!rawMode && payload?.at !== originalAt) {
    originalAt = payload?.at ?? null;
    originalRoot = payload ? JSON.parse(JSON.stringify(payload.json)) : null;
  }
  $: editableTable = !rawMode ? payload?.meta?.editableTable : null;
  $: dbConn = editableTable ? $connections.find((c) => c.id === payload?.meta?.connectionId) : null;
  // { rowIndex, changes: {col: newVal} }[] — only rows present in both
  // snapshots, at the same array index, with at least one changed field.
  // Rows added/removed since load are left out of the DB save entirely
  // (no safe INSERT/DELETE inference from a plain edited array) — still
  // fine to edit and export locally, just not part of what gets written.
  $: pendingDbEdits =
    editableTable && Array.isArray(root) && Array.isArray(originalRoot)
      ? root
          .map((row, i) => {
            const orig = originalRoot[i];
            if (!row || !orig || typeof row !== 'object' || typeof orig !== 'object') return null;
            const changes = {};
            for (const k of Object.keys(row)) {
              if (JSON.stringify(row[k]) !== JSON.stringify(orig[k])) changes[k] = row[k];
            }
            return Object.keys(changes).length ? { rowIndex: i, orig, changes } : null;
          })
          .filter(Boolean)
      : [];
  let savingToDb = false;
  async function saveToDatabase() {
    if (!editableTable || !dbConn || !pendingDbEdits.length) return;
    const missingPk = pendingDbEdits.find((e) => editableTable.pkCols.some((pk) => e.orig[pk] === undefined));
    if (missingPk) {
      toast('Primary key column missing from result — cannot save', 'error', 4000);
      return;
    }
    const ref =
      dbConn.kind === 'sqlite'
        ? quote(dbConn, editableTable.table)
        : `${quote(dbConn, editableTable.schema)}.${quote(dbConn, editableTable.table)}`;
    savingToDb = true;
    try {
      for (const { orig, changes } of pendingDbEdits) {
        const sets = Object.entries(changes)
          .map(([col, val]) => `${quote(dbConn, col)} = ${literal(dbConn, val)}`)
          .join(', ');
        const where = editableTable.pkCols
          .map((pk) => `${quote(dbConn, pk)} = ${literal(dbConn, orig[pk])}`)
          .join(' AND ');
        await api.queryRun(dbConn, `UPDATE ${ref} SET ${sets} WHERE ${where}`);
      }
      originalRoot = JSON.parse(JSON.stringify(root));
      inspectorPayload.update((p) => (p ? { ...p, json: root } : p));
      toast(`Saved ${pendingDbEdits.length} row${pendingDbEdits.length === 1 ? '' : 's'} to the database`, 'success', 2500);
    } catch (e) {
      toastError(e);
    } finally {
      savingToDb = false;
    }
  }
  function discardEdits() {
    if (originalRoot === null) return;
    inspectorPayload.update((p) => (p ? { ...p, json: JSON.parse(JSON.stringify(originalRoot)) } : p));
    toast('Edits discarded', 'info', 1500);
  }

  // ---- Table mode cell editing
  let editingCell = null; // { rowIndex, col } | null
  let cellEditText = '';
  function startCellEdit(rowIndex, col, value) {
    editingCell = { rowIndex, col };
    cellEditText = value === undefined || value === null ? '' : typeof value === 'object' ? JSON.stringify(value) : String(value);
  }
  function commitCellEdit() {
    if (!editingCell) return;
    const { rowIndex, col } = editingCell;
    editingCell = null;
    const row = root[rowIndex];
    if (!row) return;
    let next = cellEditText;
    // Try to preserve the original value's type (number/bool/null/object)
    // instead of silently turning everything into a string.
    const orig = row[col];
    if (typeof orig === 'number' && next.trim() !== '' && !Number.isNaN(Number(next))) next = Number(next);
    else if (typeof orig === 'boolean') next = next === 'true';
    else if (orig === null && next === '') next = null;
    else if (orig !== null && typeof orig === 'object') {
      try {
        next = JSON.parse(next);
      } catch {
        /* keep as typed string — invalid JSON for an object column */
      }
    }
    if (next === orig) return;
    const nextRoot = [...root];
    nextRoot[rowIndex] = { ...row, [col]: next };
    inspectorPayload.update((p) => (p ? { ...p, json: nextRoot } : p));
  }
  function cancelCellEdit() {
    editingCell = null;
  }

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
  // ---- open a local .json file, same idea as Paste JSON but from disk.
  // Loads through the normal `inspectorPayload` path (not rawMode) so it
  // behaves exactly like a SQL result or HTTP response sent here —
  // works in every mode (Tree/Table/Summary/Chart), not just Raw.
  let fileInputEl;
  function openFilePicker() {
    fileInputEl?.click();
  }
  function onFileChosen(e) {
    const file = e.target.files?.[0];
    e.target.value = ''; // so choosing the same file again still fires 'change'
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      try {
        const parsed = JSON.parse(String(reader.result));
        sendToInspector('file', file.name, parsed);
        rawMode = false;
      } catch (err) {
        toast(`${file.name} isn't valid JSON: ${err.message}`, 'error');
      }
    };
    reader.onerror = () => toast(`Couldn't read ${file.name}`, 'error');
    reader.readAsText(file);
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

  // Svelte's `$:` dependency tracking is purely textual — it scans the
  // reactive statement's own source for variable references, it doesn't
  // look inside a separately-declared function that the statement merely
  // *calls*. A plain `function subtreePretty() { ...reads selectedValue... }`
  // invoked as `{subtreePretty()}` in the template therefore had no
  // dependency Svelte could see (the call expression itself references
  // nothing reactive), so it was never re-run after the node's first
  // render: Path/Type/Size all reference `selected`/`selectedValue`
  // directly in their own expressions and updated correctly on every
  // click, but the value pane silently kept showing whatever was
  // selected first. Inlining the body into the `$:` statement itself
  // (rather than calling out to it) puts `selected`/`selectedValue`
  // back in view of the dependency scanner.
  $: subtreePrettyText = (() => {
    if (!selected) return '';
    try {
      return JSON.stringify(selectedValue, null, 2);
    } catch {
      return String(selectedValue);
    }
  })();
  function sizeOf(v) {
    if (v === null || typeof v !== 'object') return String(v ?? '').length + ' chars';
    return Array.isArray(v) ? `${v.length} items` : `${Object.keys(v).length} keys`;
  }

  // ---- edit the selected node's value in place
  let editingNode = false;
  let nodeEditText = '';
  let nodeEditError = '';
  function startNodeEdit() {
    nodeEditText = subtreePrettyText;
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

  // ---- "Go to line" for Raw mode — most useful on pasted logs/code/text
  // dropped in via Paste JSON or a JSON-wrapped blob, where scrolling to
  // find one specific line by eye is the whole reason you're looking at
  // Raw in the first place.
  $: rawLines = rawPretty ? rawPretty.split('\n') : [];
  let gotoLineInput = '';
  let highlightLine = null;
  let rawPrettyEl;
  let rawEditEl;
  function goToLine() {
    const n = parseInt(gotoLineInput, 10);
    if (!n || n < 1) return;
    if (n > rawLines.length) {
      toast(`Only ${rawLines.length.toLocaleString()} lines`, 'error', 2000);
      return;
    }
    if (editingRaw && rawEditEl) {
      const lines = rawEditText.split('\n');
      let offset = 0;
      for (let i = 0; i < n - 1; i++) offset += lines[i].length + 1;
      rawEditEl.focus();
      rawEditEl.setSelectionRange(offset, offset + (lines[n - 1]?.length ?? 0));
      const lineHeight = parseFloat(getComputedStyle(rawEditEl).lineHeight) || 18;
      rawEditEl.scrollTop = Math.max(0, (n - 3) * lineHeight);
    } else {
      highlightLine = n;
      rawPrettyEl?.querySelector(`[data-line="${n}"]`)?.scrollIntoView({ block: 'center' });
    }
  }
  // a fresh Raw search/edit toggle clears a stale highlight
  $: if (mode !== 'raw') highlightLine = null;
  $: if (editingRaw) highlightLine = null;

  // Export the loaded payload — JSON always, CSV when it's an array of
  // objects (same shape Table mode needs).
  $: fileBase = (label || 'inspector').replace(/[^\w.-]+/g, '_').slice(0, 60) || 'inspector';
  async function doExport(fmt, withHeaders = true) {
    if (root === undefined || root === null) return;
    let saved;
    if (fmt === 'json') {
      saved = await downloadText(`${fileBase}.json`, rawPretty, 'application/json');
    } else if (fmt === 'csv') {
      if (!tableRows) {
        toast('CSV needs an array of objects — switch to Table mode to check the shape', 'error', 3000);
        return;
      }
      const csvCols = tableCols.map((name) => ({ name }));
      const csvRows = tableRows.map((r) => tableCols.map((c) => r[c]));
      saved = await downloadText(`${fileBase}.csv`, rowsToDelimited(csvCols, csvRows, ',', withHeaders), 'text/csv');
    }
    if (saved) toast('Saved', 'success', 1500);
  }
  function doCopy() {
    copyText(rawPretty).then((ok) => toast(ok ? 'Copied' : 'Copy blocked', ok ? 'success' : 'error', 1500));
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
    {#if !rawMode && originalRoot !== null && JSON.stringify(root) !== JSON.stringify(originalRoot)}
      <span class="edited-tag">edited</span>
      {#if editableTable}
        <button
          class="btn primary sm"
          disabled={!dbConn || !pendingDbEdits.length || savingToDb}
          title={dbConn ? '' : "This result's connection isn't available"}
          on:click={saveToDatabase}
        >
          {savingToDb ? 'Saving…' : `Save ${pendingDbEdits.length || ''} to database`}
        </button>
      {/if}
      <button class="btn ghost sm" on:click={discardEdits}>Discard edits</button>
    {/if}
    {#if root !== undefined && root !== null}
      <ExportMenu
        formats={[
          { key: 'csv', label: 'CSV' },
          { key: 'json', label: 'JSON' }
        ]}
        copyTitle="Copy pretty JSON"
        on:export={(e) => doExport(e.detail.format, e.detail.withHeaders)}
        on:copy={doCopy}
      />
    {/if}
    <button class="btn ghost sm" on:click={openFilePicker}>Open file…</button>
    <input
      type="file"
      accept=".json,application/json"
      bind:this={fileInputEl}
      on:change={onFileChosen}
      style="display:none"
    />
    <button class="btn ghost sm" class:active={rawMode} on:click={() => (rawMode = !rawMode)}>
      {rawMode ? '← Loaded data' : 'Paste JSON'}
    </button>
    <button class="btn ghost sm" class:active={jwtMode} on:click={() => (jwtMode = !jwtMode)}>
      {jwtMode ? 'Hide JWT' : 'Decode JWT'}
    </button>
  </div>

  <div class="body" bind:this={bodyEl}>
    <div class="content">
      {#if jwtMode}
        <div class="raw-tools">
          <span class="jwt-hint">Paste a JWT — decoded header + payload load like any other JSON below.</span>
        </div>
        <textarea
          class="raw jwt-input"
          bind:value={jwtInput}
          placeholder="eyJhbGciOi...header.eyJzdWIiOi...payload.signature"
          spellcheck="false"
        ></textarea>
        {#if jwtError}<div class="raw-err">{jwtError}</div>{/if}
        <div class="raw-view-tools">
          <button class="btn primary sm" on:click={decodeJwtInput} disabled={!jwtInput.trim()}>Decode</button>
        </div>
      {/if}
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
        {#if !rawMode && !jwtMode}
          <div class="empty">
            Run a SQL query or send a request, then choose <b>→ Inspector</b>.<br />
            Or <b>Open file…</b> / <b>Paste JSON</b> / <b>Decode JWT</b> above.
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
                      {#if editingCell?.rowIndex === i && editingCell?.col === c}
                        <td class="editing">
                          <input
                            class="cell-edit"
                            value={cellEditText}
                            autofocus
                            on:input={(e) => (cellEditText = e.target.value)}
                            on:blur={commitCellEdit}
                            on:keydown={(e) => {
                              if (e.key === 'Enter') commitCellEdit();
                              else if (e.key === 'Escape') cancelCellEdit();
                            }}
                          />
                        </td>
                      {:else}
                        <td
                          class:cell-edited={originalRoot?.[i] && JSON.stringify(row[c]) !== JSON.stringify(originalRoot[i][c])}
                          on:click={() => startCellEdit(i, c, row[c])}
                        >{row[c] === undefined ? '' : typeof row[c] === 'object' ? JSON.stringify(row[c]) : String(row[c])}</td>
                      {/if}
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
            <form class="goto-line" on:submit|preventDefault={goToLine}>
              <span>Line</span>
              <input
                class="input sm"
                type="number"
                min="1"
                placeholder="#"
                bind:value={gotoLineInput}
                disabled={!rawPretty}
              />
              <button class="btn ghost sm" type="submit" disabled={!rawPretty || !gotoLineInput}>Go</button>
            </form>
            <span class="mc">{rawPretty.length.toLocaleString()} chars · {rawLines.length.toLocaleString()} lines</span>
          </div>
          {#if editingRaw}
            <textarea class="raw" bind:value={rawEditText} bind:this={rawEditEl} spellcheck="false"></textarea>
          {:else}
            <pre class="raw-pretty" bind:this={rawPrettyEl}>{#each rawLines as line, i}<div class="raw-line" class:hl={i + 1 === highlightLine} data-line={i + 1}>{line}</div>{/each}</pre>
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
            <pre class="d-json">{subtreePrettyText}</pre>
            <div class="d-actions">
              <button class="btn sm" on:click={startNodeEdit}>{@html ICONS.editCells.svg} Edit</button>
              <button class="btn sm" on:click={() => copy(selected.path)}>Copy path</button>
              <button class="btn sm" on:click={() => copy(String(selectedValue))}>Copy value</button>
              <button class="btn sm" on:click={() => copy(subtreePrettyText)}>Copy pretty</button>
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
  .jwt-hint {
    font-size: 11px;
    color: var(--text-muted);
  }
  .jwt-input {
    height: 120px;
    flex: none;
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
  .raw-line {
    padding: 0 4px;
    border-radius: 2px;
  }
  .raw-line.hl {
    background: color-mix(in srgb, var(--tool-inspector-text) 30%, transparent);
    outline: 1px solid var(--tool-inspector-text);
    outline-offset: -1px;
  }
  .goto-line {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .goto-line span {
    font-size: 11px;
    color: var(--text-muted);
  }
  .goto-line input {
    width: 64px;
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
  td:not(.rn) {
    cursor: pointer;
  }
  td:not(.rn):hover {
    background: var(--surface-3);
  }
  td.cell-edited {
    background: color-mix(in srgb, var(--tool-inspector-text) 16%, transparent);
  }
  td.editing {
    padding: 0;
  }
  .cell-edit {
    width: 100%;
    box-sizing: border-box;
    border: none;
    outline: 2px solid var(--tool-inspector-text);
    outline-offset: -2px;
    background: var(--surface-1);
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: 11px;
    padding: 4px 8px;
  }
  .edited-tag {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--tool-inspector-text);
    background: var(--tool-inspector-tint);
    padding: 2px 6px;
    border-radius: 3px;
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
