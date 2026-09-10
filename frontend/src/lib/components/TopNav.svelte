<script>
  import {
    connections,
    sqlTabs,
    activeSqlTabId,
    activeTool,
    newSqlTab,
    closeSqlTab,
    persistSqlTab
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
</script>

<nav class="topnav">
  <!-- SQL tool: connection groups with tabs -->
  <div
    class="tool"
    class:active={$activeTool === 'sql'}
    style="--tint: var(--tool-sql-tint); --tint-text: var(--tool-sql-text);"
  >
    <button class="tool-label" on:click={() => activeTool.set('sql')}>SQL</button>
    {#each $connections as conn (conn.id)}
      <div class="conn">
        <span class="dot" style="background: {conn.color || 'var(--conn-slate)'}" />
        <button class="conn-label" on:click={() => activeTool.set('sql')}>{conn.nickname}</button>
        {#each tabsByConn(conn.id) as tab (tab.id)}
          <button
            class="tab"
            class:active={$activeSqlTabId === tab.id && $activeTool === 'sql'}
            style="--dot: {conn.color || 'var(--conn-slate)'}"
            on:click={() => selectTab(tab.id)}
            title={tab.title}
          >
            {tab.dirty ? '•' : ''}{tab.title}
            <span class="x" on:click={(e) => close(tab.id, e)} role="button" tabindex="-1">×</span>
          </button>
        {/each}
        <button class="add" title="New query" on:click={() => addTab(conn.id)}>+</button>
      </div>
    {/each}
    {#if $connections.length === 0}
      <span class="hint">no connections</span>
    {/if}
  </div>

  <div class="divider" />

  {#each TOOLS.slice(1) as tool}
    <div
      class="tool flat"
      class:active={$activeTool === tool.id}
      style="--tint: {tool.tint}; --tint-text: {tool.text};"
    >
      <button class="tool-label" on:click={() => activeTool.set(tool.id)}>{tool.label}</button>
    </div>
  {/each}
</nav>

<style>
  .topnav {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
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
    gap: 5px;
    background: var(--tint);
    border-radius: var(--radius);
    padding: 4px;
  }
  .tool.active {
    box-shadow: 0 0 0 1.5px var(--tint-text) inset;
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
    gap: 3px;
    background: var(--surface-2);
    border-radius: 6px;
    padding: 3px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    margin-left: 3px;
    flex-shrink: 0;
  }
  .conn-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    padding: 4px 5px;
    background: none;
    border: none;
    cursor: pointer;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: var(--text-muted);
    padding: 4px 7px;
    border-radius: 5px;
    background: none;
    border: none;
    cursor: pointer;
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tab.active {
    background: color-mix(in srgb, var(--dot, var(--text-secondary)) 20%, transparent);
    color: var(--text-primary);
    font-weight: 500;
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
