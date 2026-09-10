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
  colorTheme: 'default'
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
  applyColorTheme(a.colorTheme || 'default', effectiveMode());
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
  if (typeof document !== 'undefined') applyColorTheme(get(appearance).colorTheme, effectiveMode());
}
theme.subscribe((t) => {
  if (typeof document !== 'undefined') applyTheme(t);
});
if (typeof window !== 'undefined' && window.matchMedia) {
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener?.('change', () => {
    if (get(theme) === 'system') applyColorTheme(get(appearance).colorTheme, effectiveMode());
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
    toastError(e);
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

