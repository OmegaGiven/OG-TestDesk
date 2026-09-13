<script>
  import Modal from '../components/Modal.svelte';
  import { toast, toastError } from '../stores.js';
  import { api } from '../api.js';
  import { createEventDispatcher } from 'svelte';

  const dispatch = createEventDispatcher();
  const URL_KEY = 'og_testdesk_grpc_url';

  let url = localStorage.getItem(URL_KEY) || 'http://127.0.0.1:9000';
  let loadingServices = false;
  let services = [];
  let selectedService = '';
  let methods = [];
  let selectedMethod = null;
  let payload = '{}';
  let response = null;
  let error = null;
  let calling = false;

  async function discover() {
    error = null;
    loadingServices = true;
    services = [];
    selectedService = '';
    methods = [];
    selectedMethod = null;
    response = null;
    try {
      localStorage.setItem(URL_KEY, url);
    } catch {}
    try {
      services = await api.grpcListServices(url.trim());
      if (!services.length) toast('Server reflection returned no services', 'error', 3000);
    } catch (e) {
      error = String(e);
    } finally {
      loadingServices = false;
    }
  }

  async function pickService(svc) {
    selectedService = svc;
    selectedMethod = null;
    methods = [];
    try {
      methods = await api.grpcListMethods(url.trim(), svc);
    } catch (e) {
      toastError(e);
    }
  }

  function pickMethod(m) {
    selectedMethod = m;
    response = null;
    error = null;
    payload = '{}';
  }

  async function call() {
    if (!selectedMethod) return;
    try {
      JSON.parse(payload);
    } catch {
      toast('Request payload must be valid JSON', 'error', 2500);
      return;
    }
    calling = true;
    error = null;
    response = null;
    try {
      response = await api.grpcCallUnary(url.trim(), selectedService, selectedMethod.method, payload);
    } catch (e) {
      error = String(e);
    } finally {
      calling = false;
    }
  }
</script>

<Modal title="gRPC" width="700px" on:close>
  <div class="conn-bar">
    <input class="input mono" bind:value={url} placeholder="http://host:port" />
    <button class="btn primary sm" on:click={discover} disabled={loadingServices}>
      {loadingServices ? 'Discovering…' : 'Discover (reflection)'}
    </button>
  </div>
  <p class="hint">
    Uses server reflection to find services/methods — the server must have reflection enabled. Unary
    calls only; streaming RPCs aren't supported. No .proto file import — reflection-only discovery.
  </p>

  {#if error && !services.length}
    <div class="err">{error}</div>
  {/if}

  {#if services.length}
    <div class="grpc-layout">
      <div class="col services">
        <div class="col-label">Services</div>
        {#each services as svc}
          <button class="list-item" class:active={svc === selectedService} on:click={() => pickService(svc)}>
            {svc}
          </button>
        {/each}
      </div>
      <div class="col methods">
        <div class="col-label">Methods</div>
        {#each methods as m}
          <button class="list-item" class:active={selectedMethod === m} on:click={() => pickMethod(m)} disabled={m.client_streaming || m.server_streaming}>
            {m.method}
            {#if m.client_streaming || m.server_streaming}<span class="stream-tag">streaming — unsupported</span>{/if}
          </button>
        {/each}
        {#if selectedService && !methods.length}<div class="empty">Loading…</div>{/if}
      </div>
    </div>

    {#if selectedMethod}
      <div class="call-area">
        <div class="method-sig">{selectedMethod.input_type} → {selectedMethod.output_type}</div>
        <label for="payload">Request (JSON)</label>
        <textarea id="payload" class="input mono" rows="6" bind:value={payload} />
        <button class="btn primary sm" on:click={call} disabled={calling}>{calling ? 'Calling…' : 'Call'}</button>
        {#if error}
          <div class="err">{error}</div>
        {:else if response}
          <label for="resp">Response</label>
          <pre id="resp" class="resp">{response}</pre>
        {/if}
      </div>
    {/if}
  {/if}

  <svelte:fragment slot="footer">
    <span style="flex:1" />
    <button class="btn" on:click={() => dispatch('close')}>Close</button>
  </svelte:fragment>
</Modal>

<style>
  .conn-bar {
    display: flex;
    gap: 8px;
    margin-bottom: 4px;
  }
  .conn-bar .input {
    flex: 1;
  }
  .hint {
    font-size: 10.5px;
    color: var(--text-muted);
    margin: 4px 0 10px;
  }
  .err {
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: 11.5px;
    white-space: pre-wrap;
    padding: 8px;
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    border-radius: 5px;
    margin-top: 6px;
  }
  .grpc-layout {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    height: 200px;
    margin-bottom: 10px;
  }
  .col {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: auto;
  }
  .col-label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-muted);
    padding: 5px 8px;
    background: var(--surface-1);
    border-bottom: 1px solid var(--border);
    position: sticky;
    top: 0;
  }
  .list-item {
    all: unset;
    cursor: pointer;
    padding: 5px 8px;
    font-family: var(--font-mono);
    font-size: 11px;
    border-bottom: 1px solid var(--border);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .list-item:hover {
    background: var(--surface-3);
  }
  .list-item.active {
    background: var(--tool-requests-tint);
    color: var(--tool-requests-text);
  }
  .list-item:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .stream-tag {
    font-size: 9px;
    color: var(--text-muted);
    margin-left: 6px;
  }
  .empty {
    padding: 10px;
    color: var(--text-muted);
    font-size: 11px;
  }
  .call-area {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .method-sig {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-secondary);
  }
  .call-area label {
    font-size: 11px;
    color: var(--text-secondary);
  }
  textarea.input {
    width: 100%;
    resize: vertical;
    font-size: 11.5px;
  }
  .resp {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 8px;
    font-family: var(--font-mono);
    font-size: 11.5px;
    white-space: pre-wrap;
    max-height: 220px;
    overflow: auto;
    margin: 0;
  }
</style>
