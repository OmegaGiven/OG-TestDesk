<script>
  import { createEventDispatcher } from 'svelte';
  export let title = '';
  export let width = '460px';
  const dispatch = createEventDispatcher();
  function close() {
    dispatch('close');
  }
  function key(e) {
    if (e.key === 'Escape') close();
  }
</script>

<svelte:window on:keydown={key} />

<div class="backdrop" on:mousedown|self={close} role="presentation">
  <div class="panel" style="width:{width}">
    <header>
      <span>{title}</span>
      <button class="x" on:click={close} aria-label="Close">×</button>
    </header>
    <div class="body">
      <slot />
    </div>
    {#if $$slots.footer}
      <footer><slot name="footer" /></footer>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 900;
  }
  .panel {
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 60px);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
    font-weight: 600;
    font-size: 13px;
  }
  .x {
    border: none;
    background: none;
    font-size: 18px;
    line-height: 1;
    color: var(--text-muted);
    cursor: pointer;
  }
  .body {
    padding: 14px;
    overflow: auto;
  }
  footer {
    padding: 12px 14px;
    border-top: 1px solid var(--border);
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
