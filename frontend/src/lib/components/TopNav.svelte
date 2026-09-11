<script>
  import { onMount, tick } from 'svelte';
  import {
    connections,
    sqlTabs,
    activeSqlTabId,
    activeTool,
    newSqlTab,
    closeSqlTab,
    persistSqlTab,
    requestTabs,
    activeRequestTabId,
    newRequestTab,
    closeRequestTab,
    persistRequestTab,
    connMenuOpen
  } from '../stores.js';
  import { api } from '../api.js';
  import { ICONS } from '../icons.js';

  $: tabsByConn = (connId) =>
    $sqlTabs.filter((t) => t.connection_id === connId).sort((a, b) => a.position - b.position);

  function selectTab(id) {
    activeTool.set('sql');
    activeSqlTabId.set(id);
    persistSqlTab(id, true);
  }
  async function addTab(connId) {
    activeTool.set('sql');
    try {
      await newSqlTab(connId);
    } catch (e) {}
  }
  function close(id, e) {
    e.stopPropagation();
    closeSqlTab(id);
  }

  function selectReqTab(id) {
    activeTool.set('requests');
    activeRequestTabId.set(id);
    persistRequestTab(id, true);
  }
  async function addReqTab() {
    activeTool.set('requests');
    try {
      await newRequestTab();
    } catch (e) {}
  }
  function closeReq(id, e) {
    e.stopPropagation();
    closeRequestTab(id);
  }

  // ---- horizontal scroll without a scrollbar
  let scroller;
  let overflowing = false;
  let canLeft = false;
  let canRight = false;

  function refresh() {
    if (!scroller) return;
    const max = scroller.scrollWidth - scroller.clientWidth;
    overflowing = max > 2;
    canLeft = scroller.scrollLeft > 1;
    canRight = scroller.scrollLeft < max - 1;
  }
  function nudge(dir) {
    scroller?.scrollBy({ left: dir * Math.max(160, scroller.clientWidth * 0.6), behavior: 'smooth' });
  }
  function onWheel(e) {
    if (!scroller) return;
    // let a plain vertical wheel scroll the bar sideways
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      scroller.scrollLeft += e.deltaY;
      e.preventDefault();
    }
  }

  onMount(() => {
    refresh();
    const ro = new ResizeObserver(refresh);
    if (scroller) {
      ro.observe(scroller);
      for (const child of scroller.children) ro.observe(child);
    }
    window.addEventListener('resize', refresh);
    // layout / font settling
    const timers = [60, 250, 700].map((t) => setTimeout(refresh, t));
    return () => {
      ro.disconnect();
      window.removeEventListener('resize', refresh);
      timers.forEach(clearTimeout);
    };
  });

  // re-check when the tab set changes
  $: if ($sqlTabs || $requestTabs || $connections) tick().then(refresh);
</script>

