<script>
  // Custom min / max / close for frameless windows (Windows & Linux).
  // Not rendered on macOS — the OS traffic lights overlay the top bar there.
  import { onMount } from 'svelte';
  import { api } from '../api.js';

  let win = null;
  let maximized = false;
  let tiling = false; // tiling WM → hide min/max, keep close

  onMount(async () => {
    try {
      tiling = !!(await api.windowEnvironment()).tiling;
    } catch {}
    try {
      const mod = await import('@tauri-apps/api/window');
      win = mod.getCurrentWindow();
      maximized = await win.isMaximized();
      await win.onResized(async () => {
        try {
          maximized = await win.isMaximized();
        } catch {}
      });
    } catch {
      win = null;
    }
  });

  const act = (fn) => async () => {
    try {
      await win[fn]();
      if (fn === 'toggleMaximize') maximized = await win.isMaximized();
    } catch {}
  };
</script>

{#if win}
  <div class="wc">
    {#if !tiling}
      <button class="wc-btn" title="Minimize" on:click={act('minimize')} aria-label="Minimize">
        <svg width="10" height="10" viewBox="0 0 10 10"><line x1="1" y1="5" x2="9" y2="5" /></svg>
      </button>
      <button
        class="wc-btn"
        title={maximized ? 'Restore' : 'Maximize'}
        on:click={act('toggleMaximize')}
        aria-label="Maximize"
      >
        {#if maximized}
          <svg width="10" height="10" viewBox="0 0 10 10">
            <rect x="2.5" y="1" width="6" height="6" />
            <rect x="1" y="3" width="6" height="6" fill="var(--surface-1)" />
          </svg>
        {:else}
          <svg width="10" height="10" viewBox="0 0 10 10"><rect x="1" y="1" width="8" height="8" /></svg>
        {/if}
      </button>
    {/if}
    <button class="wc-btn close" title="Close" on:click={act('close')} aria-label="Close">
      <svg width="10" height="10" viewBox="0 0 10 10">
        <line x1="1" y1="1" x2="9" y2="9" /><line x1="9" y1="1" x2="1" y2="9" />
      </svg>
    </button>
  </div>
{/if}

<style>
  .wc {
    display: flex;
    align-self: stretch;
    -webkit-app-region: no-drag;
  }
  .wc-btn {
    min-width: 44px;
    border: none;
    border-left: 1px solid var(--border);
    background: var(--surface-1);
    color: var(--text-secondary);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .wc-btn svg {
    width: 14px;
    height: 14px;
    stroke: currentColor;
    stroke-width: 1.1;
    fill: none;
  }
  .wc-btn:hover {
    color: var(--text-primary);
  }
  .wc-btn:hover {
    background: var(--surface-3);
  }
  .wc-btn.close:hover {
    background: #e81123;
    color: #fff;
  }
</style>
