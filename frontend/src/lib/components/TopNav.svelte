<script>
  // tools: [{ id, label, tint, text, connections?: [{ id, label, color, tabs: [{id,label}] }], tabs?: [{id,label}] }]
  export let tools = [];
  export let activeToolId = tools[0]?.id;
  export let activeTabId = null;

  import { createEventDispatcher } from 'svelte';
  const dispatch = createEventDispatcher();

  function selectTool(toolId) {
    activeToolId = toolId;
    dispatch('selectTool', { toolId });
  }

  function selectTab(toolId, tabId) {
    activeToolId = toolId;
    activeTabId = tabId;
    dispatch('selectTab', { toolId, tabId });
  }

  function addTab(toolId, connectionId = null) {
    dispatch('addTab', { toolId, connectionId });
  }

  function closeTab(toolId, tabId, event) {
    event.stopPropagation();
    dispatch('closeTab', { toolId, tabId });
  }
</script>

<nav class="topnav">
  {#each tools as tool, i}
    {#if i > 0}<div class="divider" />{/if}

    <div
      class="tool-group"
      class:active={activeToolId === tool.id}
      style="--tint: {tool.tint}; --tint-text: {tool.text};"
    >
      <button class="tool-label" on:click={() => selectTool(tool.id)}>
        {tool.label}
      </button>

      {#if tool.connections}
        {#each tool.connections as conn}
          <div class="conn-group">
            <span class="conn-dot" style="background: {conn.color}" aria-hidden="true" />
            <button class="conn-label" on:click={() => selectTool(tool.id)}>
              {conn.label}
            </button>
            {#each conn.tabs as tab}
              <button
                class="tab"
                class:active={activeTabId === tab.id}
                style="--dot: {conn.color}"
                on:click={() => selectTab(tool.id, tab.id)}
              >
                {tab.label}
                <span class="close" on:click={(e) => closeTab(tool.id, tab.id, e)}>×</span>
              </button>
            {/each}
          </div>
        {/each}
        <button class="add-btn" on:click={() => addTab(tool.id)} aria-label="Add connection">+</button>
      {:else if tool.tabs}
        {#each tool.tabs as tab}
          <button
            class="tab"
            class:active={activeTabId === tab.id}
            on:click={() => selectTab(tool.id, tab.id)}
          >
            {tab.label}
            <span class="close" on:click={(e) => closeTab(tool.id, tab.id, e)}>×</span>
          </button>
        {/each}
      {/if}
    </div>
  {/each}
</nav>

<style>
  .topnav {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 12px;
    overflow-x: auto;
    white-space: nowrap;
    background: var(--surface-1);
    border-bottom: 0.5px solid var(--border);
  }

  .divider {
    width: 0.5px;
    height: 20px;
    background: var(--border);
    margin: 0 4px;
    flex-shrink: 0;
  }

  .tool-group {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--tint);
    border-radius: 8px;
    padding: 4px;
  }

  .tool-group.active {
    box-shadow: 0 0 0 1.5px var(--tint-text) inset;
  }

  .tool-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--tint-text);
    padding: 4px 8px;
    background: none;
    border: none;
    cursor: pointer;
  }

  .conn-group {
    display: flex;
    align-items: center;
    gap: 4px;
    background: var(--surface-2);
    border-radius: 6px;
    padding: 3px;
  }

  .conn-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    margin-left: 4px;
    flex-shrink: 0;
  }

  .conn-label {
    font-size: 12px;
    color: var(--text-secondary);
    padding: 5px 6px;
    background: none;
    border: none;
    cursor: pointer;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-muted);
    padding: 5px 8px;
    border-radius: 5px;
    background: none;
    border: none;
    cursor: pointer;
  }

  .tab.active {
    background: color-mix(in srgb, var(--dot, var(--text-accent)) 15%, transparent);
    color: var(--text-primary);
    font-weight: 500;
  }

  .close {
    font-size: 11px;
    opacity: 0.6;
  }

  .close:hover {
    opacity: 1;
  }

  .add-btn {
    width: 24px;
    height: 24px;
    border-radius: 6px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    flex-shrink: 0;
  }

  .add-btn:hover {
    background: var(--surface-2);
  }
</style>
