<script>
  import Modal from '../components/Modal.svelte';
  import { api } from '../api.js';
  import { ICONS } from '../icons.js';
  import { downloadText } from '../export.js';
  import { toPostmanEnvironment } from './postman.js';
  import {
    environments,
    requestGlobals,
    saveGlobals,
    reloadRequests,
    toast,
    toastError,
    confirmDialog
  } from '../stores.js';

  const GLOBALS_KEY = '__globals__';

  $: list = $environments;

  // Clicking a name expands it in place; the vars underneath are edited
  // right there instead of jumping to a separate edit screen. `drafts`
  // holds an in-progress edit per env id, seeded when it's expanded.
  let expandedId = null;
  let drafts = {};

  function draftFromGlobals() {
    return {
      name: 'Globals',
      vars: Object.entries($requestGlobals)
        .map(([k, v]) => ({ k, v: String(v) }))
        .concat({ k: '', v: '' })
    };
  }
  function draftFromEnv(env) {
    const obj = JSON.parse(env.variables_json || '{}');
    return {
      name: env.name,
      is_active: env.is_active,
      vars: Object.entries(obj)
        .map(([k, v]) => ({ k, v: String(v) }))
        .concat({ k: '', v: '' })
    };
  }

  function toggle(id, seed) {
    if (expandedId === id) {
      expandedId = null;
      return;
    }
    expandedId = id;
    if (!drafts[id]) drafts = { ...drafts, [id]: seed() };
  }
  function addRow(id) {
    drafts[id].vars = [...drafts[id].vars, { k: '', v: '' }];
    drafts = drafts;
  }
  function removeRow(id, i) {
    drafts[id].vars = drafts[id].vars.filter((_, idx) => idx !== i);
    drafts = drafts;
  }

  function startNew() {
    const id = '';
    drafts = {
      ...drafts,
      [id]: { name: 'New environment', is_active: list.length === 0, vars: [{ k: '', v: '' }] }
    };
    expandedId = id;
  }

  async function save(id) {
    const draft = drafts[id];
    const variables = {};
    for (const { k, v } of draft.vars) if (k.trim()) variables[k.trim()] = v;
    try {
      if (id === GLOBALS_KEY) {
        await saveGlobals(variables);
        toast('Globals saved', 'success');
      } else {
        await api.environmentSave({
          id,
          name: draft.name.trim() || 'Environment',
          variables_json: JSON.stringify(variables),
          is_active: draft.is_active
        });
        await reloadRequests();
        toast('Environment saved', 'success');
      }
      const { [id]: _, ...rest } = drafts;
      drafts = rest;
      expandedId = null;
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
    if (!(await confirmDialog(`Delete environment "${env.name}"?`, { danger: true }))) return;
    try {
      await api.environmentDelete(env.id);
      if (expandedId === env.id) expandedId = null;
      await reloadRequests();
    } catch (e) {
      toastError(e);
    }
  }
  function exportEnv(name, variables) {
    const pm = toPostmanEnvironment(name, variables);
    const file = (name || 'environment').replace(/[^\w.-]+/g, '_').slice(0, 60) || 'environment';
    downloadText(`${file}.postman_environment.json`, JSON.stringify(pm, null, 2), 'application/json');
    toast('Exported as a Postman environment', 'success', 2000);
  }
</script>

<Modal title="Environments" width="560px" on:close>
  <div class="list">
    <div class="env-block">
      <div class="env globals-row" on:click={() => toggle(GLOBALS_KEY, draftFromGlobals)}>
        <span class="chevron">{expandedId === GLOBALS_KEY ? ICONS.expandOpen.glyph : ICONS.expandClosed.glyph}</span>
        <span class="radio on">★</span>
        <span class="name">Globals</span>
        <span class="cnt">{Object.keys($requestGlobals).length} vars · always on</span>
        <button
          class="icon-btn sm"
          title="Export as Postman environment"
          on:click|stopPropagation={() => exportEnv('Globals', $requestGlobals)}
          >{ICONS.exportPostman.glyph}</button
        >
      </div>
      {#if expandedId === GLOBALS_KEY && drafts[GLOBALS_KEY]}
        <div class="editor">
          <p class="hint" style="margin-top:0">
            Globals apply to every request. The active environment overrides a global with the same name.
          </p>
          <div class="vars">
            <div class="vh"><span>Variable</span><span>Value</span><span /></div>
            {#each drafts[GLOBALS_KEY].vars as row, i}
              <div class="vr">
                <input class="input mono" placeholder="baseUrl" bind:value={row.k} />
                <input class="input mono" placeholder="https://api.example.com" bind:value={row.v} />
                <button class="icon-btn sm" title="Remove row" on:click={() => removeRow(GLOBALS_KEY, i)}
                  >{ICONS.delete.glyph}</button
                >
              </div>
            {/each}
            <button class="btn ghost sm" on:click={() => addRow(GLOBALS_KEY)}>+ Row</button>
          </div>
          <div class="editor-actions">
            <button class="btn" on:click={() => (expandedId = null)}>Cancel</button>
            <button class="btn primary" on:click={() => save(GLOBALS_KEY)}>Save globals</button>
          </div>
        </div>
      {/if}
    </div>

    {#each list as env (env.id)}
      <div class="env-block">
        <div class="env" on:click={() => toggle(env.id, () => draftFromEnv(env))}>
          <span class="chevron">{expandedId === env.id ? ICONS.expandOpen.glyph : ICONS.expandClosed.glyph}</span>
          <button
            class="radio"
            class:on={env.is_active}
            on:click|stopPropagation={() => activate(env)}
            title="Set active"
          >
            {env.is_active ? '●' : '○'}
          </button>
          <span class="name">{env.name}</span>
          <span class="cnt">{Object.keys(JSON.parse(env.variables_json || '{}')).length} vars</span>
          <button
            class="icon-btn sm"
            title="Export as Postman environment"
            on:click|stopPropagation={() => exportEnv(env.name, JSON.parse(env.variables_json || '{}'))}
            >{ICONS.exportPostman.glyph}</button
          >
          <button class="icon-btn sm danger" on:click|stopPropagation={() => remove(env)}
            >{ICONS.delete.glyph}</button
          >
        </div>
        {#if expandedId === env.id && drafts[env.id]}
          <div class="editor">
            <div class="field">
              <label for="en-{env.id}">Name</label>
              <input id="en-{env.id}" class="input" bind:value={drafts[env.id].name} />
            </div>
            <label class="act">
              <input type="checkbox" bind:checked={drafts[env.id].is_active} /> Active
            </label>
            <div class="vars">
              <div class="vh"><span>Variable</span><span>Value</span><span /></div>
              {#each drafts[env.id].vars as row, i}
                <div class="vr">
                  <input class="input mono" placeholder="baseUrl" bind:value={row.k} />
                  <input class="input mono" placeholder="https://api.example.com" bind:value={row.v} />
                  <button class="icon-btn sm" title="Remove row" on:click={() => removeRow(env.id, i)}
                    >{ICONS.delete.glyph}</button
                  >
                </div>
              {/each}
              <button class="btn ghost sm" on:click={() => addRow(env.id)}>+ Row</button>
            </div>
            <p class="hint">Use <code>{'{{baseUrl}}'}</code> in URL, headers, or body.</p>
            <div class="editor-actions">
              <button class="btn" on:click={() => (expandedId = null)}>Cancel</button>
              <button class="btn primary" on:click={() => save(env.id)}>Save</button>
            </div>
          </div>
        {/if}
      </div>
    {/each}

    {#if expandedId === ''}
      <div class="env-block">
        <div class="editor new">
          <div class="field">
            <label for="en-new">Name</label>
            <input id="en-new" class="input" bind:value={drafts[''].name} />
          </div>
          <label class="act">
            <input type="checkbox" bind:checked={drafts[''].is_active} /> Active
          </label>
          <div class="vars">
            <div class="vh"><span>Variable</span><span>Value</span><span /></div>
            {#each drafts[''].vars as row, i}
              <div class="vr">
                <input class="input mono" placeholder="baseUrl" bind:value={row.k} />
                <input class="input mono" placeholder="https://api.example.com" bind:value={row.v} />
                <button class="icon-btn sm" title="Remove row" on:click={() => removeRow('', i)}
                  >{ICONS.delete.glyph}</button
                >
              </div>
            {/each}
            <button class="btn ghost sm" on:click={() => addRow('')}>+ Row</button>
          </div>
          <div class="editor-actions">
            <button class="btn" on:click={() => { const { '': _, ...rest } = drafts; drafts = rest; expandedId = null; }}
              >Cancel</button
            >
            <button class="btn primary" on:click={() => save('')}>Save</button>
          </div>
        </div>
      </div>
    {/if}

    {#if list.length === 0}<div class="muted">No environments yet.</div>{/if}
  </div>

  <svelte:fragment slot="footer">
    <button class="btn primary" on:click={startNew} disabled={expandedId === ''}>New environment</button>
  </svelte:fragment>
</Modal>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .env-block {
    border-bottom: 1px solid var(--border);
  }
  .env-block:last-child {
    border-bottom: none;
  }
  .env {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 4px;
    cursor: pointer;
  }
  .env:hover {
    background: var(--surface-3);
  }
  .chevron {
    font-size: 9px;
    color: var(--text-muted);
    width: 10px;
    text-align: center;
    flex-shrink: 0;
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
  .editor {
    padding: 4px 8px 12px 22px;
  }
  .editor.new {
    padding-left: 8px;
    padding-top: 10px;
  }
  .editor-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 10px;
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
    grid-template-columns: 1fr 1.4fr auto;
    gap: 6px;
    align-items: center;
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
