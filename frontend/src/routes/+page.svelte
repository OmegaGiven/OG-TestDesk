<script>
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { listen } from '@tauri-apps/api/event';
  import { sqlTabs, requestTabs, activeSqlTabId } from '../lib/stores.js';
  import { api } from '../lib/api.js';
  import TopNav from '../lib/components/TopNav.svelte';
  import Toasts from '../lib/components/Toasts.svelte';
  import DialogHost from '../lib/components/DialogHost.svelte';
  import SettingsModal from '../lib/components/SettingsModal.svelte';
  import ActivityModal from '../lib/components/ActivityModal.svelte';
  import WindowControls from '../lib/components/WindowControls.svelte';
  import ConnectionsMenu from '../lib/sql/ConnectionsMenu.svelte';
  import { appearance, applyAppearance } from '../lib/stores.js';
  import { ICONS } from '../lib/icons.js';

  const isMac =
    typeof navigator !== 'undefined' &&
    /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent || '');
  import SqlView from '../lib/sql/SqlView.svelte';
  import RequestsView from '../lib/requests/RequestsView.svelte';
  import InspectorView from '../lib/inspector/InspectorView.svelte';
  import {
    activeTool,
    theme,
    reloadConnections,
    reloadTabs,
    reloadRequests,
    reloadRequestTabs,
    reloadSavedQueries,
    reloadSavedCharts,
    sendToInspector,
    connMenuOpen,
    debugSnapshot,
    ensureGroupOrder,
    toast
  } from '../lib/stores.js';

  let settingsOpen = false;
  let settingsInitialTab = '';
  let activityOpen = false;
  function openSettings(tabId = '') {
    settingsInitialTab = tabId;
    settingsOpen = true;
  }
  const THEMES = ['system', 'light', 'dark'];
  function cycleTheme() {
    theme.update((t) => THEMES[(THEMES.indexOf(t) + 1) % THEMES.length]);
  }

  function onKey(e) {
    if ((e.metaKey || e.ctrlKey) && ['1', '2', '3'].includes(e.key)) {
      e.preventDefault();
      activeTool.set(['sql', 'requests', 'inspector'][+e.key - 1]);
    }
  }

  // Re-pull everything from the backend metadata store on launch.
  async function reloadEverything() {
    await reloadConnections();
    await Promise.all([reloadTabs(), reloadRequests(), reloadRequestTabs(), reloadSavedQueries(), reloadSavedCharts()]);
  }

  // Re-pull on window focus too — since the MCP server's "populate"
  // tools (save_query, save_request, add_connection) write to the
  // backend from outside the UI's own action flow and there's no push
  // channel from Rust, this is how something an AI just added actually
  // shows up without a manual reload. Deliberately NOT open SQL/request
  // tabs here: a tab has its own live, locally-authoritative state
  // (dirty edits, an in-flight close) and blindly overwriting it from
  // a backend snapshot on every focus repeatedly proved to resurrect
  // tabs that had just been closed, racing against their own delete /
  // debounced-save calls. `open_sql_tab` results are still visible via
  // Saved Queries / a manual reconnect; that's an acceptable trade for
  // not clobbering live tab state on every alt-tab.
  async function reloadOnFocus() {
    await reloadConnections();
    await Promise.all([reloadRequests(), reloadSavedQueries(), reloadSavedCharts()]);
  }

  // Genuinely frontend-only failures (an uncaught exception, a rejected
  // promise nothing awaited) — not the backend `Result::Err`s that
  // already land in the error log via every command's `err()` helper.
  // Without this, these just vanish into the devtools console the user
  // never opens.
  // Keep the backend's copy of "what does the app think is open" fresh
  // — debounced, since this store recomputes on nearly every tab/tool
  // action — so the get_app_state MCP tool isn't reading something
  // stale from minutes ago.
  let debugPushTimer;
  $: debugSnapshotJson = JSON.stringify($debugSnapshot, null, 2);
  $: if (debugSnapshotJson) {
    clearTimeout(debugPushTimer);
    debugPushTimer = setTimeout(() => {
      api.debugStateSet(debugSnapshotJson).catch(() => {});
    }, 400);
  }

  // Live push from MCP tools, straight into the window (see mcp.rs's
  // `ctx.app_handle.emit(...)` calls) — precise, additive updates
  // instead of the reload-everything approach that kept resurrecting
  // tabs. A tab an AI opens shows up in the top bar immediately,
  // without switching your focus to it or touching anything else.
  function mergeSqlTab(tab) {
    sqlTabs.update((tabs) =>
      tabs.some((t) => t.id === tab.id)
        ? tabs
        : [...tabs, { ...tab, dirty: false, result: null, error: null, running: false }]
    );
    ensureGroupOrder(tab.connection_id);
    toast(`AI opened a SQL tab: ${tab.title}`, 'info', 3500);
  }
  function mergeRequestTab(tab) {
    requestTabs.update((tabs) =>
      tabs.some((t) => t.id === tab.id)
        ? tabs
        : [...tabs, { ...tab, response: null, error: null, sending: false, dirty: false }]
    );
    toast(`AI opened a request tab: ${tab.title}`, 'info', 3500);
  }

  function onWindowError(e) {
    api.logClientError('window', e.message || String(e.error || e)).catch(() => {});
  }
  function onUnhandledRejection(e) {
    api.logClientError('promise', String(e.reason?.message || e.reason || e)).catch(() => {});
  }

  onMount(async () => {
    applyAppearance($appearance);
    window.addEventListener('error', onWindowError);
    window.addEventListener('unhandledrejection', onUnhandledRejection);
    await reloadEverything();
    window.addEventListener('focus', reloadOnFocus);
    listen('mcp:sql-tab-opened', (e) => mergeSqlTab(e.payload)).catch(() => {});
    listen('mcp:request-tab-opened', (e) => mergeRequestTab(e.payload)).catch(() => {});
    listen('mcp:saved-query-created', () => reloadSavedQueries()).catch(() => {});
    listen('mcp:saved-request-created', () => reloadRequests()).catch(() => {});
    listen('mcp:connection-created', () => reloadConnections()).catch(() => {});

    // Dev/demo helpers via query string (no effect in normal use):
    //   ?tool=requests|inspector   ?run  (auto-run the active SQL tab)
    const q = new URLSearchParams(location.search);
    if (q.has('theme')) theme.set(q.get('theme'));
    if (q.has('connmenu')) connMenuOpen.set(true);
    if (q.has('colortheme')) appearance.update((a) => ({ ...a, colorTheme: q.get('colortheme') }));
    if (q.has('settings')) settingsOpen = true;
    if (q.has('help')) openSettings('help');
    if (q.has('activity')) activityOpen = true;
    if (q.has('tool')) activeTool.set(q.get('tool'));
    if (q.has('sqltab')) {
      const t = get(sqlTabs).find((x) => x.title === q.get('sqltab'));
      if (t) activeSqlTabId.set(t.id);
    }
    if (q.has('run')) {
      const clickWhenReady = async (sel, tries = 40) => {
        for (let i = 0; i < tries; i++) {
          const el = document.querySelector(sel);
          if (el) return el.click();
          await new Promise((r) => setTimeout(r, 100));
        }
      };
      await clickWhenReady('.toolbar .btn.primary');
    }

    // ?req / ?send are handled inside RequestsView itself.
    if (q.has('inspect')) {
      sendToInspector('requests', 'POST {{baseUrl}}/orders — 201 Created', {
        order: {
          id: 1042,
          status: 'shipped',
          total: 284.97,
          customer: { name: 'Ava Ng', city: 'Dallas', vip: true },
          items: [
            { sku: 'SKU-1007', name: 'Keyboard Model 7', qty: 2, price: 79.99 },
            { sku: 'SKU-1019', name: 'Monitor Model 19', qty: 1, price: 124.99 }
          ]
        },
        meta: { duration_ms: 128, cached: false, region: 'us-east' }
      });
    }
    if (q.has('inspectarray')) {
      sendToInspector('sql', 'Orders by status (5 rows)', [
        { status: 'delivered', n: 21, revenue: 18422.55 },
        { status: 'paid', n: 17, revenue: 14201.1 },
        { status: 'shipped', n: 15, revenue: 12980.4 },
        { status: 'pending', n: 12, revenue: 9004.22 },
        { status: 'refunded', n: 10, revenue: 7411.98 }
      ]);
    }
  });
