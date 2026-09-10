<script>
  import { toasts, dismissToast } from '../stores.js';
</script>

<div class="wrap">
  {#each $toasts as t (t.id)}
    <button class="toast {t.kind}" on:click={() => dismissToast(t.id)} title="Dismiss">
      {t.message}
    </button>
  {/each}
</div>

<style>
  .wrap {
    position: fixed;
    bottom: 14px;
    right: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 1000;
    max-width: 380px;
  }
  .toast {
    text-align: left;
    font-size: 12px;
    line-height: 1.4;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    color: var(--text-primary);
    box-shadow: var(--shadow-pop);
    cursor: pointer;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .toast.error {
    border-color: var(--danger);
    background: color-mix(in srgb, var(--danger) 12%, var(--surface-2));
  }
  .toast.success {
    border-color: var(--ok);
    background: color-mix(in srgb, var(--ok) 12%, var(--surface-2));
  }
</style>
