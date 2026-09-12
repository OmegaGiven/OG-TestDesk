<script>
  import ConnPicker from './ConnPicker.svelte';
  import { connMenuOpen } from '../stores.js';

  let modal = null;

  function close() {
    connMenuOpen.set(false);
  }
  function onKey(e) {
    if (e.key === 'Escape' && !modal) close();
  }
</script>

<svelte:window on:keydown={onKey} />

{#if $connMenuOpen}
  <div class="backdrop" on:mousedown|self={close} role="presentation">
    <div class="menu">
      <ConnPicker onDone={close} bind:modal />
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 800;
  }
  .menu {
    position: absolute;
    top: 46px;
    left: 10px;
    width: 320px;
    max-height: calc(100vh - 80px);
    display: flex;
    flex-direction: column;
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
  }
</style>