</script>

<svelte:head><title>OG TestDesk</title></svelte:head>
<svelte:window on:keydown={onKey} />

<div class="app">
 <div class="shell">
  <div class="chrome" class:mac={isMac} data-tauri-drag-region>
    {#if isMac}<div class="tl-space" data-tauri-drag-region></div>{/if}
    <TopNav />
    <button class="chrome-btn" on:click={() => (activityOpen = true)} title="History & schedules">{@html ICONS.history.svg}</button>
    <button class="chrome-btn" on:click={() => openSettings()} title="Settings">{@html ICONS.settings.svg}</button>
    <button class="chrome-btn" on:click={cycleTheme} title="Theme: {$theme}">
      {@html $theme === 'dark' ? ICONS.themeDark.svg : $theme === 'light' ? ICONS.themeLight.svg : ICONS.themeSystem.svg}
    </button>
    {#if !isMac}<WindowControls />{/if}
  </div>

  <main>
    <div class="view" class:show={$activeTool === 'sql'}><SqlView /></div>
    <div class="view" class:show={$activeTool === 'requests'}><RequestsView /></div>
    <div class="view" class:show={$activeTool === 'inspector'}><InspectorView /></div>
  </main>
 </div>
</div>

{#if settingsOpen}
  <SettingsModal initialTab={settingsInitialTab} on:close={() => (settingsOpen = false)} />
{/if}
{#if activityOpen}
  <ActivityModal on:close={() => (activityOpen = false)} />
{/if}

<ConnectionsMenu />

<Toasts />
<DialogHost />

<style>
  .app {
    height: 100vh;
    overflow: hidden;
    padding: var(--app-gutter, 0);
    background: var(--surface-0);
  }
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    border-radius: var(--radius);
    border: 1px solid var(--border);
  }
  :global(:root:not([data-gutter])) .shell {
    border-radius: 0;
    border: none;
  }
  .tl-space {
    /* trafficLightPosition in tauri.conf.json is x:18 with a ~52px-wide
       dot cluster, so 70px is the traffic lights' exact right edge —
       zero breathing room before the "+" button. A bit of headroom. */
    width: 82px;
    flex-shrink: 0;
    -webkit-app-region: drag;
  }
  .chrome {
    display: flex;
    align-items: stretch;
    -webkit-app-region: drag;
    background: var(--surface-1);
    border-bottom: 1px solid var(--border);
  }
  .chrome {
    min-width: 0;
    overflow: hidden;
  }
  .chrome :global(.topnav) {
    flex: 1 1 0;
    width: 0;
    min-width: 0;
    border-bottom: none;
  }
  .chrome-btn {
    border: none;
    border-left: 1px solid var(--border);
    background: var(--surface-1);
    color: var(--text-secondary);
    font-size: 19px;
    line-height: 1;
    min-width: 40px;
    padding: 0 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    -webkit-app-region: no-drag;
  }
  .chrome-btn:hover {
    background: var(--surface-3);
    color: var(--text-primary);
  }
  main {
    flex: 1;
    position: relative;
    overflow: hidden;
  }
  .view {
    position: absolute;
    inset: 0;
    display: none;
  }
  .view.show {
    display: block;
  }
</style>
