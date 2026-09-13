<script>
  import Modal from '../components/Modal.svelte';
  import { api } from '../api.js';
  import { toast, toastError, confirmDialog } from '../stores.js';
  import { quote, literal } from '../sqlIdent.js';
  import { ICONS } from '../icons.js';
  import { createEventDispatcher, onMount } from 'svelte';

  export let conn;
  export let schema;
  export let table;
  const dispatch = createEventDispatcher();

  // SQLite's ALTER TABLE only supports adding/dropping/renaming a column
  // (and renaming the table) — no in-place type/nullable/default change
  // without a full rebuild (new table + copy + swap), which this doesn't
  // attempt. Postgres and MySQL support the full set.
  $: limited = conn?.kind === 'sqlite';

  let loading = true;
  let original = []; // Column[] as loaded
  let rows = []; // working copy: { key, origName, name, data_type, nullable, default, primary_key, isNew }
  let dropped = new Set(); // origName set
  let tab = 'structure'; // 'structure' | 'ddl'
  let saving = false;
  let seq = 0;

  onMount(load);
  async function load() {
    loading = true;
    try {
      original = await api.columnsList(conn, schema, table);
      rows = original.map((c) => ({ key: `k${seq++}`, origName: c.name, ...c }));
    } catch (e) {
      toastError(e);
    } finally {
      loading = false;
    }
  }

  function addRow() {
    rows = [
      ...rows,
      { key: `k${seq++}`, origName: null, name: '', data_type: 'text', nullable: true, default: null, primary_key: false, isNew: true }
    ];
  }
  function toggleDrop(r) {
    if (r.isNew) {
      rows = rows.filter((x) => x !== r);
      return;
    }
    const s = new Set(dropped);
    s.has(r.origName) ? s.delete(r.origName) : s.add(r.origName);
    dropped = s;
  }

  const ref = () =>
    conn.kind === 'sqlite' ? quote(conn, table) : `${quote(conn, schema)}.${quote(conn, table)}`;

  function colDdl(c) {
    let s = `${quote(conn, c.name)} ${c.data_type}`;
    if (!c.nullable) s += ' NOT NULL';
    if (c.default) s += ` DEFAULT ${c.default}`;
    return s;
  }

  $: ddlText = (() => {
    const cols = rows.filter((r) => !dropped.has(r.origName)).map(colDdl);
    const pk = rows.filter((r) => r.primary_key && !dropped.has(r.origName)).map((r) => quote(conn, r.name));
    const lines = [...cols];
    if (pk.length) lines.push(`PRIMARY KEY (${pk.join(', ')})`);
    return `CREATE TABLE ${ref()} (\n  ${lines.join(',\n  ')}\n);`;
  })();

  function buildAlterStatements() {
    const stmts = [];
    for (const origName of dropped) {
      stmts.push(`ALTER TABLE ${ref()} DROP COLUMN ${quote(conn, origName)}`);
    }
    for (const r of rows) {
      if (dropped.has(r.origName)) continue;
      if (r.isNew) {
        if (!r.name.trim()) continue;
        let s = `ALTER TABLE ${ref()} ADD COLUMN ${quote(conn, r.name)} ${r.data_type}`;
        if (!r.nullable) s += ' NOT NULL';
        if (r.default) s += ` DEFAULT ${r.default}`;
        stmts.push(s);
        continue;
      }
      const orig = original.find((c) => c.name === r.origName);
      if (!orig) continue;
      if (r.name !== r.origName && r.name.trim()) {
        stmts.push(`ALTER TABLE ${ref()} RENAME COLUMN ${quote(conn, r.origName)} TO ${quote(conn, r.name)}`);
      }
      if (limited) continue; // sqlite: rename only, already handled above
      const curName = quote(conn, r.name || r.origName);
      const typeChanged = r.data_type !== orig.data_type;
      const nullChanged = r.nullable !== orig.nullable;
      const defaultChanged = (r.default || '') !== (orig.default || '');
      if (conn.kind === 'mysql') {
        if (typeChanged || nullChanged) {
          let s = `ALTER TABLE ${ref()} MODIFY COLUMN ${curName} ${r.data_type}`;
          if (!r.nullable) s += ' NOT NULL';
          stmts.push(s);
        }
        if (defaultChanged) {
          stmts.push(
            r.default
              ? `ALTER TABLE ${ref()} ALTER COLUMN ${curName} SET DEFAULT ${r.default}`
              : `ALTER TABLE ${ref()} ALTER COLUMN ${curName} DROP DEFAULT`
          );
        }
      } else {
        // postgres
        if (typeChanged) {
          stmts.push(`ALTER TABLE ${ref()} ALTER COLUMN ${curName} TYPE ${r.data_type} USING ${curName}::${r.data_type}`);
        }
        if (nullChanged) {
          stmts.push(`ALTER TABLE ${ref()} ALTER COLUMN ${curName} ${r.nullable ? 'DROP' : 'SET'} NOT NULL`);
        }
        if (defaultChanged) {
          stmts.push(
            r.default
              ? `ALTER TABLE ${ref()} ALTER COLUMN ${curName} SET DEFAULT ${r.default}`
              : `ALTER TABLE ${ref()} ALTER COLUMN ${curName} DROP DEFAULT`
          );
        }
      }
    }
    return stmts;
  }

  async function save() {
    const stmts = buildAlterStatements();
    if (!stmts.length) {
      toast('No changes to apply', 'success', 1800);
      return;
    }
    const preview = stmts.join(';\n');
    const ok = await confirmDialog(
      `Run ${stmts.length} statement${stmts.length === 1 ? '' : 's'} against "${conn.nickname}"?\n\n${preview}`,
      { danger: true }
    );
    if (!ok) return;
    saving = true;
    try {
      for (let i = 0; i < stmts.length; i++) {
        await api.queryRun(conn, stmts[i], null, null, false);
      }
      toast(`Applied ${stmts.length} statement${stmts.length === 1 ? '' : 's'}`, 'success', 2500);
      dispatch('changed');
      await load();
      dropped = new Set();
    } catch (e) {
      toastError(e);
    } finally {
      saving = false;
    }
  }
