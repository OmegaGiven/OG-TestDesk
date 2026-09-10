<script>
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { sqlTabs, activeSqlTabId } from '../lib/stores.js';
  import { api } from '../lib/api.js';
  import TopNav from '../lib/components/TopNav.svelte';
  import Toasts from '../lib/components/Toasts.svelte';
  import SettingsModal from '../lib/components/SettingsModal.svelte';
  import SqlView from '../lib/sql/SqlView.svelte';
  import RequestsView from '../lib/requests/RequestsView.svelte';
  import InspectorView from '../lib/inspector/InspectorView.svelte';
  import {
    activeTool,
    theme,
    reloadConnections,
    reloadTabs,
    reloadRequests,
    sendToInspector
  } from '../lib/stores.js';

  let settingsOpen = false;
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
    await reloadConnections();
    await Promise.all([reloadTabs(), reloadRequests()]);

    // Dev/demo helpers via query string (no effect in normal use):
    //   ?tool=requests|inspector   ?run  (auto-run the active SQL tab)
    const q = new URLSearchParams(location.search);
    if (q.has('settings')) settingsOpen = true;
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

    const clickText = async (sel, text, tries = 40) => {
      for (let i = 0; i < tries; i++) {
        const el = [...document.querySelectorAll(sel)].find((e) => e.textContent.includes(text));
        if (el) return el.click();
        await new Promise((r) => setTimeout(r, 100));
      }
    };
    if (q.has('req')) {
      await clickText('.ri-main', q.get('req'));
      if (q.has('send')) {
        await new Promise((r) => setTimeout(r, 300));
        document.querySelector('.urlbar .send')?.click();
      }
    }
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
      await new Promise((r) => setTimeout(r, 250));
      await clickText('.toolbar .btn', 'Expand all');
    }
  });
</script>

<svelte:head><title>OG TestDesk</title></svelte:head>
<svelte:window on:keydown={onKey} />

<div class="app">
  <div class="chrome">
    <TopNav />
    <button class="chrome-btn" on:click={() => (settingsOpen = true)} title="Settings">⚙</button>
    <button class="chrome-btn" on:click={cycleTheme} title="Theme: {$theme}">
      {$theme === 'dark' ? '☾' : $theme === 'light' ? '☀' : '◐'}
    </button>
  </div>

  <main>
    <div class="view" class:show={$activeTool === 'sql'}><SqlView /></div>
    <div class="view" class:show={$activeTool === 'requests'}><RequestsView /></div>
    <div class="view" class:show={$activeTool === 'inspector'}><InspectorView /></div>
  </main>
</div>

{#if settingsOpen}
  <SettingsModal on:close={() => (settingsOpen = false)} />
{/if}

<Toasts />

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }
  .chrome {
    display: flex;
    align-items: stretch;
    background: var(--surface-1);
    border-bottom: 1px solid var(--border);
  }
  .chrome :global(.topnav) {
    flex: 1;
    border-bottom: none;
  }
  .chrome-btn {
    border: none;
    border-left: 1px solid var(--border);
    background: var(--surface-1);
    color: var(--text-secondary);
    font-size: 14px;
    width: 38px;
    cursor: pointer;
  }
  .chrome-btn:hover {
    background: var(--surface-3);
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
