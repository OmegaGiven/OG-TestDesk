<script>
  import { onMount, tick } from 'svelte';
  import CodeEditor from '../components/CodeEditor.svelte';
  import EnvModal from './EnvModal.svelte';
  import { api } from '../api.js';
  import { parsePostman, toPostmanCollection } from './postman.js';
  import { ICONS } from '../icons.js';
  import { downloadText, copyText, toCurl, rowsToDelimited } from '../export.js';
  import {
    requestCollections,
    savedRequests,
    activeEnvironment,
    reloadRequests,
    toast,
    toastError,
    sendToInspector,
    historyLoad,
    requestTabs,
    activeRequestTab,
    activeRequestTabId,
    newRequestTab,
    touchRequestTab,
    persistRequestTab
  } from '../stores.js';

  const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];

  let draft = blank();
  let tab = 'params'; // params | headers | body
  let response = null;
  let sending = false;
  let error = null;
  let respTab = 'body'; // body | headers
  let envModal = false;
  let splitPct = 50;
  let dragging = false;

  function blank() {
    return {
      id: '',
      name: 'Untitled request',
      method: 'GET',
      url: '',
      headers: [{ k: '', v: '', on: true }],
      params: [{ k: '', v: '', on: true }],
      body: '',
      collection_id: null
    };
  }

  // ---- draft <-> active request tab ----------------------------------------
  let loadedTabId = null;
  let flushTimer;

  $: if ($activeRequestTab && $activeRequestTab.id !== loadedTabId) {
    hydrateDraft($activeRequestTab);
  }

  let hydrating = false;
  function hydrateDraft(t) {
    hydrating = true;
    loadedTabId = t.id;
    const headersObj = safeJson(t.headers_json);
    draft = {
      id: t.saved_request_id || '',
      name: t.title || 'Untitled request',
      method: t.method || 'GET',
      url: t.url || '',
      headers: ensureTrailingRow(
        Object.entries(headersObj).map(([k, v]) => ({ k, v: String(v), on: true }))
      ),
      params: [{ k: '', v: '', on: true }],
      body: t.body || '',
      collection_id: null
    };
    urlToParams();
    if (draft.body) tab = 'body';
    response = t.response ?? null;
    error = t.error ?? null;
    sending = t.sending ?? false;
    setTimeout(() => (hydrating = false), 0);
  }

  function safeJson(s) {
    try {
      return JSON.parse(s || '{}');
    } catch {
      return {};
    }
  }

  function headersObject() {
    const h = {};
    for (const row of draft.headers) if (row.on && row.k.trim()) h[row.k.trim()] = row.v;
    return h;
  }

  // Whenever the draft changes, sync it back to the tab (debounced).
  $: draft, scheduleFlush();
  function scheduleFlush() {
    if (!loadedTabId || hydrating) return;
    clearTimeout(flushTimer);
    flushTimer = setTimeout(() => {
      touchRequestTab(loadedTabId, {
        title: draft.name,
        method: draft.method,
        url: draft.url,
        headers_json: JSON.stringify(headersObject()),
        body: draft.body || null,
        saved_request_id: draft.id || null,
        dirty: true
      });
      persistRequestTab(loadedTabId);
    }, 400);
  }

  $: methodColor = `var(--m-${draft.method.toLowerCase()})`;

  // params <-> url sync
  let syncing = false;
  function paramsToUrl() {
    if (syncing) return;
    syncing = true;
    const base = draft.url.split('?')[0];
    const qs = draft.params
      .filter((p) => p.on && p.k)
      .map((p) => `${encodeURIComponent(p.k)}=${encodeURIComponent(p.v)}`)
      .join('&');
    draft.url = qs ? `${base}?${qs}` : base;
    syncing = false;
  }
  function urlToParams() {
    if (syncing) return;
    syncing = true;
    const q = draft.url.split('?')[1] || '';
    const parsed = q
      .split('&')
      .filter(Boolean)
      .map((pair) => {
        const [k, v = ''] = pair.split('=');
        return { k: decodeURIComponent(k), v: decodeURIComponent(v), on: true };
      });
    draft.params = [...parsed, { k: '', v: '', on: true }];
    syncing = false;
  }

  function ensureTrailingRow(arr) {
    if (!arr.length || arr[arr.length - 1].k !== '' || arr[arr.length - 1].v !== '') {
      arr.push({ k: '', v: '', on: true });
    }
    return arr;
  }
  function onHeaderInput() {
    draft.headers = ensureTrailingRow([...draft.headers]);
  }
  function onParamInput() {
    draft.params = ensureTrailingRow([...draft.params]);
    paramsToUrl();
  }

  // ---- Auth tab — a friendly composer over the Authorization header (and,
  // for API keys, a header or query param). Nothing new is persisted: this
  // just writes into the existing headers/params rows, so saved requests,
  // curl export, etc. all see it automatically.
  let authType = 'none'; // none | bearer | basic | apikey
  let authBearer = '';
  let authUser = '';
  let authPass = '';
  let authKeyName = '';
  let authKeyValue = '';
  let authKeyIn = 'header'; // header | query
  let prevApiKeyName = '';
  let authSyncedTab = null;
  let authApplying = false;

  function findRow(rows, key) {
    const lower = key.toLowerCase();
    return rows.find((r) => r.k.trim().toLowerCase() === lower && r.k.trim());
  }
  function upsertRow(rows, key, value) {
    const lower = key.toLowerCase();
    const idx = rows.findIndex((r) => r.k.trim().toLowerCase() === lower);
    let next;
    if (idx >= 0) {
      next = rows.map((r, i) => (i === idx ? { ...r, v: value, on: true } : r));
    } else {
      next = [{ k: key, v: value, on: true }, ...rows.filter((r) => r.k.trim() || r.v.trim())];
    }
    return ensureTrailingRow(next);
  }
  function removeRow(rows, key) {
    if (!key) return rows;
    const lower = key.toLowerCase();
    return ensureTrailingRow(rows.filter((r) => r.k.trim().toLowerCase() !== lower));
  }

  // Re-derive the Auth tab's fields from the current headers, best-effort,
  // each time the tab is opened (not while it stays open + you're typing).
  $: if (tab === 'auth' && authSyncedTab !== loadedTabId) {
    syncAuthFromHeaders();
    authSyncedTab = loadedTabId;
  }
  function syncAuthFromHeaders() {
    authApplying = true;
    const authHeader = findRow(draft.headers, 'authorization');
    if (authHeader && /^bearer\s+/i.test(authHeader.v)) {
      authType = 'bearer';
      authBearer = authHeader.v.replace(/^bearer\s+/i, '');
    } else if (authHeader && /^basic\s+/i.test(authHeader.v)) {
      authType = 'basic';
      try {
        const [u, ...rest] = atob(authHeader.v.replace(/^basic\s+/i, '')).split(':');
        authUser = u || '';
        authPass = rest.join(':');
      } catch {
        authUser = '';
        authPass = '';
      }
    } else {
      authType = 'none';
      authBearer = '';
      authUser = '';
      authPass = '';
    }
    authKeyName = '';
    authKeyValue = '';
    prevApiKeyName = '';
    setTimeout(() => (authApplying = false), 0);
  }
  function applyAuth() {
    if (authApplying) return;
    if (authType === 'apikey' && prevApiKeyName && prevApiKeyName !== authKeyName) {
      draft.headers = removeRow(draft.headers, prevApiKeyName);
      draft.params = removeRow(draft.params, prevApiKeyName);
    }
    if (authType === 'none') {
      draft.headers = removeRow(draft.headers, 'authorization');
    } else if (authType === 'bearer') {
      draft.headers = upsertRow(draft.headers, 'Authorization', authBearer ? `Bearer ${authBearer}` : '');
    } else if (authType === 'basic') {
      let token = '';
      try {
        token = btoa(`${authUser}:${authPass}`);
      } catch {
        token = '';
      }
      draft.headers = upsertRow(draft.headers, 'Authorization', token ? `Basic ${token}` : '');
    } else if (authType === 'apikey') {
      draft.headers = removeRow(draft.headers, 'authorization');
      if (authKeyName) {
        if (authKeyIn === 'header') {
          draft.headers = upsertRow(draft.headers, authKeyName, authKeyValue);
        } else {
          draft.params = upsertRow(draft.params, authKeyName, authKeyValue);
          paramsToUrl();
        }
      }
      prevApiKeyName = authKeyName;
    }
  }
  $: {
    // reactive dependency list — re-apply whenever any auth field changes
    authType, authBearer, authUser, authPass, authKeyName, authKeyValue, authKeyIn;
    if (!hydrating) applyAuth();
  }

  async function send() {
    if (!draft.url.trim()) return;
    const tabId = loadedTabId;
    sending = true;
    error = null;
    response = null;
    touchRequestTab(tabId, { sending: true, error: null });
    const req = {
      method: draft.method,
      url: draft.url.trim(),
      headers: headersObject(),
      body: ['GET', 'HEAD'].includes(draft.method) ? null : draft.body || null,
      timeout_secs: 60
    };
    try {
      const r = await api.requestSend(req, true, draft.id || null, draft.name || null);
      response = r;
      respTab = 'body';
      touchRequestTab(tabId, { response: r, error: null, sending: false });
    } catch (e) {
      error = String(e);
      touchRequestTab(tabId, { error: String(e), sending: false });
    } finally {
      sending = false;
    }
  }

  async function loadSaved(s) {
    await newRequestTab({
      title: s.name,
      method: s.method,
      url: s.url,
      headers_json: s.headers_json,
      body: s.body,
      saved_request_id: s.id
    });
  }

  let consumedHistory = null;
  $: if ($historyLoad && $historyLoad.kind === 'request' && $historyLoad.at !== consumedHistory) {
    consumedHistory = $historyLoad.at;
    loadHistoryEntry($historyLoad.entry, $historyLoad.resolved);
  }
  async function loadHistoryEntry(e, resolved) {
    const t = await newRequestTab({
      title: e.name || 'From history',
      method: e.method,
      url: e.url,
      headers_json: e.headers_json,
      body: e.body,
      saved_request_id: e.saved_request_id || null
    });
    let resp = null;
    if (resolved) {
      try {
        resp = JSON.parse(resolved);
      } catch {}
    }
    touchRequestTab(t.id, { response: resp, error: e.error || null });
    loadedTabId = null; // force re-hydrate to pull the response in
  }

  // Demo helper: ?req=<name>&send loads a saved request and sends it.
  onMount(async () => {
    const q = new URLSearchParams(location.search);
    if (q.has('reqtab')) tab = q.get('reqtab');
    const want = q.get('req');
    if (!want) return;
    for (let i = 0; i < 50 && !$savedRequests.length; i++) await tick();
    const match = $savedRequests.find((r) => r.name.toLowerCase().includes(want.toLowerCase()));
    if (match) {
      await loadSaved(match);
      if (q.has('send')) {
        await tick();
        await tick();
        await send();
      }
    }
  });

  async function save() {
    const headers = headersObject();
    const name = draft.id ? draft.name : prompt('Request name:', draft.name);
    if (!name) return;
    try {
      const saved = await api.savedRequestSave({
        id: draft.id,
        collection_id: draft.collection_id,
        name,
        method: draft.method,
        url: draft.url,
        headers_json: JSON.stringify(headers),
        body: draft.body || null,
        sort_order: 0,
        created_at: 0
      });
      draft.id = saved.id;
      draft.name = saved.name;
      await reloadRequests();
      toast('Request saved', 'success');
    } catch (e) {
      toastError(e);
    }
  }

  async function delSaved(s) {
    if (!confirm(`Delete "${s.name}"?`)) return;
    try {
      await api.savedRequestDelete(s.id);
      await reloadRequests();
      if (draft.id === s.id) draft.id = '';
    } catch (e) {
      toastError(e);
    }
  }

  async function newCollection() {
    const name = prompt('Collection name:');
    if (!name) return;
    try {
      await api.collectionSave({ id: '', name, parent_id: null });
      await reloadRequests();
    } catch (e) {
      toastError(e);
    }
  }
  async function delCollection(c) {
    if (!confirm(`Delete collection "${c.name}" and its requests?`)) return;
    try {
      await api.collectionDelete(c.id);
      await reloadRequests();
    } catch (e) {
      toastError(e);
    }
  }

  function exportPostman() {
    const pm = toPostmanCollection('OG TestDesk export', $requestCollections, $savedRequests);
    downloadText('og-testdesk-collection.json', JSON.stringify(pm, null, 2), 'application/json');
    toast('Exported as a Postman v2.1 collection', 'success', 2000);
  }

  let fileInput;
  async function onImportFile(e) {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    let parsed;
    try {
      parsed = parsePostman(await file.text());
    } catch (err) {
      return toastError(err);
    }
    if (!parsed) return toastError('Not a recognized Postman collection or environment');
    try {
      if (parsed.kind === 'environment') {
        await api.environmentSave({
          id: '',
          name: parsed.name,
          variables_json: JSON.stringify(parsed.variables),
          is_active: false
        });
        toast(`Imported environment "${parsed.name}" (${Object.keys(parsed.variables).length} vars)`, 'success');
      } else {
        const col = await api.collectionSave({ id: '', name: parsed.name, parent_id: null });
        for (let i = 0; i < parsed.requests.length; i++) {
          const r = parsed.requests[i];
          await api.savedRequestSave({
            id: '',
            collection_id: col.id,
            name: r.folder ? `${r.folder} / ${r.name}` : r.name,
            method: r.method,
            url: r.url,
            headers_json: JSON.stringify(r.headers || {}),
            body: r.body,
            sort_order: i,
            created_at: 0
          });
        }
        toast(`Imported "${parsed.name}" — ${parsed.requests.length} requests`, 'success');
      }
      await reloadRequests();
    } catch (err) {
      toastError(err);
    }
  }

  $: grouped = groupRequests($requestCollections, $savedRequests);
  function groupRequests(cols, reqs) {
    const byCol = new Map(cols.map((c) => [c.id, { ...c, items: [] }]));
    const loose = [];
    for (const r of reqs) {
      if (r.collection_id && byCol.has(r.collection_id)) byCol.get(r.collection_id).items.push(r);
      else loose.push(r);
    }
    return { collections: [...byCol.values()], loose };
  }

  function bodyLang() {
    const ct = (draft.headers.find((h) => h.k.toLowerCase() === 'content-type') || {}).v || '';
    if (ct.includes('json') || (draft.body.trim().startsWith('{') || draft.body.trim().startsWith('[')))
      return 'json';
    return 'text';
  }

  function prettyBody() {
    try {
      draft.body = JSON.stringify(JSON.parse(draft.body), null, 2);
    } catch (e) {
      toastError('Body is not valid JSON');
    }
  }

  function inspectResponse() {
    if (!response) return;
    let json;
    try {
      json = JSON.parse(response.body);
    } catch {
      toastError('Response is not JSON');
      return;
    }
    sendToInspector('requests', `${draft.method} ${draft.url}`, json);
  }

  function respFileBase() {
    return (draft.name || 'response').replace(/[^\w.-]+/g, '_').slice(0, 60) || 'response';
  }
  function saveResponse() {
    if (!response) return;
    const json = response.is_json;
    const text = json ? tryPretty(response.body) : response.body;
    downloadText(
      `${respFileBase()}.${json ? 'json' : 'txt'}`,
      text,
      json ? 'application/json' : 'text/plain'
    );
  }
  async function copyResponse() {
    if (!response) return;
    const ok = await copyText(response.body);
    toast(ok ? 'Response body copied' : 'Copy blocked', ok ? 'success' : 'error', 1500);
  }
  // CSV export when the JSON body is an array of objects (or has one
  // array-of-objects field, e.g. { data: [...] }).
  function responseRows() {
    if (!response?.is_json) return null;
    let v;
    try {
      v = JSON.parse(response.body);
    } catch {
      return null;
    }
    const isObjArray = (x) => Array.isArray(x) && x.every((r) => r && typeof r === 'object' && !Array.isArray(r));
    if (isObjArray(v)) return v;
    if (v && typeof v === 'object') {
      for (const val of Object.values(v)) if (isObjArray(val)) return val;
    }
    return null;
  }
  function exportResponseCsv() {
    const rows = responseRows();
    if (!rows) {
      toast('CSV needs a JSON array of objects in the response body', 'error', 3000);
      return;
    }
    const cols = [...new Set(rows.flatMap((r) => Object.keys(r)))].map((name) => ({ name }));
    const csvRows = rows.map((r) => cols.map((c) => r[c.name]));
    downloadText(`${respFileBase()}.csv`, rowsToDelimited(cols, csvRows, ','), 'text/csv');
  }
  async function copyCurl() {
    const headers = headersObject();
    const body = ['GET', 'HEAD'].includes(draft.method) ? null : draft.body || null;
    const ok = await copyText(toCurl(draft.method, draft.url, headers, body));
    toast(ok ? 'curl command copied' : 'Copy blocked', ok ? 'success' : 'error', 1500);
  }

  function statusClass(s) {
    if (s >= 200 && s < 300) return 'ok';
    if (s >= 300 && s < 400) return 'redir';
    if (s >= 400) return 'err';
    return '';
  }
  function fmtSize(n) {
    return n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(2)} MB`;
  }

  function startDrag() {
    dragging = true;
  }
  function onMove(e) {
    if (!dragging) return;
    const host = document.querySelector('.rq-work');
    if (!host) return;
    const rect = host.getBoundingClientRect();
    splitPct = Math.min(80, Math.max(20, ((e.clientY - rect.top) / rect.height) * 100));
  }
  function endDrag() {
    dragging = false;
  }
</script>

<svelte:window on:mousemove={onMove} on:mouseup={endDrag} />

<div class="rq">
  <aside class="sidebar">
    <div class="sec-head">
      <span>Collections</span>
      <div>
        <button class="btn ghost sm" title="Import Postman collection / environment" on:click={() => fileInput.click()}>{ICONS.importPostman.glyph}</button>
        <button class="btn ghost sm" title="Export everything as a Postman collection" on:click={exportPostman}>{ICONS.exportPostman.glyph}</button>
        <button class="btn ghost sm" on:click={newCollection}>+ Folder</button>
        <button class="btn ghost sm" on:click={() => newRequestTab()}>+ Req</button>
      </div>
    </div>
    <input
      type="file"
      accept=".json,application/json"
      bind:this={fileInput}
      on:change={onImportFile}
      style="display:none"
    />
    <div class="scroll">
      {#each grouped.collections as col (col.id)}
        <div class="col-head">
          <span>{col.name}</span>
          <button class="btn ghost sm danger" on:click={() => delCollection(col)}>{ICONS.delete.glyph}</button>
        </div>
        {#each col.items as s (s.id)}
          <div class="req-item" class:active={draft.id === s.id}>
            <button class="ri-main" on:click={() => loadSaved(s)}>
              <span class="mm" style="color:var(--m-{s.method.toLowerCase()})">{s.method}</span>
              <span class="rn">{s.name}</span>
            </button>
            <button class="del" on:click={() => delSaved(s)}>{ICONS.delete.glyph}</button>
          </div>
        {/each}
      {/each}
      {#if grouped.loose.length}
        <div class="col-head"><span>Ungrouped</span></div>
        {#each grouped.loose as s (s.id)}
          <div class="req-item" class:active={draft.id === s.id}>
            <button class="ri-main" on:click={() => loadSaved(s)}>
              <span class="mm" style="color:var(--m-{s.method.toLowerCase()})">{s.method}</span>
              <span class="rn">{s.name}</span>
            </button>
            <button class="del" on:click={() => delSaved(s)}>{ICONS.delete.glyph}</button>
          </div>
        {/each}
      {/if}
      {#if $savedRequests.length === 0}
        <div class="muted" style="padding:10px">No saved requests.</div>
      {/if}
    </div>
    <div class="env-bar">
      <button class="btn ghost sm" on:click={() => (envModal = true)}>
        Env: {$activeEnvironment?.name || 'none'} ▾
      </button>
    </div>
  </aside>

  <section class="main">
    <div class="urlbar">
      <select class="method" bind:value={draft.method} style="color:{methodColor}">
        {#each METHODS as m}<option value={m}>{m}</option>{/each}
      </select>
      <input
        class="url input mono"
        placeholder="https://api.example.com/v1/resource  —  {'{{baseUrl}}'} allowed"
        bind:value={draft.url}
        on:change={urlToParams}
        on:keydown={(e) => e.key === 'Enter' && send()}
      />
      <button class="btn primary send" on:click={send} disabled={sending}>
        {sending ? '…' : 'Send'}
      </button>
      <button class="btn" on:click={save}>Save</button>
    </div>

    <div class="rq-work">
      <div class="req-pane" style="height:{splitPct}%">
        <div class="subtabs">
          {#each ['params', 'headers', 'auth', 'body'] as t}
            <button class:active={tab === t} on:click={() => (tab = t)}>
              {t}
              {#if t === 'headers' && draft.headers.filter((h) => h.k).length}
                <span class="n">{draft.headers.filter((h) => h.k).length}</span>
              {/if}
              {#if t === 'params' && draft.params.filter((p) => p.k).length}
                <span class="n">{draft.params.filter((p) => p.k).length}</span>
              {/if}
              {#if t === 'auth' && authType !== 'none'}<span class="n">1</span>{/if}
            </button>
          {/each}
        </div>

        {#if tab === 'auth'}
          <div class="auth-form">
            <div class="field">
              <label for="authType">Type</label>
              <select id="authType" class="select" bind:value={authType}>
                <option value="none">No auth</option>
                <option value="bearer">Bearer token</option>
                <option value="basic">Basic auth</option>
                <option value="apikey">API key</option>
              </select>
            </div>
            {#if authType === 'bearer'}
              <div class="field">
                <label for="authBearer">Token</label>
                <input id="authBearer" class="input mono" bind:value={authBearer} placeholder="{'{{token}}'} or a raw value" />
              </div>
              <p class="hint">Sets <code>Authorization: Bearer &lt;token&gt;</code>.</p>
            {:else if authType === 'basic'}
              <div class="row2">
                <div class="field">
                  <label for="authUser">Username</label>
                  <input id="authUser" class="input" bind:value={authUser} />
                </div>
                <div class="field">
                  <label for="authPass">Password</label>
                  <input id="authPass" class="input" type="password" bind:value={authPass} />
                </div>
              </div>
              <p class="hint">Sets <code>Authorization: Basic &lt;base64(user:pass)&gt;</code>.</p>
            {:else if authType === 'apikey'}
              <div class="row2">
                <div class="field">
                  <label for="authKeyName">Key</label>
                  <input id="authKeyName" class="input mono" bind:value={authKeyName} placeholder="X-API-Key" />
                </div>
                <div class="field">
                  <label for="authKeyValue">Value</label>
                  <input id="authKeyValue" class="input mono" bind:value={authKeyValue} />
                </div>
              </div>
              <div class="field">
                <label for="authKeyIn">Add to</label>
                <select id="authKeyIn" class="select" bind:value={authKeyIn}>
                  <option value="header">Header</option>
                  <option value="query">Query param</option>
                </select>
              </div>
              <p class="hint">Adds the key to your {authKeyIn === 'header' ? 'Headers' : 'Params'} tab.</p>
            {:else}
              <p class="hint">No authorization header is sent.</p>
            {/if}
          </div>
        {:else if tab === 'params' || tab === 'headers'}
          {@const rows = tab === 'params' ? draft.params : draft.headers}
          <div class="kv-grid">
            {#each rows as row}
              <div class="kv-row">
                <input type="checkbox" bind:checked={row.on} />
                <input
                  class="input mono"
                  placeholder={tab === 'params' ? 'key' : 'Header-Name'}
                  bind:value={row.k}
                  on:input={tab === 'params' ? onParamInput : onHeaderInput}
                />
                <input
                  class="input mono"
                  placeholder="value"
                  bind:value={row.v}
                  on:input={tab === 'params' ? onParamInput : onHeaderInput}
                />
              </div>
            {/each}
          </div>
        {:else}
          <div class="body-tools">
            <button class="btn ghost sm" on:click={prettyBody}>Beautify JSON</button>
            <span class="muted">{['GET', 'HEAD'].includes(draft.method) ? 'body ignored for ' + draft.method : ''}</span>
          </div>
          <div class="body-editor">
            <CodeEditor bind:value={draft.body} language={bodyLang()} on:run={send} />
          </div>
        {/if}
      </div>

      <div class="splitter" on:mousedown={startDrag} role="separator" tabindex="-1"></div>

      <div class="resp-pane" style="height:{100 - splitPct}%">
        {#if error}
          <div class="resp-err">{error}</div>
        {:else if !response}
          <div class="empty">Send a request to see the response.</div>
        {:else}
          <div class="resp-head">
            <span class="status {statusClass(response.status)}">{response.status} {response.status_text}</span>
            <span class="meta">{response.duration_ms} ms</span>
            <span class="meta">{fmtSize(response.size_bytes)}</span>
            {#if response.content_type}<span class="meta ct">{response.content_type.split(';')[0]}</span>{/if}
            <span style="flex:1" />
            {#if response.is_json}
              <button class="btn ghost sm" on:click={inspectResponse}>{ICONS.toInspector.glyph} Inspector</button>
              <button class="btn ghost sm" title="Export as CSV (needs an array of objects)" on:click={exportResponseCsv}
                >Export CSV</button
              >
            {/if}
            <button class="btn ghost sm" on:click={saveResponse}>{response.is_json ? 'Export JSON' : 'Save'}</button>
            <button class="btn ghost sm" title="Copy response body" on:click={copyResponse}>{ICONS.copy.glyph} Body</button>
            <button class="btn ghost sm" title="Copy request as curl" on:click={copyCurl}>curl</button>
            <div class="subtabs sm">
              <button class:active={respTab === 'body'} on:click={() => (respTab = 'body')}>Body</button>
              <button class:active={respTab === 'headers'} on:click={() => (respTab = 'headers')}>
                Headers <span class="n">{response.headers.length}</span>
              </button>
            </div>
          </div>
          {#if respTab === 'body'}
            <div class="resp-body">
              <CodeEditor
                value={response.is_json ? tryPretty(response.body) : response.body}
                language={response.is_json ? 'json' : 'text'}
                readonly
              />
            </div>
          {:else}
            <div class="resp-headers">
              {#each response.headers as [k, v]}
                <div class="h-row"><span class="hk">{k}</span><span class="hv">{v}</span></div>
              {/each}
            </div>
          {/if}
        {/if}
      </div>
    </div>
  </section>
</div>

{#if envModal}
  <EnvModal on:close={() => (envModal = false)} />
{/if}

<script context="module">
  function tryPretty(s) {
    try {
      return JSON.stringify(JSON.parse(s), null, 2);
    } catch {
      return s;
    }
  }
</script>

<style>
  .rq {
    display: flex;
    height: 100%;
    overflow: hidden;
  }
  .sidebar {
    width: 250px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    background: var(--surface-1);
    display: flex;
    flex-direction: column;
  }
  .sec-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border);
  }
  .scroll {
    flex: 1;
    overflow: auto;
    padding: 4px 0;
  }
  .col-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 8px 2px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .req-item {
    display: flex;
    align-items: center;
  }
  .req-item.active {
    background: color-mix(in srgb, var(--tool-requests-text) 14%, transparent);
  }
  .ri-main {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 7px;
    background: none;
    border: none;
    color: var(--text-primary);
    padding: 5px 8px;
    cursor: pointer;
    text-align: left;
    overflow: hidden;
  }
  .mm {
    font-size: 9px;
    font-weight: 800;
    width: 42px;
    flex-shrink: 0;
  }
  .rn {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .del {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px 7px;
    font-size: 10px;
  }
  .del:hover {
    color: var(--danger);
  }
  .env-bar {
    border-top: 1px solid var(--border);
    padding: 6px;
  }
  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .urlbar {
    display: flex;
    gap: 6px;
    padding: 8px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-1);
  }
  .method {
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    height: var(--ctrl-h);
    font-weight: 800;
    font-size: 12px;
    padding: 0 22px 0 8px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-strong);
    background-color: var(--surface-2);
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath d='M1 1l4 4 4-4' fill='none' stroke='%23888780' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 7px center;
    cursor: pointer;
  }
  .url {
    flex: 1;
  }
  .send {
    min-width: 64px;
    justify-content: center;
  }
  .rq-work {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .req-pane,
  .resp-pane {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .resp-pane {
    border-top: 1px solid var(--border);
  }
  .splitter {
    height: 6px;
    background: var(--surface-1);
    cursor: row-resize;
    flex-shrink: 0;
  }
  .splitter:hover {
    background: var(--tool-requests-text);
  }
  .subtabs {
    display: flex;
    gap: 2px;
    padding: 4px 8px 0;
    border-bottom: 1px solid var(--border);
  }
  .subtabs button {
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    padding: 6px 10px;
    font-size: 11px;
    text-transform: capitalize;
    color: var(--text-muted);
    cursor: pointer;
  }
  .subtabs button.active {
    color: var(--tool-requests-text);
    border-bottom-color: var(--tool-requests-text);
    font-weight: 600;
  }
  .subtabs.sm button {
    padding: 3px 8px;
  }
  .n {
    background: var(--tool-requests-tint);
    color: var(--tool-requests-text);
    border-radius: 8px;
    padding: 0 5px;
    font-size: 9px;
  }
  .kv-grid {
    overflow: auto;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .kv-row {
    display: grid;
    grid-template-columns: 18px 1fr 1.5fr;
    gap: 6px;
    align-items: center;
  }
  .body-tools {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
  }
  .auth-form {
    overflow: auto;
    padding: 10px 12px;
    max-width: 420px;
  }
  .auth-form .row2 {
    display: flex;
    gap: 10px;
  }
  .auth-form .row2 .field {
    flex: 1;
  }
  .auth-form .hint {
    font-size: 11px;
    color: var(--text-muted);
    margin: 2px 0 0;
  }
  .auth-form .hint code {
    font-family: var(--font-mono);
    background: var(--surface-2);
    padding: 1px 4px;
    border-radius: 3px;
  }
  .body-editor,
  .resp-body {
    flex: 1;
    overflow: hidden;
  }
  .empty,
  .resp-err {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    font-size: 12px;
    padding: 20px;
    text-align: center;
  }
  .resp-err {
    color: var(--danger);
    font-family: var(--font-mono);
    white-space: pre-wrap;
    align-items: flex-start;
    overflow: auto;
  }
  .resp-head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-1);
    flex-wrap: wrap;
  }
  .status {
    font-weight: 700;
    font-size: 12px;
  }
  .status.ok {
    color: var(--ok);
  }
  .status.redir {
    color: var(--warn);
  }
  .status.err {
    color: var(--danger);
  }
  .meta {
    font-size: 11px;
    color: var(--text-muted);
  }
  .meta.ct {
    font-family: var(--font-mono);
  }
  .resp-headers {
    overflow: auto;
    padding: 8px;
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .h-row {
    display: flex;
    gap: 10px;
    padding: 3px 0;
    border-bottom: 1px solid var(--border);
  }
  .hk {
    color: var(--j-key);
    min-width: 180px;
  }
  .hv {
    color: var(--text-secondary);
    word-break: break-all;
  }
</style>
