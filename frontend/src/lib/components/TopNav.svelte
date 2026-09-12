<script>
  import { onMount, tick } from 'svelte';
  import {
    connections,
    sqlTabs,
    activeSqlTabId,
    activeTool,
    closeSqlTab,
    persistSqlTab,
    requestTabs,
    activeRequestTabId,
    closeRequestTab,
    persistRequestTab,
    connMenuOpen,
    inspectorOpen,
    groupOrder,
    tabGroupOverride,
    groupOf,
    reorderGroups,
    moveTabToGroup,
    INSPECTOR_TAB_ID
  } from '../stores.js';
  import { ICONS } from '../icons.js';

  const inspectorTab = { id: INSPECTOR_TAB_ID, title: 'Inspector' };

  function selectTab(id) {
    activeTool.set('sql');
    activeSqlTabId.set(id);
    persistSqlTab(id, true);
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
  function closeReq(id, e) {
    e.stopPropagation();
    closeRequestTab(id);
  }

  function selectInspector() {
    activeTool.set('inspector');
  }
  function closeInspector(e) {
    e.stopPropagation();
    inspectorOpen.set(false);
    if ($activeTool === 'inspector') activeTool.set('sql');
  }

  // ---- group tabs by connection, honoring any manual drag-to-regroup
  // override, in the user's saved group order. Only SQL tabs, request
  // tabs, and the Inspector "tab" that have been dragged into a
  // connection's group end up here — anything with no override renders
  // standalone in the loose row below instead.
  function groupBy(list, kind, overrides) {
    const map = new Map();
    for (const tab of list) {
      const key = groupOf(kind, tab, overrides);
      if (key == null) continue;
      if (!map.has(key)) map.set(key, []);
      map.get(key).push({ kind, tab });
    }
    return map;
  }
  $: sqlByGroup = groupBy($sqlTabs, 'sql', $tabGroupOverride);
  $: reqByGroup = groupBy($requestTabs, 'request', $tabGroupOverride);
  $: inspByGroup = groupBy($inspectorOpen ? [inspectorTab] : [], 'inspector', $tabGroupOverride);
  $: allKeys = (() => {
    const order = [...$groupOrder];
    const seen = new Set(order);
    for (const k of [...sqlByGroup.keys(), ...reqByGroup.keys(), ...inspByGroup.keys()]) {
      if (!seen.has(k)) {
        seen.add(k);
        order.push(k);
      }
    }
    return order;
  })();
  $: renderGroups = allKeys
    .map((key) => {
      const items = [
        ...(sqlByGroup.get(key) || []),
        ...(reqByGroup.get(key) || []),
        ...(inspByGroup.get(key) || [])
      ].sort((a, b) => (a.tab.position ?? 0) - (b.tab.position ?? 0));
      const conn = $connections.find((c) => c.id === key);
      return { key, kind: 'conn', conn, items };
    })
    .filter((g) => g.items.length && g.conn);

  // ---- standalone (ungrouped) request tabs + the Inspector pill
  $: looseRequestTabs = $requestTabs.filter(
    (tab) => groupOf('request', tab, $tabGroupOverride) == null
  );
  $: looseInspector = $inspectorOpen && groupOf('inspector', inspectorTab, $tabGroupOverride) == null;

  // ---- drag & drop: reorder whole groups, or drag a tab into another group
  let draggedGroup = null;
  let draggedTab = null; // { kind, tab }
  let dragOverKey = null;

  function onGroupDragStart(key, e) {
    draggedGroup = key;
    draggedTab = null;
    e.dataTransfer.effectAllowed = 'move';
  }
  function onTabDragStart(kind, tab, e) {
    draggedTab = { kind, tab };
    draggedGroup = null;
    e.dataTransfer.effectAllowed = 'move';
  }
  function onGroupDragOver(key, e) {
    e.preventDefault();
    dragOverKey = key;
  }
  function onGroupDrop(key) {
    if (draggedGroup && draggedGroup !== key) reorderGroups(draggedGroup, key);
    else if (draggedTab) moveTabToGroup(draggedTab.kind, draggedTab.tab, key);
    draggedGroup = null;
    draggedTab = null;
    dragOverKey = null;
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
  $: if (renderGroups) tick().then(refresh);
</script>

<nav class="topnav">
  <button
    class="icon-btn plus"
    title="New connection, request, or open Inspector"
    on:click={() => connMenuOpen.update((v) => !v)}>+</button
  >

  <button
    class="edge left"
    class:show={overflowing}
    disabled={!canLeft}
    tabindex="-1"
    title="Scroll left"
    on:click={() => nudge(-1)}>‹</button
  >

  <div class="scroller" bind:this={scroller} on:scroll={refresh} on:wheel={onWheel}>
    {#each renderGroups as g (g.key)}
      <div
        class="tool"
        class:over={dragOverKey === g.key}
        style="--c: {g.conn.color || 'var(--conn-slate)'}"
        on:dragover={(e) => onGroupDragOver(g.key, e)}
        on:dragleave={() => (dragOverKey = null)}
        on:drop={() => onGroupDrop(g.key)}
      >
        <button
          class="group-label"
          draggable="true"
          on:dragstart={(e) => onGroupDragStart(g.key, e)}
          on:click={() => connMenuOpen.set(true)}
          title="Drag to reorder · click to switch connection"
        >
          {g.conn.nickname}
        </button>
        {#each g.items as item (item.kind + ':' + item.tab.id)}
          {#if item.kind === 'sql'}
            <button
              class="tab"
              class:active={$activeSqlTabId === item.tab.id && $activeTool === 'sql'}
              draggable="true"
              on:dragstart|stopPropagation={(e) => onTabDragStart('sql', item.tab, e)}
              on:click={() => selectTab(item.tab.id)}
              title={item.tab.title}
            >
              {item.tab.dirty ? '•' : ''}{item.tab.title}
              <span class="x" on:click={(e) => close(item.tab.id, e)} role="button" tabindex="-1"
                >{ICONS.closeTab.glyph}</span
              >
            </button>
          {:else if item.kind === 'request'}
            <button
              class="tab"
              class:active={$activeRequestTabId === item.tab.id && $activeTool === 'requests'}
              draggable="true"
              on:dragstart|stopPropagation={(e) => onTabDragStart('request', item.tab, e)}
              on:click={() => selectReqTab(item.tab.id)}
              title={item.tab.title}
            >
              <span class="rt-method" style="color: var(--m-{(item.tab.method || 'get').toLowerCase()})">
                {item.tab.method}
              </span>
              {item.tab.dirty ? '•' : ''}{item.tab.title}
              <span class="x" on:click={(e) => closeReq(item.tab.id, e)} role="button" tabindex="-1"
                >{ICONS.closeTab.glyph}</span
              >
            </button>
          {:else}
            <button
              class="tab"
              class:active={$activeTool === 'inspector'}
              draggable="true"
              on:dragstart|stopPropagation={(e) => onTabDragStart('inspector', item.tab, e)}
              on:click={selectInspector}
              title="Inspector"
            >
              <span class="insp-icon" style="color: var(--tool-inspector-text)">I</span>
              Inspector
              <span class="x" on:click={closeInspector} role="button" tabindex="-1"
                >{ICONS.closeTab.glyph}</span
              >
            </button>
          {/if}
        {/each}
      </div>
    {/each}

    {#each looseRequestTabs as tab (tab.id)}
      <button
        class="tab loose"
        class:active={$activeRequestTabId === tab.id && $activeTool === 'requests'}
        draggable="true"
        on:dragstart={(e) => onTabDragStart('request', tab, e)}
        on:click={() => selectReqTab(tab.id)}
        title={tab.title}
      >
        <span class="rt-method" style="color: var(--m-{(tab.method || 'get').toLowerCase()})">
          {tab.method}
        </span>
        {tab.dirty ? '•' : ''}{tab.title}
        <span class="x" on:click={(e) => closeReq(tab.id, e)} role="button" tabindex="-1"
          >{ICONS.closeTab.glyph}</span
        >
      </button>
    {/each}

    {#if looseInspector}
      <button
        class="tab loose"
        class:active={$activeTool === 'inspector'}
        draggable="true"
        on:dragstart={(e) => onTabDragStart('inspector', inspectorTab, e)}
        on:click={selectInspector}
        title="Inspector"
      >
        <span class="insp-icon" style="color: var(--tool-inspector-text)">I</span>
        Inspector
        <span class="x" on:click={closeInspector} role="button" tabindex="-1"
          >{ICONS.closeTab.glyph}</span
        >
      </button>
    {/if}
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
  .plus {
    flex-shrink: 0;
    align-self: center;
    margin-left: var(--nav-pad, 6px);
    color: var(--text-secondary);
    font-weight: 700;
  }
  .plus:hover {
    background: var(--surface-3);
    color: var(--text-primary);
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
  .tool {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
    background: color-mix(in srgb, var(--c, var(--conn-slate)) 22%, var(--surface-2));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c, var(--conn-slate)) 38%, transparent);
    border-radius: 6px;
    padding: 2px;
    border: none;
  }
  .tool.over {
    box-shadow: inset 0 0 0 2px var(--c, var(--tool-sql-text));
  }
  .group-label {
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
  .tab.loose {
    flex-shrink: 0;
  }
  .insp-icon {
    font-size: 9px;
    font-weight: 800;
    font-style: italic;
    width: 12px;
    height: 12px;
    line-height: 12px;
    text-align: center;
    border-radius: 3px;
    box-shadow: inset 0 0 0 1px currentColor;
    display: inline-block;
  }
  .x {
    font-size: 12px;
    opacity: 0.55;
  }
  .x:hover {
    opacity: 1;
  }
</style>
