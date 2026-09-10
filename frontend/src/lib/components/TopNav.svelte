<script>
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

  const TOOLS = [
    { id: 'sql', label: 'SQL', tint: 'var(--tool-sql-tint)', text: 'var(--tool-sql-text)' },
    { id: 'requests', label: 'Requests', tint: 'var(--tool-requests-tint)', text: 'var(--tool-requests-text)' },
    { id: 'inspector', label: 'Inspector', tint: 'var(--tool-inspector-tint)', text: 'var(--tool-inspector-text)' }
  ];

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
</script>

<nav class="topnav">
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
              <span class="x" on:click={(e) => close(tab.id, e)} role="button" tabindex="-1">×</span>
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
        <span class="x" on:click={(e) => closeReq(rt.id, e)} role="button" tabindex="-1">×</span>
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
</nav>

<style>
  .topnav {
    display: flex;
    align-items: center;
    gap: var(--nav-gap, 5px);
    padding: var(--nav-pad, 6px) calc(var(--nav-pad, 6px) + 2px);
    overflow-x: auto;
    white-space: nowrap;
    background: var(--surface-1);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    -webkit-app-region: drag;
  }
  .topnav button {
    -webkit-app-region: no-drag;
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
    /* whole group tinted with the connection colour */
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
