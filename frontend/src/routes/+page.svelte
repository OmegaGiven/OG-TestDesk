<script>
  import TopNav from '$lib/components/TopNav.svelte';

  // Placeholder data — will come from `list_connections` / metadata store
  // via @tauri-apps/api invoke() once the backend commands are filled in.
  let tools = [
    {
      id: 'sql',
      label: 'SQL',
      tint: 'var(--tool-sql-tint)',
      text: 'var(--tool-sql-text)',
      connections: [
        {
          id: 'prod',
          label: 'PROD',
          color: 'var(--conn-red)',
          tabs: [
            { id: 't1', label: 'Unnamed 1' },
            { id: 't2', label: 'Failed alerts' }
          ]
        },
        {
          id: 'staging-read',
          label: 'STAGING READ',
          color: 'var(--conn-amber)',
          tabs: [{ id: 't3', label: 'User activities' }]
        }
      ]
    },
    {
      id: 'requests',
      label: 'Requests',
      tint: 'var(--tool-requests-tint)',
      text: 'var(--tool-requests-text)',
      tabs: [{ id: 'r1', label: 'Carriers collection' }]
    },
    {
      id: 'inspector',
      label: 'Inspector',
      tint: 'var(--tool-inspector-tint)',
      text: 'var(--tool-inspector-text)',
      tabs: [{ id: 'i1', label: 'GET highway.co...' }]
    }
  ];

  let activeToolId = 'sql';
  let activeTabId = 't1';
</script>

<TopNav
  {tools}
  bind:activeToolId
  bind:activeTabId
  on:selectTab={(e) => console.log('select tab', e.detail)}
  on:addTab={(e) => console.log('add tab', e.detail)}
  on:closeTab={(e) => console.log('close tab', e.detail)}
/>

<main style="padding: 24px; color: var(--text-secondary); font-size: 13px;">
  Active tool: {activeToolId} · Active tab: {activeTabId}
</main>
