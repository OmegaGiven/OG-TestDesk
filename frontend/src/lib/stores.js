import { writable, derived, get } from 'svelte/store';
import { api } from './api.js';
import { applyColorTheme, fontStack, FONT_SANS, FONT_MONO } from './themes.js';

/* ------------------------------------------------------------ appearance */

const APPEARANCE_KEY = 'ogtestdesk.appearance';
export const APPEARANCE_DEFAULT = {
  radius: 8,
  gutter: 0,
  density: 1,
  navPad: 6,
  navGap: 5,
  fontSans: 'system',
  fontMono: 'system',
  fontScale: 1,
  colorTheme: 'default',
  customTheme: { light: {}, dark: {} },
  pageSize: 500
};

function initialAppearance() {
  try {
    return { ...APPEARANCE_DEFAULT, ...JSON.parse(localStorage.getItem(APPEARANCE_KEY) || '{}') };
  } catch {
    return { ...APPEARANCE_DEFAULT };
  }
}
export const appearance = writable(initialAppearance());

/** 'light' | 'dark' — from the data-theme attribute, else the OS preference. */
export function effectiveMode() {
  try {
    const dt = document.documentElement.getAttribute('data-theme');
    if (dt === 'light' || dt === 'dark') return dt;
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  } catch {
    return 'light';
  }
}

export function applyAppearance(a) {
  const root = document.documentElement;
  root.style.setProperty('--radius', `${a.radius}px`);
  root.style.setProperty('--radius-sm', `${Math.max(2, a.radius - 3)}px`);
  root.style.setProperty('--app-gutter', `${a.gutter}px`);
  root.style.setProperty('--density', String(a.density));
  root.style.setProperty('--nav-pad', `${a.navPad ?? 6}px`);
  root.style.setProperty('--nav-gap', `${a.navGap ?? 5}px`);
  root.style.setProperty('--font-sans', fontStack(FONT_SANS, a.fontSans));
  root.style.setProperty('--font-mono', fontStack(FONT_MONO, a.fontMono));
  // Whole-UI scale (like browser zoom) — covers fonts + spacing everywhere.
  root.style.zoom = String(a.fontScale ?? 1);
  if (a.gutter > 0) root.dataset.gutter = '1';
  else delete root.dataset.gutter;
  applyColorTheme(a.colorTheme || 'default', effectiveMode(), a.customTheme);
  try {
    localStorage.setItem(APPEARANCE_KEY, JSON.stringify(a));
  } catch {}
}
appearance.subscribe((a) => {
  if (typeof document !== 'undefined') applyAppearance(a);
});

/* ------------------------------------------------------------------ theme */

const THEME_KEY = 'ogtestdesk.theme';
function initialTheme() {
  try {
    return localStorage.getItem(THEME_KEY) || 'system';
  } catch {
    return 'system';
  }
}
export const theme = writable(initialTheme());

