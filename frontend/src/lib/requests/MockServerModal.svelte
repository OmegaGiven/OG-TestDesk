<script>
  import Modal from '../components/Modal.svelte';
  import { api } from '../api.js';
  import { toast, toastError, confirmDialog } from '../stores.js';
  import { createEventDispatcher, onMount } from 'svelte';

  const dispatch = createEventDispatcher();
  const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'];

  let routes = [];
  let loading = true;
  let status = { running: false, port: null };
  let port = 7799;

  onMount(load);
  async function load() {
    loading = true;
    try {
      [routes, status] = await Promise.all([api.mockRoutesList(), api.mockServerStatus()]);
      if (status.port) port = status.port;
    } catch (e) {
      toastError(e);
    } finally {
      loading = false;
    }
  }

  function blank() {
    return { id: '', method: 'GET', path: '/', status: 200, headers_json: '{"Content-Type":"application/json"}', body: '{}', enabled: true };
  }
  let editing = null; // route being edited, or null

  async function save() {
    try {
      JSON.parse(editing.headers_json || '{}');
    } catch {
      toast('Headers must be valid JSON', 'error', 2500);
      return;
    }
    try {
      await api.mockRouteSave({ ...editing, status: Number(editing.status) });
      editing = null;
      await load();
    } catch (e) {
      toastError(e);
    }
  }

  async function remove(r) {
    if (!(await confirmDialog(`Delete mock route ${r.method} ${r.path}?`, { danger: true }))) return;
    try {
      await api.mockRouteDelete(r.id);
      await load();
    } catch (e) {
      toastError(e);
    }
  }

  async function toggleEnabled(r) {
    try {
      await api.mockRouteSave({ ...r, enabled: !r.enabled });
      await load();
    } catch (e) {
      toastError(e);
    }
  }

  async function toggleServer() {
    try {
      if (status.running) {
        status = await api.mockServerStop();
        toast('Mock server stopped', 'success', 1500);
      } else {
        status = await api.mockServerStart(Number(port));
        toast(`Mock server running on 127.0.0.1:${status.port}`, 'success', 2500);
      }
    } catch (e) {
      toastError(e);
    }
  }
</script>

<Modal title="Mock server" width="620px" on:close>
  <div class="server-bar">
    <span class="dot" class:on={status.running} />
    <span class="status-text">
      {status.running ? `Running on 127.0.0.1:${status.port}` : 'Stopped'}
    </span>
    {#if !status.running}
      <input class="input mono sm" type="number" bind:value={port} style="width:80px" />
    {/if}
    <button class="btn {status.running ? 'danger' : 'primary'} sm" on:click={toggleServer}>
      {status.running ? 'Stop' : 'Start'}
    </button>
  </div>
  <p class="hint">
    Exact method + path matches only — no wildcards or path params yet. Routes are read live, so edits
    apply immediately without restarting the server.
  </p>

  {#if loading}
    <div class="empty">Loading…</div>
  {:else if editing}
    <div class="edit-form">
      <div class="row2">
        <div class="field">
          <label for="m">Method</label>
          <select id="m" class="select" bind:value={editing.method}>
            {#each METHODS as m}<option value={m}>{m}</option>{/each}
          </select>
        </div>
        <div class="field" style="flex:2">
          <label for="p">Path</label>
          <input id="p" class="input mono" bind:value={editing.path} placeholder="/api/users" />
        </div>
        <div class="field">
          <label for="s">Status</label>
          <input id="s" class="input" type="number" bind:value={editing.status} />
        </div>
      </div>
      <div class="field">
        <label for="h">Headers (JSON)</label>
        <input id="h" class="input mono" bind:value={editing.headers_json} />
      </div>
      <div class="field">
        <label for="b">Body</label>
        <textarea id="b" class="input mono" rows="6" bind:value={editing.body} />
      </div>
      <div class="edit-actions">
        <button class="btn" on:click={() => (editing = null)}>Cancel</button>
        <button class="btn primary" on:click={save}>Save route</button>
      </div>
    </div>
  {:else}
    <button class="btn ghost sm" on:click={() => (editing = blank())}>+ Add route</button>
    {#each routes as r}
      <div class="route-row" class:off={!r.enabled}>
        <input type="checkbox" checked={r.enabled} on:change={() => toggleEnabled(r)} />
        <span class="method" style="color: var(--m-{r.method.toLowerCase()})">{r.method}</span>
        <span class="path">{r.path}</span>
        <span class="rstatus">{r.status}</span>
        <button class="btn ghost sm" on:click={() => (editing = { ...r })}>Edit</button>
        <button class="btn ghost sm" on:click={() => remove(r)}>Delete</button>
      </div>
    {/each}
    {#if !routes.length}
      <div class="empty">No mock routes yet.</div>
    {/if}
  {/if}

  <svelte:fragment slot="footer">
    <span style="flex:1" />
    <button class="btn" on:click={() => dispatch('close')}>Close</button>
  </svelte:fragment>
</Modal>

<style>
  .server-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-muted);
  }
  .dot.on {
    background: var(--ok);
  }
  .status-text {
    font-size: 12px;
    font-family: var(--font-mono);
    flex: 1;
  }
  .hint {
    font-size: 10.5px;
    color: var(--text-muted);
    margin: 4px 0 10px;
  }
  .empty {
    padding: 20px;
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
  }
  .route-row {
    display: grid;
    grid-template-columns: 18px auto 1fr auto auto auto;
    align-items: center;
    gap: 8px;
    padding: 5px 0;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  .route-row.off {
    opacity: 0.5;
  }
  .method {
    font-family: var(--font-mono);
    font-weight: 700;
    font-size: 10.5px;
  }
  .path {
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rstatus {
    font-family: var(--font-mono);
    color: var(--text-muted);
  }
  .edit-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .edit-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  textarea.input {
    width: 100%;
    resize: vertical;
    font-size: 11px;
  }
</style>
