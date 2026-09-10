<script>
  import { onMount } from 'svelte';
  import TopNav from '../lib/components/TopNav.svelte';
  import Toasts from '../lib/components/Toasts.svelte';
  import SqlView from '../lib/sql/SqlView.svelte';
  import RequestsView from '../lib/requests/RequestsView.svelte';
  import InspectorView from '../lib/inspector/InspectorView.svelte';
  import {
    activeTool,
    theme,
    reloadConnections,
    reloadTabs,
    reloadRequests
  } from '../lib/stores.js';

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
  });
</script>

<svelte:head><title>OG TestDesk</title></svelte:head>
<svelte:window on:keydown={onKey} />

<div class="app">
  <div class="chrome">
    <TopNav />
    <button class="theme" on:click={cycleTheme} title="Theme: {$theme}">
      {$theme === 'dark' ? '☾' : $theme === 'light' ? '☀' : '◐'}
    </button>
  </div>

  <main>
    <div class="view" class:show={$activeTool === 'sql'}><SqlView /></div>
    <div class="view" class:show={$activeTool === 'requests'}><RequestsView /></div>
    <div class="view" class:show={$activeTool === 'inspector'}><InspectorView /></div>
  </main>
</div>

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
  .theme {
    border: none;
    border-left: 1px solid var(--border);
    background: var(--surface-1);
    color: var(--text-secondary);
    font-size: 14px;
    width: 40px;
    cursor: pointer;
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