export function applyTheme(t) {
  const root = document.documentElement;
  if (t === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', t);
  try {
    localStorage.setItem(THEME_KEY, t);
  } catch {}
  // colour theme depends on light/dark, re-apply
  if (typeof document !== 'undefined') applyColorTheme(get(appearance).colorTheme, effectiveMode(), get(appearance).customTheme);
}
theme.subscribe((t) => {
  if (typeof document !== 'undefined') applyTheme(t);
});
if (typeof window !== 'undefined' && window.matchMedia) {
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener?.('change', () => {
    if (get(theme) === 'system') applyColorTheme(get(appearance).colorTheme, effectiveMode(), get(appearance).customTheme);
  });
}

/* ------------------------------------------------------------------ toasts */

export const toasts = writable([]);
let toastId = 0;
export function toast(message, kind = 'info', ttl = 4000) {
  const id = ++toastId;
  toasts.update((t) => [...t, { id, message, kind }]);
  if (ttl) setTimeout(() => dismissToast(id), ttl);
  return id;
}
export function dismissToast(id) {
  toasts.update((t) => t.filter((x) => x.id !== id));
}
export function toastError(e) {
  toast(typeof e === 'string' ? e : e?.message || String(e), 'error', 7000);
}

/* -------------------------------------------------------------- navigation */

export const activeTool = writable('sql'); // 'sql' | 'requests' | 'inspector'

/* ------------------------------------------------------------- connections */

export const connections = writable([]);
export const schemasByConn = writable({}); // connId -> Schema[]

export async function reloadConnections() {
  try {
    connections.set(await api.connectionsList());
  } catch (e) {
    toastError(e);
  }
}

export async function loadSchemas(conn, force = false) {
  if (!force) {
    const cache = get(schemasByConn)[conn.id];
    if (cache) return cache;
  }
  const schemas = await api.schemasList(conn);
  schemasByConn.update((m) => ({ ...m, [conn.id]: schemas }));
  return schemas;
}

/* ------------------------------------------------------- connections menu */

export const connMenuOpen = writable(false);

/* --------------------------------------------------- SQL split editor */
// The SQL view can show a second query tab side by side with the active
// one. `splitTabId` is that second tab's id, or null when there's no
// split — the primary/left pane is always just $activeSqlTabId. Not
// persisted; a reload starts back at a single pane.
export const splitTabId = writable(null);

export function openSplit(tabId) {
  splitTabId.set(tabId);
}
export function closeSplit() {
  splitTabId.set(null);
}

/** Set (dragstart) / cleared (dragend or drop) by the top bar while a SQL
 * tab is being dragged, purely so the SQL editor can show a drop-zone
 * highlight for "drop here to split left/right" while the drag is live. */
export const draggingSqlTab = writable(null);

/** Whether the shared Inspector "tab" is currently open on the top bar.
 * Closed like any other tab; reopened via the + menu's Inspector row. */
const INSPECTOR_OPEN_KEY = 'ogtestdesk.inspectorOpen';
export const inspectorOpen = writable(
  JSON.parse(localStorage.getItem(INSPECTOR_OPEN_KEY) ?? 'false')
);
inspectorOpen.subscribe((v) => {
  try {
    localStorage.setItem(INSPECTOR_OPEN_KEY, JSON.stringify(v));
  } catch {}
});

/* ---------------------------------------------------- top-bar tab groups */
// The top bar shows one colored, draggable group per DB connection.
// Request tabs and the single Inspector "tab" have no group of their
// own — they float independently (a request's method badge already
// marks what it is) — but either can be dragged into a connection's
// group purely for organization. That override, and the left-to-right
// order of the groups themselves, are cosmetic and saved locally (not
// synced to the backend).
const GROUP_ORDER_KEY = 'ogtestdesk.groupOrder';
const TAB_GROUP_KEY = 'ogtestdesk.tabGroupOverride';
/** Stable synthetic id for the one shared Inspector view, so it can be
 * dragged into a connection group like any other tab. */
export const INSPECTOR_TAB_ID = '__inspector__';

function loadJson(key, fallback) {
  try {
    const v = JSON.parse(localStorage.getItem(key));
    return v ?? fallback;
  } catch {
    return fallback;
  }
}
function saveJson(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {}
}

export const groupOrder = writable(loadJson(GROUP_ORDER_KEY, []));
groupOrder.subscribe((v) => saveJson(GROUP_ORDER_KEY, v));

/** Push a group key to the end of the order if it isn't already tracked. */
export function ensureGroupOrder(key) {
  groupOrder.update((order) => (order.includes(key) ? order : [...order, key]));
}

export function reorderGroups(draggedKey, targetKey) {
  groupOrder.update((order) => {
    const next = order.filter((k) => k !== draggedKey);
    const at = next.indexOf(targetKey);
    if (at < 0) return order;
    next.splice(at, 0, draggedKey);
    return next;
  });
}

export const tabGroupOverride = writable(loadJson(TAB_GROUP_KEY, {}));
tabGroupOverride.subscribe((v) => saveJson(TAB_GROUP_KEY, v));

/** Only a SQL tab has a natural group (its own connection) — request
 * tabs and the Inspector float loose until dragged somewhere. */
export function naturalGroup(kind, tab) {
  return kind === 'sql' ? tab.connection_id : null;
}
export function groupOf(kind, tab, overrides) {
  return overrides[tab.id] ?? naturalGroup(kind, tab);
}
export function moveTabToGroup(kind, tab, groupKey) {
  tabGroupOverride.update((m) => {
    const next = { ...m };
    if (groupKey == null || groupKey === naturalGroup(kind, tab)) delete next[tab.id];
    else next[tab.id] = groupKey;
    return next;
  });
  if (groupKey != null) ensureGroupOrder(groupKey);
}

/* ------------------------------------------------------- tab left-right order */
// Independent of grouping: a flat drag-to-reorder position for every SQL
// tab, request tab, and the Inspector, whether it's sitting loose or
// inside a connection group. Cosmetic/local only, like the group order.
const TAB_ORDER_KEY = 'ogtestdesk.tabOrder';

export const tabOrder = writable(loadJson(TAB_ORDER_KEY, []));
tabOrder.subscribe((v) => saveJson(TAB_ORDER_KEY, v));

export function tabKey(kind, tab) {
  return `${kind}:${tab.id}`;
}

/** Push a tab's key to the end of the order if it isn't already tracked. */
export function ensureTabOrder(kind, tab) {
  const key = tabKey(kind, tab);
  tabOrder.update((order) => (order.includes(key) ? order : [...order, key]));
}

/** Drag `draggedTab` to sit just before `targetTab` in the flat order. */
export function reorderTab(draggedKind, draggedTab, targetKind, targetTab) {
  const draggedKey = tabKey(draggedKind, draggedTab);
  const targetKey = tabKey(targetKind, targetTab);
  if (draggedKey === targetKey) return;
  tabOrder.update((order) => {
    const next = order.filter((k) => k !== draggedKey);
    let at = next.indexOf(targetKey);
    if (at < 0) at = next.length;
    next.splice(at, 0, draggedKey);
    return next;
  });
}

export function tabOrderIndex(order, kind, tab) {
  const i = order.indexOf(tabKey(kind, tab));
  return i < 0 ? Infinity : i;
}

/* ----------------------------------------------------------- saved queries */

export const savedQueries = writable([]);
export const savedQueryFolders = writable([]);

export async function reloadSavedQueries() {
  try {
    const [q, f] = await Promise.all([api.savedQueriesList(), api.savedQueryFoldersList()]);
    savedQueries.set(q);
    savedQueryFolders.set(f);
  } catch (e) {
    toastError(e);
  }
}

/* --------------------------------------------------------------- SQL tabs */

export const sqlTabs = writable([]); // {id, connection_id, title, sql_text, position, is_active, dirty, result, error, running}
export const activeSqlTabId = writable(null);

export const activeSqlTab = derived([sqlTabs, activeSqlTabId], ([$tabs, $id]) =>
  $tabs.find((t) => t.id === $id)
);

export async function reloadTabs() {
  try {
    const rows = await api.tabsListAll();
    sqlTabs.set(rows.map((t) => ({ ...t, dirty: false, result: null, error: null, running: false })));
    for (const t of rows) ensureGroupOrder(t.connection_id);
    const active = rows.find((t) => t.is_active);
    if (active) activeSqlTabId.set(active.id);
    else if (rows[0]) activeSqlTabId.set(rows[0].id);
  } catch (e) {
    toastError(e);
  }
}

export async function newSqlTab(connectionId, sql = '') {
  ensureGroupOrder(connectionId);
  const tabs = get(sqlTabs).filter((t) => t.connection_id === connectionId);
  const position = tabs.length;
  let tab = {
    id: '',
    connection_id: connectionId,
    title: `Query ${position + 1}`,
    sql_text: sql,
    position,
    is_active: true
  };
  tab = await api.tabSave(tab);
  const local = { ...tab, dirty: false, result: null, error: null, running: false };
  sqlTabs.update((t) => [...t, local]);
  activeSqlTabId.set(tab.id);
  return local;
}

let saveTimers = {};
export function touchSqlTab(id, patch) {
  sqlTabs.update((tabs) => tabs.map((t) => (t.id === id ? { ...t, ...patch } : t)));
}
export function persistSqlTab(id, immediate = false) {
  clearTimeout(saveTimers[id]);
  const doSave = async () => {
    const t = get(sqlTabs).find((x) => x.id === id);
    if (!t) return;
    try {
      await api.tabSave({
        id: t.id,
        connection_id: t.connection_id,
        title: t.title,
        sql_text: t.sql_text,
        position: t.position,
        is_active: get(activeSqlTabId) === t.id
      });
      touchSqlTab(id, { dirty: false });
    } catch (e) {
      toastError(e);
    }
  };
  if (immediate) doSave();
  else saveTimers[id] = setTimeout(doSave, 600);
}

export async function closeSqlTab(id) {
  clearTimeout(saveTimers[id]);
  try {
    await api.tabDelete(id);
  } catch (e) {
    // Don't drop it from the UI if the backend didn't actually delete it —
    // otherwise it silently reappears the next time anything refetches
    // tabs from the backend (e.g. the window regaining focus), which
    // looks like the close never "took" at all.
    toastError(e);
    return;
  }
  const tabs = get(sqlTabs).filter((t) => t.id !== id);
  sqlTabs.set(tabs);
  if (get(activeSqlTabId) === id) {
    activeSqlTabId.set(tabs[tabs.length - 1]?.id || null);
  }
  if (get(splitTabId) === id) {
    splitTabId.set(null);
  }
}

/* ------------------------------------------------------------- Inspector */

// Payload handed to the Inspector from SQL results / HTTP responses.
// `meta` (optional) carries what's needed to re-run the source query for
// a saved chart later — { connectionId, sql } for SQL results.
export const inspectorPayload = writable(null); // { source, label, json, meta }

export function sendToInspector(source, label, json, meta = null) {
  inspectorPayload.set({ source, label, json, meta, at: Date.now() });
  activeTool.set('inspector');
}

/* -------------------------------------------------------- saved charts */

export const savedCharts = writable([]);

export async function reloadSavedCharts() {
  try {
    savedCharts.set(await api.savedChartsList());
  } catch (e) {
    toastError(e);
  }
}

/* --------------------------------------------------------- Requests state */

export const requestCollections = writable([]);
export const savedRequests = writable([]);
export const environments = writable([]);
export const requestGlobals = writable({}); // plain vars, applied under active env

export async function reloadRequests() {
  try {
    const [cols, reqs, envs, globalsRaw] = await Promise.all([
      api.collectionsList(),
      api.savedRequestsList(),
      api.environmentsList(),
      api.globalsGet()
    ]);
    requestCollections.set(cols);
    savedRequests.set(reqs);
    environments.set(envs);
    try {
      requestGlobals.set(globalsRaw ? JSON.parse(globalsRaw) : {});
    } catch {
      requestGlobals.set({});
    }
  } catch (e) {
    toastError(e);
  }
}

export async function saveGlobals(obj) {
  requestGlobals.set(obj);
  try {
    await api.globalsSet(JSON.stringify(obj));
  } catch (e) {
    toastError(e);
  }
}

export const activeEnvironment = derived(environments, ($e) => $e.find((x) => x.is_active) || null);

/* --------------------------------------------------------- request tabs */

export const requestTabs = writable([]);
export const activeRequestTabId = writable(null);
export const activeRequestTab = derived([requestTabs, activeRequestTabId], ([$t, $id]) =>
  $t.find((x) => x.id === $id)
);

export async function reloadRequestTabs() {
  try {
    let rows = await api.requestTabsList();
    requestTabs.set(
      rows.map((t) => ({ ...t, response: null, error: null, sending: false, dirty: false }))
    );
    const active = rows.find((t) => t.is_active);
    activeRequestTabId.set(active?.id ?? rows[0]?.id ?? null);
    if (rows.length === 0) await newRequestTab();
  } catch (e) {
    toastError(e);
  }
}

export async function newRequestTab(seed = {}) {
  const tabs = get(requestTabs);
  let tab = {
    id: '',
    saved_request_id: seed.saved_request_id ?? null,
    title: seed.title || 'Untitled',
    method: seed.method || 'GET',
    url: seed.url || '',
    headers_json: seed.headers_json || '{}',
    body: seed.body ?? null,
    position: tabs.length,
    is_active: true
  };
  tab = await api.requestTabSave(tab);
  const local = { ...tab, response: null, error: null, sending: false, dirty: false };
  requestTabs.update((t) => [...t, local]);
  activeRequestTabId.set(tab.id);
  return local;
}

export function touchRequestTab(id, patch) {
  requestTabs.update((tabs) => tabs.map((t) => (t.id === id ? { ...t, ...patch } : t)));
}

let rtTimers = {};
export function persistRequestTab(id, immediate = false) {
  clearTimeout(rtTimers[id]);
  const save = async () => {
    const t = get(requestTabs).find((x) => x.id === id);
    if (!t) return;
    try {
      await api.requestTabSave({
        id: t.id,
        saved_request_id: t.saved_request_id,
        title: t.title,
        method: t.method,
        url: t.url,
        headers_json: t.headers_json,
        body: t.body,
        position: t.position,
        is_active: get(activeRequestTabId) === t.id
      });
      touchRequestTab(id, { dirty: false });
    } catch (e) {
      toastError(e);
    }
  };
  if (immediate) save();
  else rtTimers[id] = setTimeout(save, 600);
}

export async function closeRequestTab(id) {
  clearTimeout(rtTimers[id]);
  try {
    await api.requestTabDelete(id);
  } catch (e) {
    // Same reasoning as closeSqlTab: if the backend delete failed, keep
    // it in the UI too, or it silently comes back on the next refetch
    // (e.g. the window regaining focus) looking like the close undid
    // itself.
    toastError(e);
    return;
  }
  const tabs = get(requestTabs).filter((t) => t.id !== id);
  requestTabs.set(tabs);
  if (get(activeRequestTabId) === id) {
    activeRequestTabId.set(tabs[tabs.length - 1]?.id ?? null);
  }
}

/* ------------------------------------------------------- history recall */

// Set by the Activity modal; consumed by SqlView / RequestsView.
// `resolved` is the on-demand-fetched result/response JSON string, or null.
export const historyLoad = writable(null); // { kind, entry, resolved, at }

export function loadFromHistory(kind, entry, resolved = null) {
  historyLoad.set({ kind, entry, resolved, at: Date.now() });
  activeTool.set(kind === 'sql' ? 'sql' : 'requests');
}

