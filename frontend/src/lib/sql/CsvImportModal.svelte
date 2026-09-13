<script>
  import Modal from '../components/Modal.svelte';
  import { api } from '../api.js';
  import { toast, toastError, confirmDialog } from '../stores.js';
  import { quote, literal, defaultSchema } from '../sqlIdent.js';
  import { createEventDispatcher } from 'svelte';

  export let conn;
  const dispatch = createEventDispatcher();

  let fileName = '';
  let rawParsed = []; // all rows from the file, unsplit into header/data yet
  let parseError = '';
  let hasHeaderRow = true;
  let mode = 'new'; // 'new' | 'existing'
  let newTableName = '';
  let tables = []; // [{schema, table}]
  let existingTable = ''; // "schema.table"
  let existingColumns = [];
  let importing = false;
  let imported = 0;

  $: headers = hasHeaderRow ? rawParsed[0] || [] : (rawParsed[0] || []).map((_, i) => `col${i + 1}`);
  $: dataRows = hasHeaderRow ? rawParsed.slice(1) : rawParsed;
  $: preview = dataRows.slice(0, 8);

  async function loadTables() {
    try {
      const schemas = await api.schemasList(conn);
      tables = schemas.flatMap((s) => (s.relations || []).map((r) => ({ schema: s.name, table: r.name })));
    } catch {
      tables = [];
    }
  }
  $: if (conn) loadTables();

  async function loadExistingColumns() {
    const t = tables.find((x) => `${x.schema}.${x.table}` === existingTable);
    if (!t) return;
    try {
      existingColumns = await api.columnsList(conn, t.schema, t.table);
    } catch {
      existingColumns = [];
    }
  }
  $: if (mode === 'existing' && existingTable) loadExistingColumns();

  // Minimal RFC4180-ish parser: quoted fields (with "" escaping), commas,
  // and both \n and \r\n line endings.
  function parseCsv(text) {
    const out = [];
    let row = [];
    let field = '';
    let inQuotes = false;
    let i = 0;
    const n = text.length;
    while (i < n) {
      const c = text[i];
      if (inQuotes) {
        if (c === '"') {
          if (text[i + 1] === '"') {
            field += '"';
            i += 2;
            continue;
          }
          inQuotes = false;
          i++;
          continue;
        }
        field += c;
        i++;
        continue;
      }
      if (c === '"') {
        inQuotes = true;
        i++;
        continue;
      }
      if (c === ',') {
        row.push(field);
        field = '';
        i++;
        continue;
      }
      if (c === '\r') {
        i++;
        continue;
      }
      if (c === '\n') {
        row.push(field);
        out.push(row);
        row = [];
        field = '';
        i++;
        continue;
      }
      field += c;
      i++;
    }
    if (field.length || row.length) {
      row.push(field);
      out.push(row);
    }
    return out.filter((r) => !(r.length === 1 && r[0] === ''));
  }

  async function onFile(e) {
    const f = e.target.files?.[0];
    if (!f) return;
    fileName = f.name;
    parseError = '';
    try {
      const text = await f.text();
      rawParsed = parseCsv(text);
      if (!rawParsed.length) parseError = 'File is empty';
      if (!newTableName) {
        newTableName = fileName.replace(/\.csv$/i, '').replace(/[^\w]+/g, '_').toLowerCase() || 'imported';
      }
    } catch (err) {
      parseError = String(err);
    }
  }

  function inferType(values) {
    const sample = values.filter((v) => v !== '' && v != null).slice(0, 200);
    if (!sample.length) return 'text';
    if (sample.every((v) => /^-?\d+$/.test(v))) return 'integer';
    if (sample.every((v) => /^-?\d*\.?\d+([eE][+-]?\d+)?$/.test(v))) return 'real';
    if (sample.every((v) => /^(true|false)$/i.test(v))) return 'boolean';
    return 'text';
  }
  function ddlType(kind, inferred) {
    if (inferred === 'integer') return kind === 'sqlite' ? 'INTEGER' : 'BIGINT';
    if (inferred === 'real') return kind === 'mysql' ? 'DOUBLE' : kind === 'sqlite' ? 'REAL' : 'DOUBLE PRECISION';
    if (inferred === 'boolean') return kind === 'sqlite' ? 'INTEGER' : 'BOOLEAN';
    return 'TEXT';
  }
  function coerce(inferred, raw) {
    if (raw === '' || raw == null) return null;
    if (inferred === 'integer' || inferred === 'real') return Number(raw);
    if (inferred === 'boolean') return /^true$/i.test(raw);
    return raw;
  }
  $: colTypes = mode === 'new' ? headers.map((h, i) => inferType(dataRows.map((r) => r[i]))) : [];

  async function doImport() {
    if (!dataRows.length || !headers.length) {
      toast('Nothing to import', 'error');
      return;
    }
    const targetTable =
      mode === 'existing' ? existingTable.split('.').slice(1).join('.') : newTableName.trim();
    if (!targetTable) {
      toast('Table name required', 'error');
      return;
    }
    const targetSchema =
      mode === 'existing' ? existingTable.split('.')[0] : defaultSchema(conn);
    const ref = conn.kind === 'sqlite' ? quote(conn, targetTable) : `${quote(conn, targetSchema)}.${quote(conn, targetTable)}`;

    const insertCols =
      mode === 'existing'
        ? headers.filter((h) => existingColumns.some((c) => c.name.toLowerCase() === h.toLowerCase()))
        : headers;
    if (!insertCols.length) {
      toast("No CSV columns match the target table's columns", 'error', 4000);
      return;
    }
    const idxByCol = insertCols.map((h) => headers.indexOf(h));

    const ok = await confirmDialog(
      mode === 'new'
        ? `Create table "${targetTable}" and insert ${dataRows.length.toLocaleString()} row${dataRows.length === 1 ? '' : 's'} into "${conn.nickname}"?`
        : `Insert ${dataRows.length.toLocaleString()} row${dataRows.length === 1 ? '' : 's'} into "${targetTable}" on "${conn.nickname}"?`,
      { danger: true }
    );
    if (!ok) return;

    importing = true;
    imported = 0;
    try {
      if (mode === 'new') {
        const cols = headers.map((h, i) => `${quote(conn, h)} ${ddlType(conn.kind, colTypes[i])}`).join(', ');
        await api.queryRun(conn, `CREATE TABLE ${ref} (${cols})`, null, null, false);
      }
      const BATCH = 500;
      const colNames = insertCols.map((h) => quote(conn, h)).join(', ');
      for (let start = 0; start < dataRows.length; start += BATCH) {
        const batch = dataRows.slice(start, start + BATCH);
        const values = batch
          .map(
            (r) =>
              '(' +
              idxByCol
                .map((ci, k) => {
                  const raw = r[ci];
                  return literal(conn, mode === 'new' ? coerce(colTypes[k], raw) : raw === '' ? null : raw);
                })
                .join(', ') +
              ')'
          )
          .join(', ');
        await api.queryRun(conn, `INSERT INTO ${ref} (${colNames}) VALUES ${values}`, null, null, false);
        imported = Math.min(dataRows.length, start + BATCH);
      }
      toast(`Imported ${dataRows.length.toLocaleString()} rows into "${targetTable}"`, 'success', 3000);
      dispatch('imported', { schema: targetSchema, table: targetTable });
      dispatch('close');
    } catch (e) {
      toastError(e);
    } finally {
      importing = false;
    }
  }
