<script>
  import Modal from '../components/Modal.svelte';
  import { api } from '../api.js';
  import { toast, toastError, confirmDialog } from '../stores.js';
  import { createEventDispatcher, onMount } from 'svelte';

  const dispatch = createEventDispatcher();
  let cookies = [];
  let loading = true;

  onMount(load);
  async function load() {
    loading = true;
    try {
      cookies = await api.cookiesList();
    } catch (e) {
      toastError(e);
    } finally {
      loading = false;
    }
  }

  async function remove(c) {
    try {
      await api.cookieDelete(c.domain, c.name);
      await load();
    } catch (e) {
      toastError(e);
    }
  }

  async function clearAll() {
    if (!(await confirmDialog('Clear every cookie the app has collected?', { danger: true }))) return;
    try {
      await api.cookiesClear(null);
      await load();
      toast('Cookies cleared', 'success', 1800);
    } catch (e) {
      toastError(e);
    }
  }

  $: byDomain = cookies.reduce((acc, c) => {
    (acc[c.domain] ||= []).push(c);
    return acc;
  }, {});
</script>

<Modal title="Cookies" width="560px" on:close>
  {#if loading}
    <div class="empty">Loading…</div>
  {:else if !cookies.length}
    <div class="empty">
      No cookies yet — they're collected automatically from any response's Set-Cookie header and replayed
      on later requests to the same domain, like a browser.
    </div>
  {:else}
    {#each Object.entries(byDomain) as [domain, list]}
      <div class="domain-group">
        <div class="domain-head">{domain}</div>
        {#each list as c}
          <div class="cookie-row">
            <span class="cname">{c.name}</span>
            <span class="cval" title={c.value}>{c.value}</span>
            <span class="cflags">
              {#if c.secure}<span class="flag">Secure</span>{/if}
              {#if c.http_only}<span class="flag">HttpOnly</span>{/if}
              <span class="flag muted">{c.path}</span>
            </span>
            <button class="btn ghost sm" on:click={() => remove(c)}>Delete</button>
          </div>
        {/each}
      </div>
    {/each}
  {/if}

  <svelte:fragment slot="footer">
    <button class="btn danger" on:click={clearAll} disabled={!cookies.length}>Clear all</button>
    <span style="flex:1" />
    <button class="btn" on:click={() => dispatch('close')}>Close</button>
  </svelte:fragment>
</Modal>

<style>
  .empty {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
    line-height: 1.6;
  }
  .domain-group {
    margin-bottom: 12px;
  }
  .domain-head {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    padding: 4px 0;
    border-bottom: 1px solid var(--border);
    margin-bottom: 4px;
  }
  .cookie-row {
    display: grid;
    grid-template-columns: 1fr 1.5fr auto auto;
    align-items: center;
    gap: 8px;
    padding: 4px 0;
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .cname {
    color: var(--j-key);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cval {
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cflags {
    display: flex;
    gap: 4px;
  }
  .flag {
    font-size: 9px;
    background: var(--surface-3);
    color: var(--text-muted);
    padding: 1px 5px;
    border-radius: 3px;
  }
  .flag.muted {
    opacity: 0.7;
  }
</style>
