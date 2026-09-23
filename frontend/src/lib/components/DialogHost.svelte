<script>
  import { tick } from 'svelte';
  import Modal from './Modal.svelte';
  import { dialogRequest } from '../stores.js';

  let value = '';
  let inputEl;

  // Init (seed `value`, focus the input) exactly once per dialog, the
  // moment it opens — NOT a continuously-reactive block. `$: if (...) {
  // value = ... }` looked right but wasn't: Svelte re-runs a component's
  // reactive statements on every flush, and re-assigning `value` here
  // (itself a dirtying write) was enough to make this block's own guard
  // re-evaluate truthy on the very next flush too — every keystroke
  // (which reassigns `value` via bind:value) re-triggered this block,
  // which stomped `value` straight back to defaultValue before the
  // keystroke could ever be seen: typing looked like it did nothing.
  // Tracking which request we've already initialized makes the whole
  // block run at most once per dialog, however many flushes happen.
  let initedFor = null;
  $: if ($dialogRequest?.type === 'prompt' && $dialogRequest !== initedFor) {
    initedFor = $dialogRequest;
    value = $dialogRequest.defaultValue ?? '';
    tick().then(() => inputEl?.focus());
  }

  function settle(result) {
    const req = $dialogRequest;
    dialogRequest.set(null);
    req?.resolve(result);
  }
  function onSubmit() {
    if ($dialogRequest.type === 'confirm') settle(true);
    else settle(value.trim() ? value : null);
  }
  function onCancel() {
    settle($dialogRequest.type === 'confirm' ? false : null);
  }
  function onKeydown(e) {
    if (e.key === 'Enter' && $dialogRequest?.type === 'prompt') onSubmit();
  }
</script>

{#if $dialogRequest}
  <Modal title={$dialogRequest.type === 'confirm' ? 'Confirm' : 'Input needed'} width="380px" on:close={onCancel}>
    <p class="msg">{$dialogRequest.message}</p>
    {#if $dialogRequest.type === 'prompt'}
      <input class="input" bind:value bind:this={inputEl} on:keydown={onKeydown} />
    {/if}
    <svelte:fragment slot="footer">
      <button class="btn" on:click={onCancel}>Cancel</button>
      <button class="btn primary" class:danger={$dialogRequest.danger} on:click={onSubmit}>
        {$dialogRequest.type === 'confirm' ? ($dialogRequest.danger ? 'Delete' : 'OK') : 'OK'}
      </button>
    </svelte:fragment>
  </Modal>
{/if}

<style>
  .msg {
    margin: 0 0 10px;
    font-size: 13px;
    color: var(--text-primary);
    white-space: pre-wrap;
  }
  .btn.primary.danger {
    background: var(--danger);
    border-color: var(--danger);
  }
</style>