<nav class="topnav">
  <button
    class="edge left"
    class:show={overflowing}
    disabled={!canLeft}
    tabindex="-1"
    title="Scroll left"
    on:click={() => nudge(-1)}>‹</button
  >

  <div class="scroller" bind:this={scroller} on:scroll={refresh} on:wheel={onWheel}>
    <!-- SQL tool: connection groups with tabs -->
    <div
      class="tool"
      class:active={$activeTool === 'sql'}
      style="--tint: var(--tool-sql-tint); --tint-text: var(--tool-sql-text);"
    >
      <button
        class="tool-label"
        title="Connections"
        on:click={() => {
          activeTool.set('sql');
          connMenuOpen.update((v) => !v);
        }}
      >
        SQL <span class="caret">▾</span>
      </button>
      {#each $connections as conn (conn.id)}
        {#if tabsByConn(conn.id).length}
          <div class="conn" style="--c: {conn.color || 'var(--conn-slate)'}">
            <button class="conn-label" on:click={() => connMenuOpen.set(true)} title="Switch / manage">
              {conn.nickname}
            </button>
            {#each tabsByConn(conn.id) as tab (tab.id)}
              <button
                class="tab"
                class:active={$activeSqlTabId === tab.id && $activeTool === 'sql'}
                on:click={() => selectTab(tab.id)}
                title={tab.title}
              >
                {tab.dirty ? '•' : ''}{tab.title}
                <span class="x" on:click={(e) => close(tab.id, e)} role="button" tabindex="-1">{ICONS.closeTab.glyph}</span>
              </button>
            {/each}
            <button class="add" title="New query" on:click={() => addTab(conn.id)}>+</button>
          </div>
        {/if}
      {/each}
      {#if $connections.length === 0}
        <span class="hint">no connections</span>
      {/if}
    </div>

    <div class="divider" />

    <!-- Requests tool: request tabs -->
    <div
      class="tool"
      class:active={$activeTool === 'requests'}
      style="--tint: var(--tool-requests-tint); --tint-text: var(--tool-requests-text);"
    >
      <button class="tool-label" on:click={() => activeTool.set('requests')}>Requests</button>
      {#each $requestTabs as rt (rt.id)}
        <button
          class="tab"
          class:active={$activeRequestTabId === rt.id && $activeTool === 'requests'}
          style="--dot: var(--m-{(rt.method || 'get').toLowerCase()})"
          on:click={() => selectReqTab(rt.id)}
          title={rt.title}
        >
          <span class="rt-method" style="color: var(--m-{(rt.method || 'get').toLowerCase()})">
            {rt.method}
          </span>
          {rt.dirty ? '•' : ''}{rt.title}
          <span class="x" on:click={(e) => closeReq(rt.id, e)} role="button" tabindex="-1">{ICONS.closeTab.glyph}</span>
        </button>
      {/each}
      <button class="add" title="New request" on:click={addReqTab}>+</button>
    </div>

    <div class="divider" />

    <div
      class="tool flat"
      class:active={$activeTool === 'inspector'}
      style="--tint: var(--tool-inspector-tint); --tint-text: var(--tool-inspector-text);"
    >
      <button class="tool-label" on:click={() => activeTool.set('inspector')}>Inspector</button>
    </div>
  </div>

  <button
    class="edge right"
    class:show={overflowing}
    disabled={!canRight}
    tabindex="-1"
    title="Scroll right"
    on:click={() => nudge(1)}>›</button
  >
</nav>

<style>
  .topnav {
    display: flex;
    align-items: stretch;
    background: var(--surface-1);
    border-bottom: 1px solid var(--border);
    min-width: 0;
    overflow: hidden;
    -webkit-app-region: drag;
  }
  .scroller {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--nav-gap, 5px);
    padding: var(--nav-pad, 6px) calc(var(--nav-pad, 6px) + 2px);
    white-space: nowrap;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none; /* Firefox */
  }
  .scroller::-webkit-scrollbar {
    display: none; /* Chromium / WebKit */
  }
  .topnav button {
    -webkit-app-region: no-drag;
  }
  /* left/right scroll affordances — only take space when usable */
  .edge {
    flex-shrink: 0;
    width: 0;
    border: none;
    background: var(--surface-1);
    color: var(--text-secondary);
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    overflow: hidden;
    opacity: 0;
    transition:
      width 0.12s ease,
      opacity 0.12s ease;
  }
  .edge.show {
    width: 22px;
    opacity: 1;
  }
  .edge:disabled {
    opacity: 0.28;
    cursor: default;
  }
  .edge:not(:disabled):hover {
    background: var(--surface-3);
    color: var(--text-primary);
  }
  .edge.left {
    box-shadow: 6px 0 6px -4px rgba(0, 0, 0, 0.18);
  }
  .edge.right {
    box-shadow: -6px 0 6px -4px rgba(0, 0, 0, 0.18);
  }
  .divider {
    width: 1px;
    height: 20px;
    background: var(--border-strong);
    margin: 0 4px;
    flex-shrink: 0;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: var(--nav-gap, 5px);
    background: var(--tint);
    border-radius: var(--radius);
    padding: calc(var(--nav-gap, 5px) - 1px);
    flex-shrink: 0;
  }
  .tool.active {
    box-shadow: 0 0 0 1.5px var(--tint-text) inset;
  }
  .caret {
    font-size: 7px;
    opacity: 0.6;
  }
  .tool-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--tint-text);
    padding: 4px 8px;
    background: none;
    border: none;
    cursor: pointer;
  }
  .conn {
    display: flex;
    align-items: center;
    gap: 2px;
    background: color-mix(in srgb, var(--c, var(--conn-slate)) 22%, var(--surface-2));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c, var(--conn-slate)) 38%, transparent);
    border-radius: 6px;
    padding: 2px;
  }
  .conn-label {
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.02em;
    color: color-mix(in srgb, var(--c, var(--text-secondary)) 55%, var(--text-primary));
    padding: 4px 6px;
    background: none;
    border: none;
    cursor: pointer;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: var(--text-secondary);
    padding: 4px 7px;
    border-radius: 4px;
    background: none;
    border: none;
    cursor: pointer;
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tab:hover {
    background: color-mix(in srgb, var(--c, var(--text-secondary)) 12%, transparent);
  }
  .tab.active {
    background: color-mix(in srgb, var(--c, var(--text-secondary)) 55%, var(--surface-2));
    color: var(--text-primary);
    font-weight: 600;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c, var(--text-secondary)) 40%, transparent);
  }
  .rt-method {
    font-size: 9px;
    font-weight: 800;
  }
  .x {
    font-size: 12px;
    opacity: 0.55;
  }
  .x:hover {
    opacity: 1;
  }
  .add {
    width: 20px;
    height: 20px;
    border-radius: 5px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    flex-shrink: 0;
  }
  .add:hover {
    background: var(--surface-3);
  }
  .hint {
    font-size: 11px;
    color: var(--text-muted);
    padding: 0 6px;
  }
</style>
