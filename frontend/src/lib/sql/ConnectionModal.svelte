<script>
  import Modal from '../components/Modal.svelte';
  import { api } from '../api.js';
  import { reloadConnections, toast, toastError, confirmDialog } from '../stores.js';
  import { createEventDispatcher } from 'svelte';

  export let existing = null;
  const dispatch = createEventDispatcher();

  const DOT_COLORS = [
    'var(--conn-red)', 'var(--conn-amber)', 'var(--conn-teal)', 'var(--conn-pink)',
    'var(--conn-blue)', 'var(--conn-purple)', 'var(--conn-slate)', 'var(--conn-lime)'
  ];

  let form = existing
    ? {
        id: existing.id,
        nickname: existing.nickname,
        kind: existing.kind,
        host: existing.host || '',
        port: existing.port || null,
        database: existing.database || '',
        user: existing.user || '',
        file_path: existing.file_path || '',
        use_tls: !!existing.use_tls,
        color: existing.color || DOT_COLORS[6],
        read_only: !!existing.read_only
      }
    : {
        id: '',
        nickname: '',
        kind: 'postgres',
        host: 'localhost',
        port: 5432,
        database: '',
        user: '',
        file_path: '',
        use_tls: false,
        color: DOT_COLORS[0],
        read_only: false
      };
  let password = '';
  let testing = false;
  let saving = false;
  let testResult = null;

  $: isSqlite = form.kind === 'sqlite';
  $: if (form.kind === 'postgres' && !form.port) form.port = 5432;

  function kindChanged() {
    if (form.kind === 'postgres') form.port = 5432;
    else if (form.kind === 'mysql') form.port = 3306;
    testResult = null;
  }

  function payload() {
    return {
      id: form.id,
      nickname: form.nickname.trim() || 'Untitled',
      kind: form.kind,
      host: isSqlite ? null : form.host || null,
      port: isSqlite ? null : form.port ? Number(form.port) : null,
      database: isSqlite ? null : form.database || null,
      user: isSqlite ? null : form.user || null,
      file_path: isSqlite ? form.file_path || null : null,
      use_tls: isSqlite ? false : form.use_tls,
      color: form.color,
      read_only: form.read_only
    };
  }

  async function test() {
    testing = true;
    testResult = null;
    try {
      const info = await api.connectionTest(payload(), password || null);
      testResult = { ok: true, text: `${info.kind} — ${info.version}` };
    } catch (e) {
      testResult = { ok: false, text: String(e) };
    } finally {
      testing = false;
    }
  }

  async function save() {
    saving = true;
    try {
      const saved = await api.connectionSave(payload(), password ? password : existing ? null : '');
      await reloadConnections();
      toast('Connection saved', 'success');
      dispatch('saved', saved);
    } catch (e) {
      toastError(e);
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!existing) return;
    if (!(await confirmDialog(`Delete connection "${existing.nickname}"?`, { danger: true }))) return;
    try {
      await api.connectionDelete(existing.id);
      await reloadConnections();
      toast('Connection deleted', 'success');
      dispatch('deleted', existing.id);
    } catch (e) {
      toastError(e);
    }
  }
</script>

<Modal title={existing ? 'Edit connection' : 'New connection'} width="480px" on:close>
  <div class="field">
    <label for="nick">Name</label>
    <input id="nick" class="input" bind:value={form.nickname} placeholder="PROD" />
  </div>

  <div class="row">
    <div class="field">
      <label for="kind">Engine</label>
      <select id="kind" class="select" bind:value={form.kind} on:change={kindChanged}>
        <option value="postgres">PostgreSQL</option>
        <option value="mysql">MySQL / MariaDB</option>
        <option value="sqlite">SQLite</option>
      </select>
    </div>
    <div class="field">
      <label>Accent</label>
      <div class="swatches">
        {#each DOT_COLORS as c}
          <button
            class="sw"
            class:sel={form.color === c}
            style="background:{c}"
            on:click={() => (form.color = c)}
            aria-label="color"
          />
        {/each}
      </div>
    </div>
  </div>

  {#if isSqlite}
    <div class="field">
      <label for="fp">Database file path</label>
      <input id="fp" class="input mono" bind:value={form.file_path} placeholder="/path/to/app.db" />
    </div>
  {:else}
    <div class="row">
      <div class="field" style="flex:2">
        <label for="host">Host</label>
        <input id="host" class="input" bind:value={form.host} />
      </div>
      <div class="field" style="flex:1">
        <label for="port">Port</label>
        <input id="port" class="input" type="number" bind:value={form.port} />
      </div>
    </div>
    <div class="row">
      <div class="field">
        <label for="db">Database</label>
        <input id="db" class="input" bind:value={form.database} />
      </div>
      <div class="field">
        <label for="user">User</label>
        <input id="user" class="input" bind:value={form.user} />
      </div>
    </div>
    <div class="field">
      <label for="pw">Password {existing ? '(leave blank to keep)' : ''}</label>
      <input id="pw" class="input" type="password" bind:value={password} autocomplete="off" />
    </div>
    <label class="tls">
      <input type="checkbox" bind:checked={form.use_tls} /> Require TLS
    </label>
  {/if}

  <label class="tls" title="Blocks every write statement on this connection app-wide — the query editor, MCP, and the scheduler all refuse anything but SELECT/row-returning statements.">
    <input type="checkbox" bind:checked={form.read_only} /> Read-only (block all writes)
  </label>

  {#if testResult}
    <div class="test {testResult.ok ? 'ok' : 'bad'}">{testResult.text}</div>
  {/if}

  <svelte:fragment slot="footer">
    {#if existing}
      <button class="btn danger" on:click={remove}>Delete</button>
    {/if}
    <span style="flex:1" />
    <button class="btn" on:click={test} disabled={testing}>{testing ? 'Testing…' : 'Test'}</button>
    <button class="btn primary" on:click={save} disabled={saving}>{saving ? 'Saving…' : 'Save'}</button>
  </svelte:fragment>
</Modal>

<style>
  .swatches {
    display: flex;
    gap: 5px;
    flex-wrap: wrap;
  }
  .sw {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 2px solid transparent;
    cursor: pointer;
  }
  .sw.sel {
    border-color: var(--text-primary);
  }
  .tls {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .test {
    font-size: 11px;
    font-family: var(--font-mono);
    padding: 8px;
    border-radius: 5px;
    word-break: break-word;
    margin-top: 4px;
  }
  .test.ok {
    background: color-mix(in srgb, var(--ok) 15%, transparent);
    color: var(--ok);
  }
  .test.bad {
    background: color-mix(in srgb, var(--danger) 15%, transparent);
    color: var(--danger);
  }
  footer span {
    flex: 1;
  }
</style>
