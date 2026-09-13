<script>
  import { get } from 'svelte/store';
  import { tick } from 'svelte';
  import { ICONS } from '../icons.js';
  import CodeEditor from '../components/CodeEditor.svelte';
  import ResultGrid from './ResultGrid.svelte';
  import SchemaTree from './SchemaTree.svelte';
  import SavedQueries from './SavedQueries.svelte';
  import ConnPicker from './ConnPicker.svelte';
  import { api } from '../api.js';
  import { format as formatSqlText } from 'sql-formatter';
  import { quote, literal, defaultSchema } from '../sqlIdent.js';
  import CsvImportModal from './CsvImportModal.svelte';
  import { downloadText } from '../export.js';
  import {
    connections,
    sqlTabs,
    activeSqlTab,
    activeSqlTabId,
    newSqlTab,
    touchSqlTab,
    persistSqlTab,
    toast,
    toastError,
    sendToInspector,
    historyLoad,
    appearance,
    savedQueries,
    reloadSavedQueries,
    splitTabId,
    closeSplit,
    draggingSqlTab,
    promptDialog,
    confirmDialog
  } from '../stores.js';

  // When this instance is one half of a split (rendered via
  // <svelte:self tabId={...}> below), it shows that specific tab
  // instead of whatever's globally active. The top-level instance
  // (no tabId prop) is what +page.svelte mounts. `side` only matters
  // for a split sub-pane, so a tab dropped on it knows whether to
  // become the left (primary) or right tab.
  export let tabId = null;
  export let side = null; // null | 'left' | 'right'
  let showCsvImport = false;

  // ---- split-screen host (top-level instance only): render two
  // <svelte:self> side by side, resizable, when a split is active.
  let hSplitPct = 50;
  let hDragging = false;
  let hostEl;
  function startHDrag() {
    hDragging = true;
  }
  function onHMove(e) {
    if (!hDragging || !hostEl) return;
    const rect = hostEl.getBoundingClientRect();
    hSplitPct = Math.min(80, Math.max(20, ((e.clientX - rect.left) / rect.width) * 100));
  }
  function endHDrag() {
    hDragging = false;
  }

  // ---- drop a dragged SQL tab onto this pane to open/replace a split
  let dropSide = null; // 'left' | 'right' | null
  function onPaneDragOver(e) {
    if (!$draggingSqlTab) return;
    e.preventDefault();
    if (tabId) {
      dropSide = side;
      return;
    }
    const rect = e.currentTarget.getBoundingClientRect();
    dropSide = (e.clientX - rect.left) / rect.width < 0.5 ? 'left' : 'right';
  }
  function onPaneDragLeave() {
    dropSide = null;
  }
  function onPaneDrop(e) {
    e.preventDefault();
    const dragged = $draggingSqlTab;
    const chosen = dropSide;
    dropSide = null;
    draggingSqlTab.set(null);
    if (!dragged) return;
    if (tabId) {
      if (side === 'left') activeSqlTabId.set(dragged.id);
      else if (side === 'right') splitTabId.set(dragged.id);
      return;
    }
    if (dragged.id === tab?.id) return;
    if (chosen === 'left') {
      if (tab) splitTabId.set(tab.id);
      activeSqlTabId.set(dragged.id);
    } else {
      splitTabId.set(dragged.id);
    }
  }

  let sqOpen =
    typeof location === 'undefined' || !new URLSearchParams(location.search).has('savedqueriesclosed');
  let consumedHistory = null;

  $: if ($historyLoad && $historyLoad.kind === 'sql' && $historyLoad.at !== consumedHistory) {
    consumedHistory = $historyLoad.at;
    openFromHistory($historyLoad.entry, $historyLoad.resolved);
  }

  async function openFromHistory(entry, resolved) {
    const connId =
      ($connections.find((c) => c.id === entry.connection_id) || {}).id ||
      sidebarConnId ||
      $connections[0]?.id;
    if (!connId) return;
    const t = await newSqlTab(connId, entry.sql_text);
    touchSqlTab(t.id, { title: 'History', dirty: false });
    if (resolved) {
      try {
        touchSqlTab(t.id, { result: JSON.parse(resolved) });
      } catch {}
    } else if (entry.error) {
      touchSqlTab(t.id, { error: entry.error });
    }
    persistSqlTab(t.id, true);
  }
  let splitPct = 55;
  let dragging = false;

  const SIDEBAR_W_KEY = 'ogtestdesk.sql.sidebarW';
  function loadSidebarW() {
    try {
      const n = Number(localStorage.getItem(SIDEBAR_W_KEY));
      return n >= 180 && n <= 480 ? n : 260;
    } catch {
      return 260;
    }
  }
  let sidebarW = loadSidebarW();
  let rootEl;
  // A split sub-pane starts with its sidebar (saved queries/schema tree)
  // collapsed, since it's the thing that made a narrow split pane feel
  // cramped — one click on the resizer (or dragging it) brings it back.
  let sidebarCollapsed = !!tabId;
  let draggingSidebar = false;
  let sidebarDragStartX = 0;
  let sidebarDragMoved = false;
  function startSidebarDrag(e) {
    draggingSidebar = true;
    sidebarDragStartX = e.clientX;
    sidebarDragMoved = false;
  }
  function onSidebarMove(e) {
    if (!draggingSidebar || !rootEl) return;
    if (Math.abs(e.clientX - sidebarDragStartX) > 3) sidebarDragMoved = true;
    if (sidebarCollapsed) return;
    const rect = rootEl.getBoundingClientRect();
    sidebarW = Math.min(480, Math.max(180, e.clientX - rect.left));
  }
  function endSidebarDrag() {
    if (!draggingSidebar) return;
    draggingSidebar = false;
    if (!sidebarDragMoved) {
      // a plain click (no drag) on the resizer toggles collapse instead
      sidebarCollapsed = !sidebarCollapsed;
      return;
    }
    try {
      localStorage.setItem(SIDEBAR_W_KEY, String(Math.round(sidebarW)));
    } catch {}
  }

  $: conns = $connections;
  $: tab = tabId ? $sqlTabs.find((t) => t.id === tabId) : $activeSqlTab;
  $: tabConn = tab ? conns.find((c) => c.id === tab.connection_id) : null;
  // schema tree + fallbacks follow the active tab's connection
  $: sidebarConn = tabConn || conns[0];
  $: sidebarConnId = sidebarConn?.id;

  // ---- DB clock, shown next to the connection badge so a DB on a
  // different timezone (or a stopped/drifted clock) is obvious at a glance
  let dbTime = null;
  let dbTimeConnId = null;
  let dbTimeErr = false;
  $: if (tabConn && tabConn.id !== dbTimeConnId) loadDbTime(tabConn);
  async function loadDbTime(conn) {
    dbTimeConnId = conn.id;
    dbTimeErr = false;
    try {
      dbTime = await api.dbTime(conn);
    } catch {
      dbTime = null;
      dbTimeErr = true;
    }
  }
  function fmtOffset(secs) {
    const sign = secs >= 0 ? '+' : '−';
    const abs = Math.abs(secs);
    const h = Math.floor(abs / 3600);
    const m = Math.floor((abs % 3600) / 60);
    return `UTC${sign}${h}${m ? ':' + String(m).padStart(2, '0') : ''}`;
  }
  function fmtDiff(dbSecs) {
    const localSecs = -new Date().getTimezoneOffset() * 60;
    const diff = dbSecs - localSecs;
    if (Math.abs(diff) < 60) return 'same as your system';
    const h = Math.abs(diff) / 3600;
    const hh = Number.isInteger(h) ? h : h.toFixed(1);
    return `${hh}h ${diff > 0 ? 'ahead of' : 'behind'} you`;
  }

  // ---- SQL variables: {{name}} tokens filled from slots above the editor
  const VAR_RE = /\{\{\s*([A-Za-z_]\w*)\s*\}\}/g;
  let varValues = {}; // { [tabId]: { [name]: value } }
  let loadedVarsFor = null;
  let varTimer;

  $: vars = tab
    ? [...new Set([...tab.sql_text.matchAll(VAR_RE)].map((m) => m[1]))]
    : [];

  $: if (tab && tab.id !== loadedVarsFor) {
    loadedVarsFor = tab.id;
    loadVars(tab.id);
  }

  async function loadVars(id) {
    try {
      const raw = await api.stateGet('sqlvars:' + id);
      varValues = { ...varValues, [id]: raw ? JSON.parse(raw) : {} };
    } catch {
      varValues = { ...varValues, [id]: {} };
    }
  }
  function setVar(name, value) {
    const id = tab.id;
    varValues = { ...varValues, [id]: { ...(varValues[id] || {}), [name]: value } };
    clearTimeout(varTimer);
    varTimer = setTimeout(
      () => api.stateSet('sqlvars:' + id, JSON.stringify(varValues[id] || {})).catch(() => {}),
      500
    );
  }
  function applyVars(sql, id) {
    const vals = varValues[id] || {};
    return sql.replace(VAR_RE, (m, name) => (name in vals && vals[name] !== '' ? vals[name] : m));
  }
  $: missingVars = tab ? vars.filter((v) => !((varValues[tab.id] || {})[v] ?? '')) : [];

  // Recognizes exactly the `openRelation()`-style statement (`SELECT *
  // FROM [schema.]table [WHERE|ORDER BY|LIMIT ...]`) that real row
  // editing supports — anything with an explicit column list, a join, or
  // an aggregate can't be safely mapped back to one table's primary key.
  function parseSingleTable(sql) {
    const s = sql.trim().replace(/;\s*$/, '');
    const ident = '(?:"[^"]+"|`[^`]+`|\\[[^\\]]+\\]|[A-Za-z_][\\w$]*)';
    const re = new RegExp(`^select\\s+\\*\\s+from\\s+(${ident}(?:\\.${ident})?)\\s*(where\\s|order\\s+by\\s|limit\\s|$)`, 'is');
    const m = s.match(re);
    if (!m) return null;
    const unquote = (id) => id.replace(/^["`[]/, '').replace(/["'`\]]$/, '');
    const parts = m[1].split('.');
    return parts.length === 2
      ? { schema: unquote(parts[0]), table: unquote(parts[1]) }
      : { schema: null, table: unquote(parts[0]) };
  }

  let columnsCache = {}; // `${connId}:${schema}.${table}` -> Promise<Column[]>
  async function refreshEditableTable(t, conn, sql) {
    const parsed = parseSingleTable(sql);
    if (!parsed || conn.read_only) {
      touchSqlTab(t.id, { editableTable: null });
      return;
    }
    const schema = parsed.schema || defaultSchema(conn);
    const key = `${conn.id}:${schema}.${parsed.table}`;
    try {
      if (!columnsCache[key]) columnsCache[key] = api.columnsList(conn, schema, parsed.table);
      const cols = await columnsCache[key];
      const pkCols = cols.filter((c) => c.primary_key).map((c) => c.name);
      touchSqlTab(t.id, { editableTable: pkCols.length ? { schema, table: parsed.table, pkCols } : null });
    } catch {
      touchSqlTab(t.id, { editableTable: null });
    }
  }

  async function onSaveEdits(e) {
    const t = tab;
    const et = t?.editableTable;
    if (!t || !et || !t.result) return;
    const conn = tabConn;
    const cols = t.result.columns;
    const pkIdx = et.pkCols.map((pk) => cols.findIndex((c) => c.name === pk));
    if (pkIdx.some((i) => i < 0)) {
      toast('Primary key column missing from result — cannot save', 'error', 4000);
      return;
    }
    const ref =
      conn.kind === 'sqlite' ? quote(conn, et.table) : `${quote(conn, et.schema)}.${quote(conn, et.table)}`;
    const whereFor = (row) =>
      pkIdx.map((pi, i) => `${quote(conn, et.pkCols[i])} = ${literal(conn, row[pi])}`).join(' AND ');

    const stmts = [];
    for (const { rowIndex, changes } of e.detail.edits) {
      const row = t.result.rows[rowIndex];
      if (!row) continue;
      const sets = Object.entries(changes)
        .map(([ci, val]) => `${quote(conn, cols[+ci].name)} = ${literal(conn, val)}`)
        .join(', ');
      if (!sets) continue;
      stmts.push(`UPDATE ${ref} SET ${sets} WHERE ${whereFor(row)}`);
    }
    for (const rowIndex of e.detail.deletes) {
      const row = t.result.rows[rowIndex];
      if (!row) continue;
      stmts.push(`DELETE FROM ${ref} WHERE ${whereFor(row)}`);
    }
    for (const values of e.detail.inserts) {
      const names = [];
      const vals = [];
      for (const ci in values) {
        names.push(quote(conn, cols[+ci].name));
        vals.push(literal(conn, values[ci]));
      }
      if (!names.length) continue;
      stmts.push(`INSERT INTO ${ref} (${names.join(', ')}) VALUES (${vals.join(', ')})`);
    }
    if (!stmts.length) return;

    const preview = stmts.slice(0, 5).join(';\n') + (stmts.length > 5 ? `\n… +${stmts.length - 5} more` : '');
    const ok = await confirmDialog(
      `Run ${stmts.length} SQL statement${stmts.length === 1 ? '' : 's'} against "${conn.nickname}"?\n\n${preview}`,
      { danger: true }
    );
    if (!ok) return;

    for (let i = 0; i < stmts.length; i++) {
      try {
        await api.queryRun(conn, stmts[i], null, null, false);
      } catch (err) {
        toast(
          `Statement ${i + 1}/${stmts.length} failed: ${err} — ${i} of ${stmts.length} already applied; re-run the query to see current state.`,
          'error',
          8000
        );
        return;
      }
    }
    toast(`${stmts.length} statement${stmts.length === 1 ? '' : 's'} applied`, 'success', 2500);
    await run();
  }

  async function openRelation(e) {
    const { schema, relation } = e.detail;
    const c = sidebarConn;
    // No LIMIT — the paged run + infinite scroll below handles browsing the
    // whole table without pulling it all into memory at once.
    const q =
      c.kind === 'sqlite'
        ? `SELECT * FROM ${quote(c, relation)};`
        : `SELECT * FROM ${quote(c, schema)}.${quote(c, relation)};`;
    const t = await newSqlTab(c.id, q);
    touchSqlTab(t.id, { title: relation });
    persistSqlTab(t.id, true);
    setTimeout(() => run(), 30);
  }

  function onChange(v) {
    if (!tab) return;
    touchSqlTab(tab.id, { sql_text: v, dirty: true });
    persistSqlTab(tab.id);
    // A real keystroke (not our own cycleHistory-driven update) means the
    // user has diverged from wherever they were in the history buffer —
    // drop it so the next Alt-Up starts a fresh cycle from here.
    if (!suppressHistoryReset && historyState[tab.id]) {
      const { [tab.id]: _drop, ...rest } = historyState;
      historyState = rest;
    }
    suppressHistoryReset = false;
  }

  const FORMATTER_LANG = { postgres: 'postgresql', mysql: 'mysql', sqlite: 'sqlite' };
  function formatSql() {
    const t = tab;
    if (!t) return;
    const src = selectionOrAll(t.sql_text);
    if (!src.trim()) return;
    try {
      const language = FORMATTER_LANG[tabConn?.kind] || 'sql';
      const formatted = formatSqlText(src, { language, keywordCase: 'upper' });
      // Selection-only formatting isn't supported here — always
      // reformats the whole statement, same as CodeMirror's own
      // save/run shortcuts operate on the full tab text.
      touchSqlTab(t.id, { sql_text: formatted, dirty: true });
      persistSqlTab(t.id);
    } catch (e) {
      toast(`Couldn't format: ${e}`, 'error', 3000);
    }
  }

  // Splits a script into top-level statements on `;`, respecting line
  // comments, block comments, and quoted strings (single/double/back-tick,
  // with doubled-quote escaping) so a semicolon inside a string literal
  // doesn't end a statement early. Doesn't understand Postgres
  // dollar-quoted (`$$...$$`) function bodies — a script that uses those
  // should be run as one statement via plain Run, not Run All.
  function splitStatements(sql) {
    const stmts = [];
    let cur = '';
    let i = 0;
    const n = sql.length;
    while (i < n) {
      const c = sql[i];
      if (c === '-' && sql[i + 1] === '-') {
        const nl = sql.indexOf('\n', i);
        const end = nl === -1 ? n : nl;
        cur += sql.slice(i, end);
        i = end;
        continue;
      }
      if (c === '/' && sql[i + 1] === '*') {
        const close = sql.indexOf('*/', i + 2);
        const end = close === -1 ? n : close + 2;
        cur += sql.slice(i, end);
        i = end;
        continue;
      }
      if (c === "'" || c === '"' || c === '`') {
        const quote = c;
        let j = i + 1;
        while (j < n) {
          if (sql[j] === quote) {
            if (sql[j + 1] === quote) {
              j += 2;
              continue;
            }
            j++;
            break;
          }
          if (sql[j] === '\\' && quote !== '`') {
            j += 2;
            continue;
          }
          j++;
        }
        cur += sql.slice(i, j);
        i = j;
        continue;
      }
      if (c === ';') {
        if (cur.trim()) stmts.push(cur.trim());
        cur = '';
        i++;
        continue;
      }
      cur += c;
      i++;
    }
    if (cur.trim()) stmts.push(cur.trim());
    return stmts;
  }

  async function runAll() {
    const t = tab;
    if (!t || t.running) return;
    const conn = get(connections).find((c) => c.id === t.connection_id);
    if (!conn) return;
    const raw = applyVars(t.sql_text, t.id);
    if (VAR_RE.test(raw)) {
      VAR_RE.lastIndex = 0;
      toast('Unfilled variables — set values in the bar above the editor', 'error', 4000);
      return;
    }
    const stmts = splitStatements(raw);
    if (stmts.length === 0) return;
    if (stmts.length === 1) {
      run();
      return;
    }
    touchSqlTab(t.id, { running: true, error: null, multiResults: [], activeResultIdx: 0, editableTable: null });
    const results = [];
    for (const sql of stmts) {
      try {
        const result = await api.queryRun(conn, sql, null, null, false);
        results.push({ sql, result, error: null });
      } catch (e) {
        results.push({ sql, result: null, error: String(e) });
        touchSqlTab(t.id, { multiResults: [...results], running: false, activeResultIdx: results.length - 1 });
        toast(`Statement ${results.length}/${stmts.length} failed — stopped there`, 'error', 5000);
        return;
      }
    }
    touchSqlTab(t.id, {
      multiResults: results,
      running: false,
      activeResultIdx: results.length - 1
    });
    toast(`${stmts.length} statements executed`, 'success', 2500);
  }

  // Alt-Up/Alt-Down in the editor cycle through this connection's recent
  // history (newest first). The in-progress edit is stashed as a "draft"
  // so cycling back down past the newest history entry restores it,
  // matching a shell history buffer.
  let historyState = {}; // tab.id -> { list, idx, draft }
  let suppressHistoryReset = false;
  async function cycleHistory(dir) {
    const t = tab;
    if (!t) return;
    let hs = historyState[t.id];
    if (!hs) {
      let list = [];
      try {
        const all = await api.historyRecent(300);
        list = all.filter((h) => h.connection_id === t.connection_id && h.sql_text?.trim());
      } catch {
        return;
      }
      hs = { list, idx: -1, draft: t.sql_text };
      historyState = { ...historyState, [t.id]: hs };
    }
    if (!hs.list.length) return;
    const nextIdx = Math.max(-1, Math.min(hs.list.length - 1, hs.idx + dir));
    if (nextIdx === hs.idx) return;
    if (hs.idx === -1 && nextIdx >= 0) hs.draft = t.sql_text; // capture draft on first step back
    hs.idx = nextIdx;
    const text = nextIdx === -1 ? hs.draft : hs.list[nextIdx].sql_text;
    suppressHistoryReset = true;
    touchSqlTab(t.id, { sql_text: text, dirty: true });
  }

  // Table-name completion for the SQL editor (`{ table: [] }` — no
  // column-level data, which would mean eagerly fetching columns for
  // every table in the schema). Cached per connection so switching
  // between tabs on the same connection doesn't refetch.
  let autocompleteSchema = null;
  let autocompleteCache = {};
  $: if (tabConn) loadAutocompleteSchema(tabConn);
  async function loadAutocompleteSchema(conn) {
    if (autocompleteCache[conn.id]) {
      autocompleteSchema = autocompleteCache[conn.id];
      return;
    }
    try {
      const schemas = await api.schemasList(conn);
      const map = {};
      for (const s of schemas) {
        for (const r of s.relations || []) {
          map[r.name] = [];
          map[`${s.name}.${r.name}`] = [];
        }
      }
      autocompleteCache[conn.id] = map;
      if (tabConn?.id === conn.id) autocompleteSchema = map;
    } catch {
      // Best-effort — plain keyword completion still works without this.
    }
  }

  // page < 0 → full result (no pagination wrapper, capped by max rows)
  async function run(page = 0) {
    const t = tab;
    if (!t || t.running) return;
    const conn = get(connections).find((c) => c.id === t.connection_id);
    if (!conn) return;

    let sql;
    if (page > 0 && t.execSql) {
      sql = t.execSql; // paging — reuse the exact statement page 0 ran
    } else {
      sql = applyVars(selectionOrAll(t.sql_text), t.id);
      if (!sql.trim()) return;
      if (VAR_RE.test(sql)) {
        VAR_RE.lastIndex = 0;
        toast('Unfilled variables — set values in the bar above the editor', 'error', 4000);
        return;
      }
      touchSqlTab(t.id, { execSql: sql });
    }

    const full = page < 0;
    const size = get(appearance).pageSize ?? 500;
    touchSqlTab(t.id, { running: true, error: null, multiResults: null });
    try {
      const result = await api.queryRun(
        conn,
        sql,
        full ? null : Math.max(0, page),
        full ? null : size,
        !full && page === 0
      );
      touchSqlTab(t.id, { result, running: false });
      if (result.is_select) {
        const shown = result.total != null ? ` of ${result.total.toLocaleString()}` : '';
        toast(`${result.row_count.toLocaleString()} rows${shown} · ${result.duration_ms} ms`, 'success', 2500);
        refreshEditableTable(t, conn, sql);
      } else {
        touchSqlTab(t.id, { editableTable: null });
      }
    } catch (e) {
      touchSqlTab(t.id, { error: String(e), running: false });
    }
  }

  // Infinite scroll: fetch the next page and append it to the rows already
  // loaded, instead of replacing them — so scrolling (while still searching
  // / filtering client-side) keeps pulling more of the table in.
  let appendFlag = false;
  async function loadMore() {
    const t = tab;
    const r = t?.result;
    if (!t || !r || !r.has_more || t.running || !t.execSql) return;
    const conn = get(connections).find((c) => c.id === t.connection_id);
    if (!conn) return;
    touchSqlTab(t.id, { running: true });
    try {
      const size = get(appearance).pageSize ?? 500;
      const next = await api.queryRun(conn, t.execSql, r.page + 1, size, false);
      const merged = {
        ...next,
        rows: [...r.rows, ...next.rows],
        row_count: r.row_count + next.row_count,
        total: r.total ?? next.total
      };
      appendFlag = true;
      touchSqlTab(t.id, { result: merged, running: false });
      await tick();
      appendFlag = false;
    } catch (e) {
      touchSqlTab(t.id, { running: false });
      toastError(e);
    }
  }

  function selectionOrAll(text) {
    const sel = window.getSelection()?.toString();
    return sel && sel.trim() ? sel : text;
  }

  async function saveQuery() {
    if (!tab) return;
    const raw = await promptDialog('Save query as:', tab.title);
    if (!raw || !raw.trim()) return;
    const name = raw.trim();
    // update in place if a query with the same name/connection exists
    const existing = get(savedQueries).find(
      (s) => s.name === name && s.connection_id === tab.connection_id
    );
    try {
      await api.savedQuerySave({
        id: existing?.id || '',
        connection_id: tab.connection_id,
        folder_id: existing?.folder_id ?? null,
        name,
        sql_text: tab.sql_text,
        sort_order: existing?.sort_order || 0,
        created_at: existing?.created_at || 0
      });
      await reloadSavedQueries();
      sqOpen = true;
      toast(existing ? 'Saved query updated' : 'Query saved', 'success', 1800);
    } catch (e) {
      toastError(e);
    }
  }

  function saveToFile() {
    if (!tab) return;
    const base = (tab.title || 'query').replace(/[^\w.-]+/g, '_').slice(0, 60) || 'query';
    downloadText(`${base}.sql`, tab.sql_text, 'application/sql');
  }

  async function openSavedQuery(e) {
    const s = e.detail;
    const connId =
      $connections.find((c) => c.id === s.connection_id)?.id || sidebarConnId || $connections[0]?.id;
    if (!connId) return;
    // already open? focus that tab instead of opening a duplicate
    const existing = get(sqlTabs).find(
      (t) => t.connection_id === connId && t.title === s.name
    );
    if (existing) {
      activeSqlTabId.set(existing.id);
      persistSqlTab(existing.id, true);
      return;
    }
    const t = await newSqlTab(connId, s.sql_text);
    touchSqlTab(t.id, { title: s.name, dirty: false });
    persistSqlTab(t.id, true);
  }

  function inspectResult() {
    const r = tab?.result;
    if (!r || !r.is_select) return;
    const objs = r.rows.map((row) => Object.fromEntries(r.columns.map((c, i) => [c.name, row[i]])));
    sendToInspector('sql', `${tab.title} (${r.row_count} rows)`, objs, {
      connectionId: tab.connection_id,
      sql: tab.execSql || tab.sql_text
    });
  }

  // splitter drag (editor vs. result pane, vertical)
  let workareaEl;
  function startDrag() {
    dragging = true;
  }
  function onMove(e) {
    if (!dragging) return;
    const host = workareaEl;
    if (!host) return;
    const rect = host.getBoundingClientRect();
    splitPct = Math.min(85, Math.max(15, ((e.clientY - rect.top) / rect.height) * 100));
  }
  function endDrag() {
    dragging = false;
  }
</script>

<svelte:window
  on:mousemove={(e) => {
    onMove(e);
    onSidebarMove(e);
    onHMove(e);
  }}
  on:mouseup={() => {
    endDrag();
    endSidebarDrag();
    endHDrag();
  }}
/>

{#if !tabId && $splitTabId}
  <div class="split-host" bind:this={hostEl}>
    <div class="split-pane" style="width:{hSplitPct}%">
      <svelte:self tabId={$activeSqlTabId} side="left" />
    </div>
    <div class="split-resizer" on:mousedown={startHDrag} role="separator" tabindex="-1">
      <button class="split-close" title="Close split" on:click|stopPropagation={closeSplit}
        >{ICONS.closeTab.glyph}</button
      >
    </div>
    <div class="split-pane" style="width:{100 - hSplitPct}%">
      <svelte:self tabId={$splitTabId} side="right" />
    </div>
  </div>
{:else}
<div class="sql" bind:this={rootEl}>
  {#if tab}
    <aside class="sidebar" class:collapsed={sidebarCollapsed} style="width:{sidebarCollapsed ? 0 : sidebarW}px">
      <div class="sq-section" class:open={sqOpen}>
        <button class="sec-head sq-toggle" on:click={() => (sqOpen = !sqOpen)}>
          <span class="chev">{sqOpen ? ICONS.expandOpen.glyph : ICONS.expandClosed.glyph}</span>
          <span>Saved queries</span>
          <span class="sq-badge">{$savedQueries.length}</span>
        </button>
        {#if sqOpen}
          <div class="sq-body">
            <SavedQueries on:open={openSavedQuery} />
          </div>
        {/if}
      </div>

      {#if sidebarConn}
        <div class="schema-host">
          <SchemaTree conn={sidebarConn} on:open={openRelation} />
        </div>
      {/if}
    </aside>

    <div
      class="sidebar-resizer"
      class:collapsed={sidebarCollapsed}
      on:mousedown={startSidebarDrag}
      title="Drag to resize · click to {sidebarCollapsed ? 'expand' : 'collapse'}"
      role="separator"
      tabindex="-1"
    >
      <span class="resizer-chev">{sidebarCollapsed ? '›' : '‹'}</span>
    </div>
  {/if}

  <section
    class="main"
    on:dragover={onPaneDragOver}
    on:dragleave={onPaneDragLeave}
    on:drop={onPaneDrop}
  >
    {#if $draggingSqlTab && $draggingSqlTab.id !== tab?.id && (tabId || !$splitTabId)}
      <div class="drop-overlay">
        {#if !tabId}
          <div class="dz dz-left" class:hot={dropSide === 'left'}>Split left</div>
          <div class="dz dz-right" class:hot={dropSide === 'right'}>Split right</div>
        {:else}
          <div class="dz dz-full" class:hot={dropSide === side}>Replace this pane</div>
        {/if}
      </div>
    {/if}
    {#if !tab}
      <div class="empty">
        <div class="empty-picker">
          <p class="empty-hint">No tabs open — pick something to start.</p>
          <ConnPicker />
        </div>
      </div>
    {:else}
      <div class="toolbar">
        <button class="btn primary sm" on:click={() => run()} disabled={tab.running}>
          {tab.running ? 'Running…' : '▶ Run'}
        </button>
        <button class="btn ghost sm" title="Run every statement in the editor, one after another" on:click={runAll} disabled={tab.running}>
          Run All
        </button>
        <button class="btn" on:click={saveQuery}>Save</button>
        <button class="btn ghost sm" title="Format SQL (⇧⌥F)" on:click={formatSql}>Format</button>
        {#if tabConn && !tabConn.read_only}
          <button class="btn ghost sm" title="Import a CSV file into this connection" on:click={() => (showCsvImport = true)}>
            Import CSV
          </button>
        {/if}
        <button class="icon-btn big-glyph" title={ICONS.saveFile.label} on:click={saveToFile}
          >{ICONS.saveFile.glyph}</button
        >
        <span class="tb-conn" style="--c: {tabConn?.color || 'var(--conn-slate)'}">
          {tabConn?.nickname}
        </span>
        <span class="tb-conn tb-tab" style="--c: {tabConn?.color || 'var(--conn-slate)'}" title={tab.title}>
          {tab.dirty ? '•' : ''}{tab.title}
        </span>
        {#if dbTime}
          <span
            class="tb-tz"
            title="Server local time: {dbTime.local_time}{dbTime.tz_name ? ` (${dbTime.tz_name})` : ''}"
          >
            {fmtOffset(dbTime.utc_offset_secs)} · {fmtDiff(dbTime.utc_offset_secs)}
          </span>
        {:else if dbTimeErr}
          <span class="tb-tz muted">clock unavailable</span>
        {/if}
        {#if tab.result?.is_select && tab.result.page_size > 0}
          {@const r = tab.result}
          <span class="pager">
            <span class="pg-info">
              {r.row_count.toLocaleString()}{r.total != null
                ? ` of ${r.total.toLocaleString()}`
                : r.has_more
                  ? '+'
                  : ''} rows loaded
            </span>
            {#if r.has_more}<span class="pg-hint">— scroll for more</span>{/if}
            {#if r.count_ms != null}<span class="pg-ct">count {r.count_ms}ms</span>{/if}
          </span>
        {/if}
        <span style="flex:1" />
        {#if tab.result?.is_select && tab.result.page_size > 0 && (tab.result.has_more || tab.result.page > 0)}
          <button class="btn ghost sm" on:click={() => run(-1)}>Load all</button>
        {/if}
      </div>

      {#if vars.length}
        <div class="varbar">
          <span class="vb-label">Variables</span>
          {#each vars as v (v)}
            <label class="vb-slot" class:missing={missingVars.includes(v)}>
              <span class="vb-name">{`{{${v}}}`}</span>
              <input
                class="vb-input"
                value={(varValues[tab.id] || {})[v] ?? ''}
                on:input={(e) => setVar(v, e.target.value)}
                placeholder="value"
              />
            </label>
          {/each}
        </div>
      {/if}

      <div class="workarea" bind:this={workareaEl}>
        <div class="pane editor-pane" style="height:{splitPct}%">
          <CodeEditor
            value={tab.sql_text}
            language="sql"
            schema={autocompleteSchema}
            on:change={(e) => onChange(e.detail)}
            on:run={() => run()}
            on:save={saveQuery}
            on:histprev={() => cycleHistory(-1)}
            on:histnext={() => cycleHistory(1)}
          />
        </div>
        <div class="splitter" on:mousedown={startDrag} role="separator" tabindex="-1"></div>
        <div class="pane result-pane" style="height:{100 - splitPct}%">
          {#if tab.multiResults?.length > 1}
            <div class="multi-strip">
              {#each tab.multiResults as mr, i (i)}
                <button
                  class="multi-tab"
                  class:on={tab.activeResultIdx === i}
                  class:bad={mr.error}
                  title={mr.error || mr.sql}
                  on:click={() => touchSqlTab(tab.id, { activeResultIdx: i })}
                >
                  {i + 1}{mr.error ? ' ✕' : mr.result?.is_select ? ` (${mr.result.row_count})` : ` (${mr.result?.rows_affected ?? 0})`}
                </button>
              {/each}
            </div>
          {/if}
          {#if tab.error}
            <div class="err">{tab.error}</div>
          {:else if tab.multiResults?.length > 1 && tab.multiResults[tab.activeResultIdx]?.error}
            <div class="err">{tab.multiResults[tab.activeResultIdx].error}</div>
          {:else}
            <ResultGrid
              result={tab.multiResults?.length > 1 ? tab.multiResults[tab.activeResultIdx]?.result : tab.result}
              name={tab.title}
              loadingMore={tab.running}
              appending={appendFlag}
              editableTable={tab.multiResults?.length > 1 ? null : tab.editableTable}
              on:loadmore={loadMore}
              on:inspect={inspectResult}
              on:save={onSaveEdits}
            />
          {/if}
        </div>
      </div>

      <div class="statusbar">
        {#if tab.error}
          <span class="s-err">error</span>
        {:else if tab.result}
          {tab.result.is_select
            ? tab.result.total != null
              ? `${tab.result.total.toLocaleString()} rows total`
              : `${tab.result.row_count.toLocaleString()} rows`
            : `${tab.result.rows_affected} affected`} · {tab.result.duration_ms} ms{tab.result.truncated
            ? ' · capped'
            : ''}
        {:else}
          ready · ⌘↵ run
        {/if}
      </div>
    {/if}
  </section>
  {#if showCsvImport && tabConn}
    <CsvImportModal
      conn={tabConn}
      on:close={() => (showCsvImport = false)}
      on:imported={() => {
        showCsvImport = false;
        toast('Refresh the schema panel (↻) to see the new/updated table', 'success', 4000);
      }}
    />
  {/if}
</div>
{/if}

<style>
  .sql {
    display: flex;
    height: 100%;
    overflow: hidden;
  }
  .sidebar {
    flex-shrink: 0;
    background: var(--surface-1);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    transition: width 0.14s ease;
  }
  .sidebar-resizer {
    position: relative;
    width: 5px;
    flex-shrink: 0;
    cursor: col-resize;
    background: var(--border);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sidebar-resizer:hover {
    background: var(--tool-sql-text);
  }
  .resizer-chev {
    width: 14px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 3px;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    color: var(--text-muted);
    font-size: 10px;
    line-height: 1;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.1s ease;
  }
  .sidebar-resizer:hover .resizer-chev,
  .sidebar-resizer.collapsed .resizer-chev {
    opacity: 1;
  }
  .sec-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 8px 6px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-secondary);
  }
  .sq-section {
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .sq-section.open {
    max-height: 42%;
  }
  .sq-toggle {
    width: 100%;
    background: none;
    border: none;
    cursor: pointer;
    justify-content: flex-start;
    gap: 5px;
  }
  .sq-toggle .chev {
    font-size: 11px;
    width: 12px;
    text-align: center;
    color: var(--text-muted);
  }
  .sq-badge {
    margin-left: auto;
    font-size: 9px;
    font-weight: 400;
    color: var(--text-muted);
  }
  .sq-body {
    flex: 1;
    overflow: hidden;
    border-top: 1px solid var(--border);
  }
  .schema-host {
    flex: 1;
    overflow: hidden;
  }
  .main {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .split-host {
    display: flex;
    height: 100%;
    overflow: hidden;
  }
  .split-pane {
    height: 100%;
    min-width: 0;
    overflow: hidden;
  }
  .split-resizer {
    position: relative;
    width: 5px;
    flex-shrink: 0;
    cursor: col-resize;
    background: var(--border);
  }
  .split-resizer:hover {
    background: var(--tool-sql-text);
  }
  .split-close {
    position: absolute;
    top: 6px;
    left: 50%;
    transform: translateX(-50%);
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    border: 1px solid var(--border-strong);
    background: var(--surface-1);
    color: var(--text-secondary);
    font-size: 11px;
    line-height: 1;
    cursor: pointer;
    z-index: 10;
  }
  .split-close:hover {
    background: var(--surface-3);
    color: var(--text-primary);
  }
  .drop-overlay {
    position: absolute;
    inset: 0;
    z-index: 40;
    display: flex;
    background: color-mix(in srgb, var(--surface-0) 55%, transparent);
  }
  .dz {
    flex: 1;
    margin: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 2px dashed var(--border-strong);
    border-radius: var(--radius);
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    pointer-events: none;
  }
  .dz.hot {
    border-color: var(--tool-sql-text);
    background: var(--tool-sql-tint);
    color: var(--tool-sql-text);
  }
  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }
  .empty-picker {
    width: 360px;
    max-height: 80%;
    display: flex;
    flex-direction: column;
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
  }
  .empty-hint {
    margin: 0;
    padding: 12px 14px 0;
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-1);
  }
  /* Run / Save / connection badge are the first thing you see in this
     bar — keep them the same fixed height and baseline so they read as
     one uniform control group instead of odd-sized, oddly-aligned bits. */
  .toolbar :global(.btn) {
    height: 26px;
    display: inline-flex;
    align-items: center;
    line-height: 1;
  }
  /* the save-to-file glyph reads as a tiny mark at the standard
     icon-btn size — size it up without growing the 26px button box. */
  .toolbar :global(.big-glyph) {
    font-size: 16px;
  }
  .tb-conn {
    height: 26px;
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    padding: 0 9px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--c, var(--conn-slate)) 22%, var(--surface-2));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c, var(--conn-slate)) 38%, transparent);
    font-size: 11px;
    line-height: 26px;
    color: color-mix(in srgb, var(--c, var(--text-secondary)) 65%, var(--text-primary));
    font-weight: 600;
  }
  .tb-tab {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tb-tz {
    height: 26px;
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    padding: 0 9px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    font-size: 10.5px;
    line-height: 26px;
    font-family: var(--font-mono);
    color: var(--text-muted);
    white-space: nowrap;
  }
  .tb-tz.muted {
    font-style: italic;
  }
  .workarea {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .varbar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    padding: 6px 8px;
    background: var(--tool-sql-tint);
    border-bottom: 1px solid var(--border);
  }
  .vb-label {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--tool-sql-text);
  }
  .vb-slot {
    display: flex;
    align-items: center;
    gap: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    padding: 2px 4px 2px 7px;
  }
  .vb-slot.missing {
    border-color: var(--warn);
  }
  .vb-name {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--tool-sql-text);
  }
  .vb-input {
    font: inherit;
    font-size: 11px;
    border: none;
    background: none;
    color: var(--text-primary);
    width: 120px;
    padding: 3px 4px;
    outline: none;
  }
  .pane {
    overflow: hidden;
  }
  .result-pane {
    border-top: 1px solid var(--border);
    background: var(--surface-2);
  }
  .splitter {
    height: 6px;
    background: var(--surface-1);
    cursor: row-resize;
    flex-shrink: 0;
  }
  .splitter:hover {
    background: var(--tool-sql-text);
  }
  .err {
    padding: 14px;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--danger);
    white-space: pre-wrap;
    overflow: auto;
    height: 100%;
  }
  .multi-strip {
    display: flex;
    gap: 3px;
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-1);
    overflow-x: auto;
  }
  .multi-tab {
    all: unset;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 4px;
    color: var(--text-secondary);
    white-space: nowrap;
  }
  .multi-tab:hover {
    background: var(--surface-3);
  }
  .multi-tab.on {
    background: var(--tool-sql-tint);
    color: var(--tool-sql-text);
  }
  .multi-tab.bad {
    color: var(--danger);
  }
  .pager {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-secondary);
  }
  .pg-info {
    font-family: var(--font-mono);
    padding: 0 4px;
  }
  .pg-hint {
    color: var(--text-muted);
  }
  .pg-ct {
    color: var(--text-muted);
    font-size: 10px;
  }
  .statusbar {
    padding: 4px 10px;
    font-size: 10.5px;
    color: var(--text-muted);
    border-top: 1px solid var(--border);
    background: var(--surface-1);
  }
  .s-err {
    color: var(--danger);
  }
</style>
