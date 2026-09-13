<script>
  import Modal from '../components/Modal.svelte';
  import { api } from '../api.js';
  import { toast, toastError } from '../stores.js';
  import { ICONS } from '../icons.js';
  import { createEventDispatcher, onMount } from 'svelte';

  const dispatch = createEventDispatcher();
  let loading = true;
  let saving = false;
  let proxyUrl = '';
  let extraCaPem = '';
  let clientCerts = []; // [{host, pem}]

  onMount(load);
  async function load() {
    loading = true;
    try {
      const s = await api.networkSettingsGet();
      proxyUrl = s.proxy_url || '';
      extraCaPem = s.extra_ca_pem || '';
      clientCerts = (s.client_certs || []).map((c) => ({ host: c.host, pem: c.pem }));
    } catch (e) {
      toastError(e);
    } finally {
      loading = false;
    }
  }

  function addCert() {
    clientCerts = [...clientCerts, { host: '', pem: '' }];
  }
  function removeCert(i) {
    clientCerts = clientCerts.filter((_, idx) => idx !== i);
  }

  async function save() {
    saving = true;
    try {
      await api.networkSettingsSet({
        proxy_url: proxyUrl.trim() || null,
        extra_ca_pem: extraCaPem.trim() || null,
        client_certs: clientCerts.filter((c) => c.host.trim() && c.pem.trim())
      });
      toast('Network settings saved', 'success', 1800);
      dispatch('close');
    } catch (e) {
      toastError(e);
    } finally {
      saving = false;
    }
  }
</script>

<Modal title="Network settings" width="560px" on:close>
  {#if loading}
    <div class="empty">Loading…</div>
  {:else}
    <div class="field">
      <label for="proxyUrl">HTTP/SOCKS proxy <span class="muted">(applies to every request)</span></label>
      <input id="proxyUrl" class="input mono" bind:value={proxyUrl} placeholder="http://user:pass@proxy.company.com:8080" />
    </div>
    <div class="field">
      <label for="caCert">Extra trusted CA certificate <span class="muted">(PEM — for internal APIs on a private/self-signed CA)</span></label>
      <textarea id="caCert" class="input mono" rows="4" bind:value={extraCaPem} placeholder={'-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----'} />
    </div>
    <div class="field">
      <label>Client certificates <span class="muted">(mTLS — per host, cert + key concatenated in one PEM)</span></label>
      {#each clientCerts as c, i}
        <div class="cert-row">
          <input class="input mono" bind:value={c.host} placeholder="api.example.com" />
          <textarea class="input mono" rows="3" bind:value={c.pem} placeholder={'-----BEGIN CERTIFICATE-----\n...\n-----BEGIN PRIVATE KEY-----\n...'} />
          <button class="btn ghost sm" on:click={() => removeCert(i)}>{@html ICONS.cancel.svg}</button>
        </div>
      {/each}
      <button class="btn ghost sm" on:click={addCert}>+ Add client certificate</button>
    </div>
  {/if}

  <svelte:fragment slot="footer">
    <span style="flex:1" />
    <button class="btn" on:click={() => dispatch('close')} disabled={saving}>Cancel</button>
    <button class="btn primary" on:click={save} disabled={saving || loading}>{saving ? 'Saving…' : 'Save'}</button>
  </svelte:fragment>
</Modal>

<style>
  .empty {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
  }
  .field {
    margin-bottom: 14px;
  }
  .field > label {
    display: block;
    font-size: 11px;
    color: var(--text-secondary);
    margin-bottom: 4px;
  }
  .muted {
    color: var(--text-muted);
    font-weight: 400;
  }
  textarea.input {
    width: 100%;
    resize: vertical;
    font-size: 11px;
    line-height: 1.4;
    padding: 6px 8px;
  }
  .cert-row {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-areas: 'host del' 'pem pem';
    gap: 4px 8px;
    margin-bottom: 8px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border);
  }
  .cert-row input {
    grid-area: host;
  }
  .cert-row textarea {
    grid-area: pem;
  }
  .cert-row button {
    grid-area: del;
  }
</style>
