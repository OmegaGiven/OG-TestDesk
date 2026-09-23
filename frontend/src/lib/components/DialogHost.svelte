<script>
  import { tick } from 'svelte';
  import Modal from './Modal.svelte';
  import { dialogRequest } from '../stores.js';

  let value = '';
  let inputEl;

  // The HTML `autofocus` attribute is unreliable in an embedded
  // webview (WKWebView on macOS notably) — it can lose the race
  // against the modal's own mount/layout, leaving focus nowhere and
  // every keystroke going nowhere. Focusing explicitly after Svelte's
  // finished the DOM update is the reliable version of the same intent.
  $: if ($dialogRequest?.type === 'prompt') {
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