</script>

<Modal title="Import CSV" width="620px" on:close>
  <div class="field">
    <label for="csvfile">CSV file</label>
    <input id="csvfile" type="file" accept=".csv,text/csv" on:change={onFile} disabled={importing} />
    {#if parseError}<div class="err">{parseError}</div>{/if}
  </div>

  {#if rawParsed.length}
    <label class="chk">
      <input type="checkbox" bind:checked={hasHeaderRow} disabled={importing} /> First row is column names
    </label>

    <div class="preview">
      <table>
        <thead>
          <tr>{#each headers as h}<th>{h}</th>{/each}</tr>
        </thead>
        <tbody>
          {#each preview as row}
            <tr>{#each headers as _, i}<td>{row[i] ?? ''}</td>{/each}</tr>
          {/each}
        </tbody>
      </table>
      <div class="rowcount">{dataRows.length.toLocaleString()} data row{dataRows.length === 1 ? '' : 's'}</div>
    </div>

    <div class="field">
      <label>Import into</label>
      <div class="mode-row">
        <label class="chk"><input type="radio" bind:group={mode} value="new" disabled={importing} /> New table</label>
        <label class="chk"><input type="radio" bind:group={mode} value="existing" disabled={importing} /> Existing table</label>
      </div>
    </div>

    {#if mode === 'new'}
      <div class="field">
        <label for="tname">Table name</label>
        <input id="tname" class="input mono" bind:value={newTableName} disabled={importing} />
      </div>
      <div class="types">
        {#each headers as h, i}
          <span class="type-chip">{h}: <b>{ddlType(conn.kind, colTypes[i])}</b></span>
        {/each}
      </div>
    {:else}
      <div class="field">
        <label for="ttable">Target table</label>
        <select id="ttable" class="select" bind:value={existingTable} disabled={importing}>
          <option value="">Choose a table…</option>
          {#each tables as t}
            <option value={`${t.schema}.${t.table}`}>{t.schema}.{t.table}</option>
          {/each}
        </select>
      </div>
      {#if existingTable && existingColumns.length}
        {@const matched = headers.filter((h) => existingColumns.some((c) => c.name.toLowerCase() === h.toLowerCase()))}
        <div class="types">
          {#each headers as h}
            <span class="type-chip" class:bad={!matched.includes(h)}>
              {h}{matched.includes(h) ? '' : ' (no matching column — skipped)'}
            </span>
          {/each}
        </div>
      {/if}
    {/if}

    {#if importing}
      <div class="progress">Importing… {imported.toLocaleString()} / {dataRows.length.toLocaleString()}</div>
    {/if}
  {/if}

  <svelte:fragment slot="footer">
    <span style="flex:1" />
    <button class="btn" on:click={() => dispatch('close')} disabled={importing}>Cancel</button>
    <button class="btn primary" on:click={doImport} disabled={importing || !dataRows.length}>
      {importing ? 'Importing…' : 'Import'}
    </button>
  </svelte:fragment>
</Modal>

<style>
  .field {
    margin-bottom: 12px;
  }
  .field label {
    display: block;
    font-size: 11px;
    color: var(--text-secondary);
    margin-bottom: 4px;
  }
  .err {
    color: var(--danger);
    font-size: 11px;
    margin-top: 4px;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    margin-bottom: 10px;
  }
  .mode-row {
    display: flex;
    gap: 16px;
  }
  .preview {
    margin-bottom: 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: auto;
    max-height: 220px;
  }
  .preview table {
    border-collapse: collapse;
    font-family: var(--font-mono);
    font-size: 11px;
    width: 100%;
  }
  .preview th,
  .preview td {
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    border-right: 1px solid var(--border);
    white-space: nowrap;
    text-align: left;
  }
  .preview thead th {
    position: sticky;
    top: 0;
    background: var(--surface-1);
  }
  .rowcount {
    padding: 4px 8px;
    font-size: 10.5px;
    color: var(--text-muted);
    background: var(--surface-1);
  }
  .types {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 8px;
  }
  .type-chip {
    font-size: 10.5px;
    font-family: var(--font-mono);
    background: var(--surface-3);
    color: var(--text-secondary);
    padding: 2px 7px;
    border-radius: 4px;
  }
  .type-chip.bad {
    color: var(--danger);
  }
  .progress {
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 6px;
  }
</style>
