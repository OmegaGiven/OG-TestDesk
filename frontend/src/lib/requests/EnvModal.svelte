<script>
  import Modal from '../components/Modal.svelte';
  import { api } from '../api.js';
  import { ICONS } from '../icons.js';
  import {
    environments,
    requestGlobals,
    saveGlobals,
    reloadRequests,
    toast,
    toastError
  } from '../stores.js';

  let editing = null; // {id, name, vars:[{k,v}], is_active} | {globals:true, vars}

  $: list = $environments;

  function editGlobals() {
    editing = {
      globals: true,
      name: 'Globals',
      vars: Object.entries($requestGlobals).map(([k, v]) => ({ k, v: String(v) })).concat({ k: '', v: '' })
    };
  }

  function startNew() {
    editing = { id: '', name: 'New environment', vars: [{ k: '', v: '' }], is_active: list.length === 0 };
  }
  function edit(env) {
    const obj = JSON.parse(env.variables_json || '{}');
    editing = {
      id: env.id,
      name: env.name,
      is_active: env.is_active,
      vars: Object.entries(obj).map(([k, v]) => ({ k, v: String(v) })).concat({ k: '', v: '' })
    };
  }
  function addRow() {
    editing.vars = [...editing.vars, { k: '', v: '' }];
  }
  async function save() {
    const variables = {};
    for (const { k, v } of editing.vars) if (k.trim()) variables[k.trim()] = v;
    try {
      if (editing.globals) {
        await saveGlobals(variables);
        toast('Globals saved', 'success');
      } else {
        await api.environmentSave({
          id: editing.id,
          name: editing.name.trim() || 'Environment',
          variables_json: JSON.stringify(variables),
          is_active: editing.is_active
        });
        await reloadRequests();
        toast('Environment saved', 'success');
      }
      editing = null;
    } catch (e) {
      toastError(e);
    }
  }
  async function activate(env) {
    try {
      await api.environmentSave({ ...env, is_active: true });
      await reloadRequests();
    } catch (e) {
      toastError(e);
    }
  }
  async function remove(env) {
    if (!confirm(`Delete environment "${env.name}"?`)) return;
    try {
      await api.environmentDelete(env.id);
      await reloadRequests();
    } catch (e) {
      toastError(e);
    }
  }
</script>

<Modal title="Environments" width="520px" on:close>
  {#if !editing}
    <div class="list">
      <div class="env globals-row">
        <span class="radio on">★</span>
        <span class="name">Globals</span>
        <span class="cnt">{Object.keys($requestGlobals).length} vars · always on</span>
        <button class="btn ghost sm" on:click={editGlobals}>Edit</button>
      </div>
      {#each list as env (env.id)}
        <div class="env">
          <button class="radio" class:on={env.is_active} on:click={() => activate(env)} title="Set active">
            {env.is_active ? '●' : '○'}
          </button>
          <span class="name">{env.name}</span>
          <span class="cnt">{Object.keys(JSON.parse(env.variables_json || '{}')).length} vars</span>
          <button class="btn ghost sm" on:click={() => edit(env)}>Edit</button>
          <button class="icon-btn sm danger" on:click={() => remove(env)}>{ICONS.delete.glyph}</button>
        </div>
      {/each}
      {#if list.length === 0}<div class="muted">No environments yet.</div>{/if}
    </div>
  {:else}
    {#if !editing.globals}
      <div class="field">
        <label for="en">Name</label>
        <input id="en" class="input" bind:value={editing.name} />
      </div>
      <label class="act"><input type="checkbox" bind:checked={editing.is_active} /> Active</label>
    {:else}
      <p class="hint" style="margin-top:0">
        Globals apply to every request. The active environment overrides a global with the same name.
      </p>
    {/if}
    <div class="vars">
      <div class="vh"><span>Variable</span><span>Value</span></div>
      {#each editing.vars as row}
        <div class="vr">
          <input class="input mono" placeholder="baseUrl" bind:value={row.k} />
          <input class="input mono" placeholder="https://api.example.com" bind:value={row.v} />
        </div>
      {/each}
      <button class="btn ghost sm" on:click={addRow}>+ Row</button>
    </div>
    <p class="hint">Use <code>{'{{baseUrl}}'}</code> in URL, headers, or body.</p>
  {/if}

  <svelte:fragment slot="footer">
    {#if !editing}
      <button class="btn primary" on:click={startNew}>New environment</button>
    {:else}
      <button class="btn" on:click={() => (editing = null)}>Cancel</button>
      <button class="btn primary" on:click={save}>
        {editing.globals ? 'Save globals' : 'Save'}
      </button>
    {/if}
  </svelte:fragment>
</Modal>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .env {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 4px;
    border-bottom: 1px solid var(--border);
  }
  .radio {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--tool-requests-text);
    font-size: 12px;
  }
  .name {
    flex: 1;
    font-weight: 600;
    font-size: 12px;
  }
  .cnt {
    font-size: 10px;
    color: var(--text-muted);
  }
  .act {
    display: flex;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    margin-bottom: 10px;
  }
  .vars {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .vh,
  .vr {
    display: grid;
    grid-template-columns: 1fr 1.4fr;
    gap: 6px;
  }
  .vh span {
    font-size: 10px;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .hint {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 10px;
  }
  .danger {
    color: var(--danger);
  }
</style>
