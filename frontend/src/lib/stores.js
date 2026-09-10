import { writable, derived, get } from 'svelte/store';
import { api } from './api.js';

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
}
theme.subscribe((t) => {
  if (typeof document !== 'undefined') applyTheme(t);
});

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
    const active = rows.find((t) => t.is_active);
    if (active) activeSqlTabId.set(active.id);
    else if (rows[0]) activeSqlTabId.set(rows[0].id);
  } catch (e) {
    toastError(e);
  }
}

export async function newSqlTab(connectionId, sql = '') {
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
    toastError(e);
  }
  const tabs = get(sqlTabs).filter((t) => t.id !== id);
  sqlTabs.set(tabs);
  if (get(activeSqlTabId) === id) {
    activeSqlTabId.set(tabs[tabs.length - 1]?.id || null);
  }
}

/* ------------------------------------------------------------- Inspector */

// Payload handed to the Inspector from SQL results / HTTP responses.
export const inspectorPayload = writable(null); // { source, label, json }

export function sendToInspector(source, label, json) {
  inspectorPayload.set({ source, label, json, at: Date.now() });
  activeTool.set('inspector');
}

/* --------------------------------------------------------- Requests state */

export const requestCollections = writable([]);
export const savedRequests = writable([]);
export const environments = writable([]);

export async function reloadRequests() {
  try {
    const [cols, reqs, envs] = await Promise.all([
      api.collectionsList(),
      api.savedRequestsList(),
      api.environmentsList()
    ]);
    requestCollections.set(cols);
    savedRequests.set(reqs);
    environments.set(envs);
  } catch (e) {
    toastError(e);
  }
}

export const activeEnvironment = derived(environments, ($e) => $e.find((x) => x.is_active) || null);
