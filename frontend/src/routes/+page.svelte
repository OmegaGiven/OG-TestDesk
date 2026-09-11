<script>
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { sqlTabs, activeSqlTabId } from '../lib/stores.js';
  import { api } from '../lib/api.js';
  import TopNav from '../lib/components/TopNav.svelte';
  import Toasts from '../lib/components/Toasts.svelte';
  import SettingsModal from '../lib/components/SettingsModal.svelte';
  import HelpModal from '../lib/components/HelpModal.svelte';
  import ActivityModal from '../lib/components/ActivityModal.svelte';
  import WindowControls from '../lib/components/WindowControls.svelte';
  import ConnectionsMenu from '../lib/sql/ConnectionsMenu.svelte';
  import { appearance, applyAppearance } from '../lib/stores.js';

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
    sendToInspector
  } from '../lib/stores.js';

  let settingsOpen = false;
  let helpOpen = false;
  let activityOpen = false;
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

  onMount(async () => {
    applyAppearance($appearance);
    await reloadConnections();
    await Promise.all([
      reloadTabs(),
      reloadRequests(),
      reloadRequestTabs(),
      reloadSavedQueries()
    ]);

    // Dev/demo helpers via query string (no effect in normal use):
    //   ?tool=requests|inspector   ?run  (auto-run the active SQL tab)
    const q = new URLSearchParams(location.search);
    if (q.has('theme')) theme.set(q.get('theme'));
    if (q.has('connmenu')) (await import('../lib/stores.js')).connMenuOpen.set(true);
    if (q.has('colortheme')) appearance.update((a) => ({ ...a, colorTheme: q.get('colortheme') }));
    if (q.has('settings')) settingsOpen = true;
    if (q.has('help')) helpOpen = true;
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
  });
</script>

<svelte:head><title>OG TestDesk</title></svelte:head>
<svelte:window on:keydown={onKey} />

<div class="app">
 <div class="shell">
  <div class="chrome" class:mac={isMac}>
    {#if isMac}<div class="tl-space"></div>{/if}
    <TopNav />
    <button class="chrome-btn" on:click={() => (activityOpen = true)} title="History & schedules">⏱</button>
    <button class="chrome-btn" on:click={() => (helpOpen = true)} title="Help">?</button>
    <button class="chrome-btn" on:click={() => (settingsOpen = true)} title="Settings">⚙</button>
    <button class="chrome-btn" on:click={cycleTheme} title="Theme: {$theme}">
      {$theme === 'dark' ? '☾' : $theme === 'light' ? '☀' : '◐'}
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
  <SettingsModal on:close={() => (settingsOpen = false)} />
{/if}
{#if helpOpen}
  <HelpModal on:close={() => (helpOpen = false)} />
{/if}
{#if activityOpen}
  <ActivityModal on:close={() => (activityOpen = false)} />
{/if}

<ConnectionsMenu />

<Toasts />

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
    width: 70px;
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