</script>

<Modal title="Table structure — {schema}.{table}" width="720px" on:close>
  <div class="tabs">
    <button class="tabbtn" class:on={tab === 'structure'} on:click={() => (tab = 'structure')}>Structure</button>
    <button class="tabbtn" class:on={tab === 'ddl'} on:click={() => (tab = 'ddl')}>DDL</button>
    <span style="flex:1" />
    {#if limited}<span class="hint">SQLite: rename/add/drop only — type &amp; default changes need a table rebuild</span>{/if}
  </div>

  {#if loading}
    <div class="empty">Loading…</div>
  {:else if tab === 'ddl'}
    <pre class="ddl">{ddlText}</pre>
    <button class="btn ghost sm" on:click={() => navigator.clipboard?.writeText(ddlText)}>Copy</button>
  {:else}
    <table class="cols">
      <thead>
        <tr>
          <th>Name</th>
          <th>Type</th>
          <th>Null</th>
          <th>Default</th>
          <th>PK</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each rows as r (r.key)}
          <tr class:drop={dropped.has(r.origName)}>
            <td><input class="cell" bind:value={r.name} placeholder="column_name" /></td>
            <td><input class="cell mono" bind:value={r.data_type} disabled={limited && !r.isNew} /></td>
            <td class="center"><input type="checkbox" bind:checked={r.nullable} disabled={limited && !r.isNew} /></td>
            <td><input class="cell mono" bind:value={r.default} placeholder="—" disabled={limited && !r.isNew} /></td>
            <td class="center">{#if r.primary_key}{@html ICONS.check.svg}{/if}</td>
            <td>
              <button class="btn ghost sm" on:click={() => toggleDrop(r)}>
                {#if dropped.has(r.origName)}{@html ICONS.revert.svg} Undo
                {:else if r.isNew}{@html ICONS.cancel.svg}
                {:else}{@html ICONS.delete.svg} Drop{/if}
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
    <button class="btn ghost sm" on:click={addRow}>+ Add column</button>
  {/if}

  <svelte:fragment slot="footer">
    <span style="flex:1" />
    <button class="btn" on:click={() => dispatch('close')} disabled={saving}>Close</button>
    {#if tab === 'structure' && !conn.read_only}
      <button class="btn primary" on:click={save} disabled={saving || loading}>{saving ? 'Applying…' : 'Save changes'}</button>
    {:else if tab === 'structure'}
      <span class="hint">Read-only connection — structure changes disabled</span>
    {/if}
  </svelte:fragment>
</Modal>

<style>
  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 10px;
  }
  .tabbtn {
    all: unset;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 10px;
    border-radius: 5px;
    color: var(--text-secondary);
  }
  .tabbtn.on {
    background: var(--tool-sql-tint);
    color: var(--tool-sql-text);
  }
  .hint {
    font-size: 10.5px;
    color: var(--text-muted);
  }
  .empty {
    padding: 30px;
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
  }
  .ddl {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 12px;
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: pre-wrap;
    margin-bottom: 8px;
    max-height: 320px;
    overflow: auto;
  }
  table.cols {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
    margin-bottom: 10px;
  }
  table.cols th {
    text-align: left;
    font-size: 10.5px;
    color: var(--text-muted);
    padding: 4px 6px;
    border-bottom: 1px solid var(--border);
  }
  table.cols td {
    padding: 3px 6px;
    border-bottom: 1px solid var(--border);
  }
  table.cols tr.drop td {
    opacity: 0.4;
    text-decoration: line-through;
  }
  .cell {
    width: 100%;
    font: inherit;
    padding: 3px 6px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--text-primary);
  }
  .cell.mono {
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .center {
    text-align: center;
  }
</style>
