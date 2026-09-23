<script>
  import { onMount, tick } from 'svelte';
  import CodeEditor from '../components/CodeEditor.svelte';
  import EnvModal from './EnvModal.svelte';
  import CookieManagerModal from './CookieManagerModal.svelte';
  import NetworkSettingsModal from './NetworkSettingsModal.svelte';
  import MockServerModal from './MockServerModal.svelte';
  import WebSocketPanel from './WebSocketPanel.svelte';
  import GrpcPanel from './GrpcPanel.svelte';
  import ConnPicker from '../sql/ConnPicker.svelte';
  import { api } from '../api.js';
  import { parsePostman, toPostmanCollection } from './postman.js';
  import { runScript } from './ScriptSandbox.js';
  import { randomToken, pkceChallengeFromVerifier } from './oauth2.js';
  import { signAwsV4 } from './awsSigV4.js';
  import VarInput from './VarInput.svelte';
  import { parseDigestChallenge, buildDigestHeader } from './digestAuth.js';
  import { CODE_GENERATORS } from './codegen.js';
  import { open as openExternal } from '@tauri-apps/plugin-shell';
  import { ICONS } from '../icons.js';
  import { downloadText, copyText, rowsToDelimited } from '../export.js';
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
    persistRequestTab,
    promptDialog,
    confirmDialog
  } from '../stores.js';

  const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];

  // collapsed collections in the sidebar — session-only, so collapsing a
  // big collection leaves room to see the others below it
  let collapsedCols = new Set();
  function toggleCol(id) {
    collapsedCols.has(id) ? collapsedCols.delete(id) : collapsedCols.add(id);
    collapsedCols = collapsedCols;
  }

  let draft = blank();
  let tab = 'params'; // params | headers | body
  let response = null;
  let sending = false;
  let error = null;
  let respTab = 'body'; // body | headers
  let envModal = false;
  let cookieModal = false;
  let networkModal = false;
  let mockModal = false;
  let wsModal = false;
  let grpcModal = false;
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
      bodyMode: 'raw', // raw | urlencoded | multipart | binary | graphql
      formFields: [{ k: '', v: '', kind: 'text', filename: '', contentType: '', on: true }],
      binaryFile: null, // { filename, base64, size }
      graphqlQuery: '',
      graphqlVariables: '',
      collection_id: null,
      pre_request_script: '',
      test_script: ''
    };
  }

  // ---- draft <-> active request tab ----------------------------------------
  let loadedTabId = null;
  let flushTimer;

  $: if ($activeRequestTab && $activeRequestTab.id !== loadedTabId) {
    hydrateDraft($activeRequestTab);
  }
  // The tab we're tracking got closed out from under us (loadedTabId
  // isn't reset by closing — only by hydrating a different tab). A
  // pending debounced flush (below) would otherwise still fire later
  // and re-upsert this tab's row via persistRequestTab, resurrecting it
  // — most visibly right after a window focus/reload race repopulates
  // $requestTabs with the not-yet-deleted row just before this stale
  // flush lands. Cancel it and stop tracking the dead id.
  $: if (loadedTabId && !$requestTabs.some((t) => t.id === loadedTabId)) {
    clearTimeout(flushTimer);
    loadedTabId = null;
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
      ...hydrateBodyMode(t),
      collection_id: $savedRequests.find((s) => s.id === t.saved_request_id)?.collection_id ?? null,
      pre_request_script: t.pre_request_script || '',
      test_script: t.test_script || ''
    };
    urlToParams();
    if (draft.body) tab = 'body';
    response = t.response ?? null;
    error = t.error ?? null;
    sending = t.sending ?? false;
    setTimeout(() => (hydrating = false), 0);
  }

  // ---- structured body (form-data / x-www-form-urlencoded / binary /
  // GraphQL) — everything raw text can't express. Stored as body_mode_json
  // on the tab/saved request, matching core::requests::RequestBody's exact
  // JSON shape 1:1 so no translation is needed when actually sending.
  function blankFormField() {
    return { k: '', v: '', kind: 'text', filename: '', contentType: '', on: true, _fileBase64: '' };
  }
  function ensureTrailingFormRow(arr) {
    const last = arr[arr.length - 1];
    if (!last || last.k !== '') arr.push(blankFormField());
    return arr;
  }
  function onFormFieldInput() {
    draft.formFields = ensureTrailingFormRow([...draft.formFields]);
  }
  function hydrateBodyMode(t) {
    const base = { bodyMode: 'raw', formFields: [blankFormField()], binaryFile: null, graphqlQuery: '', graphqlVariables: '' };
    let rb = null;
    try {
      rb = t.body_mode_json ? JSON.parse(t.body_mode_json) : null;
    } catch {}
    if (!rb) return base;
    if (rb.kind === 'form_url_encoded' || rb.kind === 'multipart') {
      const fields = (rb.fields || []).map((f) => ({
        k: f.key,
        v: f.kind === 'file' ? '' : f.value,
        kind: f.kind,
        filename: f.filename || '',
        contentType: f.content_type || '',
        on: f.enabled !== false,
        _fileBase64: f.kind === 'file' ? f.value : ''
      }));
      return { ...base, bodyMode: rb.kind === 'multipart' ? 'multipart' : 'urlencoded', formFields: ensureTrailingFormRow(fields) };
    }
    if (rb.kind === 'binary') {
      return { ...base, bodyMode: 'binary', binaryFile: { filename: '', base64: rb.base64, size: Math.round((rb.base64 || '').length * 0.75) } };
    }
    if (rb.kind === 'graphql') {
      return { ...base, bodyMode: 'graphql', graphqlQuery: rb.query || '', graphqlVariables: rb.variables || '' };
    }
    return base;
  }
  function buildBodyMode() {
    if (draft.bodyMode === 'urlencoded') {
      return {
        kind: 'form_url_encoded',
        fields: draft.formFields
          .filter((f) => f.k)
          .map((f) => ({ key: f.k, kind: 'text', value: f.v, filename: null, content_type: null, enabled: f.on }))
      };
    }
    if (draft.bodyMode === 'multipart') {
      return {
        kind: 'multipart',
        fields: draft.formFields
          .filter((f) => f.k)
          .map((f) => ({
            key: f.k,
            kind: f.kind,
            value: f.kind === 'file' ? f._fileBase64 || '' : f.v,
            filename: f.kind === 'file' ? f.filename || null : null,
            content_type: f.kind === 'file' ? f.contentType || null : null,
            enabled: f.on
          }))
      };
    }
    if (draft.bodyMode === 'binary') {
      return draft.binaryFile ? { kind: 'binary', base64: draft.binaryFile.base64 } : null;
    }
    if (draft.bodyMode === 'graphql') {
      return { kind: 'graphql', query: draft.graphqlQuery, variables: draft.graphqlVariables || null };
    }
    return null; // 'raw' — use draft.body as a plain string, unchanged
  }
  function bodyModeJsonForSave() {
    if (draft.bodyMode === 'raw') return null;
    const rb = buildBodyMode();
    return rb ? JSON.stringify(rb) : null;
  }

  function readFileAsBase64(file) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result).split(',')[1] || '');
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
  }
  async function onMultipartFilePick(e, row) {
    const file = e.target.files?.[0];
    if (!file) return;
    row._fileBase64 = await readFileAsBase64(file);
    row.filename = file.name;
    row.contentType = file.type || 'application/octet-stream';
    draft.formFields = [...draft.formFields];
    onFormFieldInput();
  }
  async function onBinaryFilePick(e) {
    const file = e.target.files?.[0];
    if (!file) return;
    draft.binaryFile = { filename: file.name, base64: await readFileAsBase64(file), size: file.size };
  }

  // Variable names offered by VarInput's "{{" dropdown — active
  // environment vars (reactive) + pm.globals (loaded once, refreshed
  // after each send since a script can add new ones there).
  let globalVarNames = [];
  onMount(async () => {
    globalVarNames = Object.keys(await loadGlobals());
  });
  $: envVarNames = $activeEnvironment ? Object.keys(safeJson($activeEnvironment.variables_json)) : [];
  $: availableVars = [...new Set([...envVarNames, ...globalVarNames])].sort();

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
        pre_request_script: draft.pre_request_script || null,
        test_script: draft.test_script || null,
        body_mode_json: bodyModeJsonForSave(),
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
  let authType = 'none'; // none | bearer | basic | apikey | oauth2 | digest | awsv4
  let authBearer = '';
  let authUser = '';
  let authPass = '';
  let authKeyName = '';
  let authKeyValue = '';
  let authKeyIn = 'header'; // header | query
  let prevApiKeyName = '';
  let authSyncedTab = null;
  let authApplying = false;

  // OAuth2/Digest/AWS SigV4 configs have no header representation to
  // round-trip through the way Bearer/Basic/API-key do (a client secret
  // or AWS access key isn't reconstructible from an Authorization
  // header), so — unlike the rest of the auth tab — this bit of config
  // is persisted separately, per tab, in localStorage rather than in
  // the backend. OAuth2's *result* (the fetched access token) still
  // flows through the normal header mechanism once obtained.
  let authOauth = { grantType: 'client_credentials', authUrl: '', tokenUrl: '', clientId: '', clientSecret: '', scope: '' };
  let authDigest = { username: '', password: '' };
  let authAws = { accessKeyId: '', secretAccessKey: '', sessionToken: '', region: 'us-east-1', service: 'execute-api' };
  let oauthBusy = false;
  let oauthStatus = '';

  function authCfgKey(id) {
    return `og_testdesk_authcfg_${id}`;
  }
  function loadAuthCfg(id) {
    try {
      const raw = localStorage.getItem(authCfgKey(id));
      if (!raw) return;
      const cfg = JSON.parse(raw);
      if (cfg.authType) authType = cfg.authType;
      if (cfg.oauth) authOauth = { ...authOauth, ...cfg.oauth };
      if (cfg.digest) authDigest = { ...authDigest, ...cfg.digest };
      if (cfg.aws) authAws = { ...authAws, ...cfg.aws };
    } catch {}
  }
  function saveAuthCfg(id) {
    if (!id) return;
    // Only persist for the non-header-backed types — the rest already
    // round-trip through headers, no need to duplicate that state.
    if (!['oauth2', 'digest', 'awsv4'].includes(authType)) {
      localStorage.removeItem(authCfgKey(id));
      return;
    }
    try {
      localStorage.setItem(
        authCfgKey(id),
        JSON.stringify({ authType, oauth: authOauth, digest: authDigest, aws: authAws })
      );
    } catch {}
  }

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
    loadAuthCfg(loadedTabId);
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
    } else if (authType === 'oauth2') {
      // Nothing to write until a token's actually been fetched — see
      // fetchOauthToken(), which writes the header itself once it has one.
    } else if (authType === 'digest' || authType === 'awsv4') {
      // Computed fresh per-send (the signature/response depends on the
      // exact request being sent right now) — never a static header.
      draft.headers = removeRow(draft.headers, 'authorization');
    }
  }

  // OAuth2: Client Credentials (server-to-server, no browser) and
  // Authorization Code + PKCE (opens the system browser, catches the
  // redirect on a loopback listener). Not implemented: Implicit and
  // Password grants (both considered deprecated/discouraged by the OAuth
  // spec itself now) and NTLM (Windows-domain auth — a different protocol
  // family entirely, rare outside enterprise intranets, and not something
  // reqwest/the browser fetch stack support out of the box).
  async function fetchOauthToken() {
    oauthBusy = true;
    oauthStatus = '';
    try {
      let tokenResp;
      if (authOauth.grantType === 'authorization_code') {
        tokenResp = await runAuthorizationCodeFlow();
      } else {
        tokenResp = await requestToken({
          grant_type: 'client_credentials',
          client_id: authOauth.clientId,
          client_secret: authOauth.clientSecret,
          scope: authOauth.scope || undefined
        });
      }
      const token = tokenResp.access_token;
      if (!token) throw new Error('Response had no access_token');
      authApplying = true;
      draft.headers = upsertRow(draft.headers, 'Authorization', `${tokenResp.token_type || 'Bearer'} ${token}`);
      setTimeout(() => (authApplying = false), 0);
      const expiresIn = tokenResp.expires_in ? `, expires in ${tokenResp.expires_in}s` : '';
      oauthStatus = `Token acquired${expiresIn}`;
      toast('OAuth2 token acquired', 'success', 2500);
    } catch (e) {
      oauthStatus = `Error: ${e.message || e}`;
      toastError(e);
    } finally {
      oauthBusy = false;
    }
  }

  async function requestToken(params) {
    const body = new URLSearchParams(Object.fromEntries(Object.entries(params).filter(([, v]) => v !== undefined)));
    const resp = await fetch(authOauth.tokenUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: body.toString()
    });
    const text = await resp.text();
    let json;
    try {
      json = JSON.parse(text);
    } catch {
      throw new Error(`Token endpoint returned non-JSON (${resp.status}): ${text.slice(0, 200)}`);
    }
    if (!resp.ok) throw new Error(json.error_description || json.error || `Token request failed (${resp.status})`);
    return json;
  }

  async function runAuthorizationCodeFlow() {
    const port = await api.oauthStartListener();
    const redirectUri = `http://127.0.0.1:${port}/callback`;
    const state = randomToken(16);
    const verifier = randomToken(32);
    const challenge = await pkceChallengeFromVerifier(verifier);

    const authUrl = new URL(authOauth.authUrl);
    authUrl.searchParams.set('response_type', 'code');
    authUrl.searchParams.set('client_id', authOauth.clientId);
    authUrl.searchParams.set('redirect_uri', redirectUri);
    authUrl.searchParams.set('state', state);
    authUrl.searchParams.set('code_challenge', challenge);
    authUrl.searchParams.set('code_challenge_method', 'S256');
    if (authOauth.scope) authUrl.searchParams.set('scope', authOauth.scope);

    oauthStatus = 'Waiting for browser authorization…';
    await openExternal(authUrl.toString());
    const params = await api.oauthWaitCallback(port, 180);
    if (params.error) throw new Error(params.error_description || params.error);
    if (params.state !== state) throw new Error('state mismatch — possible CSRF, aborting');
    if (!params.code) throw new Error('No authorization code in the redirect');

    oauthStatus = 'Exchanging code for a token…';
    return requestToken({
      grant_type: 'authorization_code',
      code: params.code,
      redirect_uri: redirectUri,
      client_id: authOauth.clientId,
      client_secret: authOauth.clientSecret || undefined,
      code_verifier: verifier
    });
  }
  $: {
    // reactive dependency list — re-apply whenever any auth field changes
    authType, authBearer, authUser, authPass, authKeyName, authKeyValue, authKeyIn;
    if (!hydrating) applyAuth();
  }
  $: {
    // reactive dependency list for the localStorage-backed configs
    authType, authOauth, authDigest, authAws;
    if (!hydrating && loadedTabId) saveAuthCfg(loadedTabId);
  }

  // pm.globals uses the same request_globals store the backend already
  // auto-applies during request_send for {{var}} substitution (api.js's
  // globalsGet/globalsSet — plumbed server-side but with no UI yet before
  // this) — not a separate localStorage copy, so a script's
  // pm.globals.set(...) is visible to every other request's {{var}}
  // substitution and vice versa, the same relationship pm.environment
  // already has with the active environment.
  async function loadGlobals() {
    try {
      const raw = await api.globalsGet();
      return raw ? JSON.parse(raw) : {};
    } catch {
      return {};
    }
  }
  async function saveGlobals(obj) {
    try {
      await api.globalsSet(JSON.stringify(obj));
    } catch {}
  }

  async function persistEnvIfChanged(env, before, after) {
    if (env && JSON.stringify(before) !== JSON.stringify(after)) {
      try {
        await api.environmentSave({ ...env, variables_json: JSON.stringify(after) });
      } catch {}
    }
  }

  // AWS SigV4 and Digest can't be precomputed like Bearer/Basic — SigV4's
  // signature depends on the exact request being sent right now
  // (timestamp, headers, body hash); Digest's response hash depends on a
  // nonce the server hasn't issued yet. Both are resolved here, right
  // before the real send, rather than in the reactive header composer.
  async function doSend(req) {
    if (authType === 'awsv4' && authAws.accessKeyId) {
      const extra = await signAwsV4({ method: req.method, url: req.url, headers: req.headers, body: req.body }, authAws);
      return api.requestSend({ ...req, headers: { ...req.headers, ...extra } }, true, draft.id || null, draft.name || null);
    }
    if (authType === 'digest' && authDigest.username) {
      const first = await api.requestSend(req, true, draft.id || null, draft.name || null);
      if (first.status !== 401) return first;
      const wwwAuth = first.headers.find(([k]) => k.toLowerCase() === 'www-authenticate');
      const challenge = wwwAuth && parseDigestChallenge(wwwAuth[1]);
      if (!challenge) return first; // not a Digest challenge — nothing we can do, show the 401 as-is
      const u = new URL(req.url);
      const digestHeader = buildDigestHeader({
        username: authDigest.username,
        password: authDigest.password,
        method: req.method,
        uri: u.pathname + u.search,
        challenge
      });
      return api.requestSend(
        { ...req, headers: { ...req.headers, Authorization: digestHeader } },
        true,
        draft.id || null,
        draft.name || null
      );
    }
    return api.requestSend(req, true, draft.id || null, draft.name || null);
  }

  async function send() {
    if (!draft.url.trim()) return;
    const tabId = loadedTabId;
    sending = true;
    error = null;
    response = null;
    touchRequestTab(tabId, { sending: true, error: null });

    const noBody = ['GET', 'HEAD'].includes(draft.method);
    let req = {
      method: draft.method,
      url: draft.url.trim(),
      headers: headersObject(),
      body: noBody || draft.bodyMode !== 'raw' ? null : draft.body || null,
      body_mode: noBody ? null : buildBodyMode(),
      timeout_secs: 60
    };

    const env = $activeEnvironment;
    let envVars = env ? safeJson(env.variables_json) : {};
    const envVarsBefore = { ...envVars };
    let globals = await loadGlobals();

    if (draft.pre_request_script?.trim()) {
      const out = await runScript('pre', draft.pre_request_script, {
        request: { method: req.method, url: req.url, headers: req.headers, body: req.body },
        environment: envVars,
        globals
      });
      if (out.error) {
        error = `Pre-request script error: ${out.error}`;
        touchRequestTab(tabId, { error, sending: false });
        sending = false;
        return;
      }
      if (out.request) {
        req = {
          ...req,
          method: out.request.method || req.method,
          url: out.request.url || req.url,
          headers: out.request.headers || req.headers,
          body: out.request.body !== undefined ? out.request.body : req.body
        };
      }
      envVars = out.vars.environment;
      globals = out.vars.globals;
      await saveGlobals(globals);
    }

    try {
      const r = await doSend(req);
      let testResults = null;
      if (draft.test_script?.trim()) {
        const out = await runScript('test', draft.test_script, {
          response: r,
          environment: envVars,
          globals
        });
        testResults = out.error ? [{ name: 'Script error', passed: false, error: out.error }] : out.results;
        envVars = out.vars.environment;
        globals = out.vars.globals;
        await saveGlobals(globals);
      }
      await persistEnvIfChanged(env, envVarsBefore, envVars);
      response = { ...r, testResults };
      respTab = 'body';
      touchRequestTab(tabId, { response, error: null, sending: false });
    } catch (e) {
      await persistEnvIfChanged(env, envVarsBefore, envVars);
      error = String(e);
      touchRequestTab(tabId, { error: String(e), sending: false });
    } finally {
      sending = false;
      globalVarNames = Object.keys(globals);
    }
  }

  async function loadSaved(s) {
    await newRequestTab({
      title: s.name,
      method: s.method,
      url: s.url,
      headers_json: s.headers_json,
      body: s.body,
      saved_request_id: s.id,
      pre_request_script: s.pre_request_script || null,
      test_script: s.test_script || null,
      body_mode_json: s.body_mode_json || null
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
    const name = draft.id ? draft.name : await promptDialog('Request name:', draft.name);
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
        created_at: 0,
        pre_request_script: draft.pre_request_script || null,
        test_script: draft.test_script || null,
        body_mode_json: bodyModeJsonForSave()
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
    if (!(await confirmDialog(`Delete "${s.name}"?`, { danger: true }))) return;
    try {
      await api.savedRequestDelete(s.id);
      await reloadRequests();
      if (draft.id === s.id) draft.id = '';
    } catch (e) {
      toastError(e);
    }
  }

  async function newCollection() {
    const name = await promptDialog('Collection name:');
    if (!name) return;
    try {
      await api.collectionSave({ id: '', name, parent_id: null });
      await reloadRequests();
    } catch (e) {
      toastError(e);
    }
  }
  let selectedReqIds = new Set();
  function toggleSelect(id) {
    const next = new Set(selectedReqIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selectedReqIds = next;
  }
  async function deleteSelected() {
    const n = selectedReqIds.size;
    if (!n) return;
    if (!(await confirmDialog(`Delete ${n} selected request${n === 1 ? '' : 's'}?`, { danger: true }))) return;
    try {
      for (const id of selectedReqIds) await api.savedRequestDelete(id);
      selectedReqIds = new Set();
      await reloadRequests();
    } catch (e) {
      toastError(e);
    }
  }

  let dragReqId = null;
  async function moveToCollection(collectionId) {
    if (!dragReqId) return;
    const s = $savedRequests.find((r) => r.id === dragReqId);
    dragReqId = null;
    if (!s || s.collection_id === collectionId) return;
    try {
      await api.savedRequestSave({ ...s, collection_id: collectionId });
      if (draft.id === s.id) draft.collection_id = collectionId;
      await reloadRequests();
    } catch (e) {
      toastError(e);
    }
  }
  async function delCollection(c) {
    if (!(await confirmDialog(`Delete collection "${c.name}" and its requests?`, { danger: true }))) return;
    try {
      await api.collectionDelete(c.id);
      await reloadRequests();
    } catch (e) {
      toastError(e);
    }
  }

  async function exportPostman() {
    const pm = toPostmanCollection('OG TestDesk export', $requestCollections, $savedRequests);
    const saved = await downloadText('og-testdesk-collection.json', JSON.stringify(pm, null, 2), 'application/json');
    if (saved) toast('Exported as a Postman v2.1 collection', 'success', 2000);
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
  async function saveResponse() {
    if (!response) return;
    const json = response.is_json;
    const text = json ? tryPretty(response.body) : response.body;
    const saved = await downloadText(
      `${respFileBase()}.${json ? 'json' : 'txt'}`,
      text,
      json ? 'application/json' : 'text/plain'
    );
    if (saved) toast('Response saved', 'success', 1800);
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
  async function exportResponseCsv() {
    const rows = responseRows();
    if (!rows) {
      toast('CSV needs a JSON array of objects in the response body', 'error', 3000);
      return;
    }
    const cols = [...new Set(rows.flatMap((r) => Object.keys(r)))].map((name) => ({ name }));
    const csvRows = rows.map((r) => cols.map((c) => r[c.name]));
    const saved = await downloadText(`${respFileBase()}.csv`, rowsToDelimited(cols, csvRows, ','), 'text/csv');
    if (saved) toast('CSV saved', 'success', 1800);
  }
  let showCodeMenu = false;
  let codeLang = 'curl';
  function codeSnippet() {
    return CODE_GENERATORS[codeLang].generate({
      method: draft.method,
      url: draft.url,
      headers: headersObject(),
      bodyMode: draft.bodyMode,
      body: ['GET', 'HEAD'].includes(draft.method) ? null : draft.body || null,
      formFields: draft.formFields,
      graphqlQuery: draft.graphqlQuery,
      graphqlVariables: draft.graphqlVariables
    });
  }
  async function copyCodeSnippet() {
    const ok = await copyText(codeSnippet());
    toast(ok ? `${CODE_GENERATORS[codeLang].label} snippet copied` : 'Copy blocked', ok ? 'success' : 'error', 1800);
  }

  // Response visualizer: html renders in a script-less sandboxed iframe
  // (a response body is untrusted content — no allow-scripts, no
  // allow-same-origin, so it can't touch the app), images/PDF render
  // straight from the base64 body via a data: URI.
  $: previewKind = (() => {
    const ct = (response?.content_type || '').split(';')[0].trim().toLowerCase();
    if (ct === 'text/html') return 'html';
    if (ct.startsWith('image/')) return 'image';
    if (ct === 'application/pdf') return 'pdf';
    return null;
  })();

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

  const SIDEBAR_W_KEY = 'ogtestdesk.requests.sidebarW';
  function loadSidebarW() {
    try {
      const n = Number(localStorage.getItem(SIDEBAR_W_KEY));
      return n >= 180 && n <= 480 ? n : 250;
    } catch {
      return 250;
    }
  }
  let sidebarW = loadSidebarW();
  let draggingSidebar = false;
  function startSidebarDrag() {
    draggingSidebar = true;
  }
  function onSidebarMove(e) {
    if (!draggingSidebar) return;
    const host = document.querySelector('.rq');
    if (!host) return;
    const rect = host.getBoundingClientRect();
    sidebarW = Math.min(480, Math.max(180, e.clientX - rect.left));
  }
  function endSidebarDrag() {
    if (!draggingSidebar) return;
    draggingSidebar = false;
    try {
      localStorage.setItem(SIDEBAR_W_KEY, String(Math.round(sidebarW)));
    } catch {}
  }
</script>

<svelte:window
  on:mousemove={(e) => {
    onMove(e);
    onSidebarMove(e);
  }}
  on:mouseup={() => {
    endDrag();
    endSidebarDrag();
  }}
/>

<div class="rq">
  {#if $requestTabs.length}
  <aside class="sidebar" style="width:{sidebarW}px">
    <div class="sec-head">
      {#if selectedReqIds.size}
        <span>{selectedReqIds.size} selected</span>
        <div class="sec-actions">
          <button class="icon-btn" title="Deselect all" on:click={() => (selectedReqIds = new Set())}
            >{@html ICONS.close?.svg ?? '✕'}</button
          >
          <button class="icon-btn" title="Delete selected" on:click={deleteSelected}
            >{@html ICONS.delete.svg}</button
          >
        </div>
      {:else}
      <span>Collections</span>
      <div class="sec-actions">
        <button
          class="icon-btn big-glyph"
          title={ICONS.importPostman.label}
          on:click={() => fileInput.click()}>{@html ICONS.importPostman.svg}</button
        >
        <button class="icon-btn big-glyph" title={ICONS.exportPostman.label} on:click={exportPostman}
          >{@html ICONS.exportPostman.svg}</button
        >
        <button class="icon-btn" title="New folder" on:click={newCollection}
          >{@html ICONS.newFolder.svg}</button
        >
        <button class="icon-btn" title="New request" on:click={() => newRequestTab()}
          >{@html ICONS.newQuery.svg}</button
        >
      </div>
      {/if}
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
        {@const isOpen = !collapsedCols.has(col.id)}
        <button
          class="col-head"
          class:drop-target={dragReqId !== null}
          on:click={() => toggleCol(col.id)}
          on:dragover|preventDefault
          on:drop|preventDefault={() => moveToCollection(col.id)}
        >
          <span class="chev">{@html isOpen ? ICONS.expandOpen.svg : ICONS.expandClosed.svg}</span>
          <span class="col-name">{col.name}</span>
          <span class="col-cnt">{col.items.length}</span>
          <span
            class="del"
            title="Delete collection"
            on:click|stopPropagation={() => delCollection(col)}
            role="button"
            tabindex="-1">{@html ICONS.delete.svg}</span
          >
        </button>
        {#if isOpen}
          {#each col.items as s (s.id)}
            <div
              class="req-item"
              class:active={draft.id === s.id}
              draggable="true"
              on:dragstart={() => (dragReqId = s.id)}
              on:dragend={() => (dragReqId = null)}
            >
              <input
                type="checkbox"
                class="ri-check"
                checked={selectedReqIds.has(s.id)}
                on:click|stopPropagation={() => toggleSelect(s.id)}
              />
              <button class="ri-main" on:click={() => loadSaved(s)}>
                <span class="mm" style="color:var(--m-{s.method.toLowerCase()})">{s.method}</span>
                <span class="rn">{s.name}</span>
              </button>
              <button class="del" on:click={() => delSaved(s)}>{@html ICONS.delete.svg}</button>
            </div>
          {/each}
        {/if}
      {/each}
      {#if grouped.loose.length || dragReqId !== null}
        <div
          class="col-head"
          class:drop-target={dragReqId !== null}
          on:dragover|preventDefault
          on:drop|preventDefault={() => moveToCollection(null)}
        ><span>Ungrouped</span></div>
        {#each grouped.loose as s (s.id)}
          <div
            class="req-item"
            class:active={draft.id === s.id}
            draggable="true"
            on:dragstart={() => (dragReqId = s.id)}
            on:dragend={() => (dragReqId = null)}
          >
            <input
              type="checkbox"
              class="ri-check"
              checked={selectedReqIds.has(s.id)}
              on:click|stopPropagation={() => toggleSelect(s.id)}
            />
            <button class="ri-main" on:click={() => loadSaved(s)}>
              <span class="mm" style="color:var(--m-{s.method.toLowerCase()})">{s.method}</span>
              <span class="rn">{s.name}</span>
            </button>
            <button class="del" on:click={() => delSaved(s)}>{@html ICONS.delete.svg}</button>
          </div>
        {/each}
      {/if}
      {#if $savedRequests.length === 0}
        <div class="muted" style="padding:10px">No saved requests.</div>
      {/if}
    </div>
    <div class="env-bar">
      <button class="btn ghost sm" on:click={() => (envModal = true)}>
        Env: {$activeEnvironment?.name || 'none'} {@html ICONS.expandOpen.svg}
      </button>
      <button class="btn ghost sm" title="Cookies collected from responses, replayed automatically" on:click={() => (cookieModal = true)}>
        {@html ICONS.cookie.svg} Cookies
      </button>
      <button class="btn ghost sm" title="Proxy, custom CA, client certificates" on:click={() => (networkModal = true)}>
        {@html ICONS.network.svg} Network
      </button>
      <button class="btn ghost sm" title="Local mock server — canned responses, no real backend" on:click={() => (mockModal = true)}>
        {@html ICONS.mockServer.svg} Mock
      </button>
      <button class="btn ghost sm" title="WebSocket connection tester" on:click={() => (wsModal = true)}>
        {@html ICONS.websocket.svg} WS
      </button>
      <button class="btn ghost sm" title="gRPC — reflection-based discovery, unary calls" on:click={() => (grpcModal = true)}>
        {@html ICONS.grpc.svg} gRPC
      </button>
    </div>
  </aside>

  <div class="sidebar-resizer" on:mousedown={startSidebarDrag} role="separator" tabindex="-1"></div>
  {/if}

  <section class="main">
    {#if $requestTabs.length === 0}
      <div class="empty">
        <div class="empty-picker">
          <p class="empty-hint">No tabs open — pick something to start.</p>
          <ConnPicker />
        </div>
      </div>
    {:else}
    <div class="urlbar">
      <select class="method" bind:value={draft.method} style="color:{methodColor}">
        {#each METHODS as m}<option value={m}>{m}</option>{/each}
      </select>
      <VarInput
        cls="url input mono"
        placeholder="https://api.example.com/v1/resource  —  {'{{baseUrl}}'} allowed"
        bind:value={draft.url}
        vars={availableVars}
        on:change={urlToParams}
        on:keydown={(e) => e.detail.key === 'Enter' && send()}
      />
      <button class="btn primary send" on:click={send} disabled={sending}>
        {sending ? '…' : 'Send'}
      </button>
      <button class="btn" on:click={save}>Save</button>
    </div>

    <div class="rq-work">
      <div class="req-pane" style="height:{splitPct}%">
        <div class="subtabs">
          {#each ['params', 'headers', 'auth', 'body', 'pre-request', 'tests'] as t}
            <button class:active={tab === t} on:click={() => (tab = t)}>
              {t === 'pre-request' ? 'Pre-request' : t === 'tests' ? 'Tests' : t}
              {#if t === 'headers' && draft.headers.filter((h) => h.k).length}
                <span class="n">{draft.headers.filter((h) => h.k).length}</span>
              {/if}
              {#if t === 'params' && draft.params.filter((p) => p.k).length}
                <span class="n">{draft.params.filter((p) => p.k).length}</span>
              {/if}
              {#if t === 'auth' && authType !== 'none'}<span class="n">1</span>{/if}
              {#if t === 'pre-request' && draft.pre_request_script?.trim()}<span class="n">●</span>{/if}
              {#if t === 'tests' && draft.test_script?.trim()}<span class="n">●</span>{/if}
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
                <option value="oauth2">OAuth 2.0</option>
                <option value="digest">Digest auth</option>
                <option value="awsv4">AWS Signature v4</option>
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
            {:else if authType === 'oauth2'}
              <div class="field">
                <label for="oauthGrant">Grant type</label>
                <select id="oauthGrant" class="select" bind:value={authOauth.grantType}>
                  <option value="client_credentials">Client Credentials</option>
                  <option value="authorization_code">Authorization Code (PKCE)</option>
                </select>
              </div>
              {#if authOauth.grantType === 'authorization_code'}
                <div class="field">
                  <label for="oauthAuthUrl">Authorization URL</label>
                  <input id="oauthAuthUrl" class="input mono" bind:value={authOauth.authUrl} placeholder="https://provider.com/oauth/authorize" />
                </div>
              {/if}
              <div class="field">
                <label for="oauthTokenUrl">Access token URL</label>
                <input id="oauthTokenUrl" class="input mono" bind:value={authOauth.tokenUrl} placeholder="https://provider.com/oauth/token" />
              </div>
              <div class="row2">
                <div class="field">
                  <label for="oauthClientId">Client ID</label>
                  <input id="oauthClientId" class="input mono" bind:value={authOauth.clientId} />
                </div>
                <div class="field">
                  <label for="oauthClientSecret">Client secret{authOauth.grantType === 'authorization_code' ? ' (optional for public clients)' : ''}</label>
                  <input id="oauthClientSecret" class="input mono" type="password" bind:value={authOauth.clientSecret} />
                </div>
              </div>
              <div class="field">
                <label for="oauthScope">Scope</label>
                <input id="oauthScope" class="input mono" bind:value={authOauth.scope} placeholder="read write" />
              </div>
              <button class="btn primary sm" on:click={fetchOauthToken} disabled={oauthBusy}>
                {oauthBusy ? 'Working…' : 'Get New Access Token'}
              </button>
              {#if oauthStatus}<p class="hint">{oauthStatus}</p>{/if}
              <p class="hint">
                {authOauth.grantType === 'authorization_code'
                  ? 'Opens your system browser to authorize, then catches the redirect on a local loopback listener — register http://127.0.0.1:<port>/callback as an allowed redirect URI with your provider (many accept any 127.0.0.1 port).'
                  : 'Server-to-server — no browser involved.'}
                On success, sets <code>Authorization: Bearer &lt;token&gt;</code> like the Bearer type above.
              </p>
            {:else if authType === 'digest'}
              <div class="row2">
                <div class="field">
                  <label for="digestUser">Username</label>
                  <input id="digestUser" class="input" bind:value={authDigest.username} />
                </div>
                <div class="field">
                  <label for="digestPass">Password</label>
                  <input id="digestPass" class="input" type="password" bind:value={authDigest.password} />
                </div>
              </div>
              <p class="hint">
                Sends the request once, reads the server's 401 challenge, and resends with the computed
                Digest response — MD5-based (RFC 2617), which is what almost every real Digest server still
                speaks.
              </p>
            {:else if authType === 'awsv4'}
              <div class="row2">
                <div class="field">
                  <label for="awsKeyId">Access key ID</label>
                  <input id="awsKeyId" class="input mono" bind:value={authAws.accessKeyId} />
                </div>
                <div class="field">
                  <label for="awsSecret">Secret access key</label>
                  <input id="awsSecret" class="input mono" type="password" bind:value={authAws.secretAccessKey} />
                </div>
              </div>
              <div class="row2">
                <div class="field">
                  <label for="awsRegion">Region</label>
                  <input id="awsRegion" class="input mono" bind:value={authAws.region} placeholder="us-east-1" />
                </div>
                <div class="field">
                  <label for="awsService">Service</label>
                  <input id="awsService" class="input mono" bind:value={authAws.service} placeholder="execute-api, s3, ..." />
                </div>
              </div>
              <div class="field">
                <label for="awsToken">Session token <span class="muted">(optional, for temporary credentials)</span></label>
                <input id="awsToken" class="input mono" bind:value={authAws.sessionToken} />
              </div>
              <p class="hint">Signs the request with SigV4 right before sending — verified against AWS's own documented signing example.</p>
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
                <VarInput
                  cls="input mono"
                  placeholder="value"
                  bind:value={row.v}
                  vars={availableVars}
                  on:input={tab === 'params' ? onParamInput : onHeaderInput}
                />
              </div>
            {/each}
          </div>
        {:else if tab === 'pre-request'}
          <p class="hint script-hint">
            Runs before the request is sent. Has <code>pm.environment</code>/<code>pm.globals</code>
            get/set/unset, <code>pm.variables.get</code>, and a mutable <code>pm.request</code>
            ({'{'}method, url, headers, body{'}'}) — changes to it apply to the outgoing request.
          </p>
          <div class="body-editor">
            <CodeEditor bind:value={draft.pre_request_script} language="text" on:run={send} />
          </div>
        {:else if tab === 'tests'}
          <p class="hint script-hint">
            Runs after the response arrives. Has <code>pm.response</code> ({'{'}code, status, responseTime,
            headers, json(), text(){'}'}), <code>pm.test(name, fn)</code>, and <code>pm.expect(actual)</code>
            (equal/eql/include/above/below/ok). Results show in the response panel's Tests tab.
          </p>
          <div class="body-editor">
            <CodeEditor bind:value={draft.test_script} language="text" on:run={send} />
          </div>
        {:else}
          <div class="body-tools">
            <select class="select sm" bind:value={draft.bodyMode}>
              <option value="raw">Raw</option>
              <option value="urlencoded">x-www-form-urlencoded</option>
              <option value="multipart">form-data</option>
              <option value="binary">Binary</option>
              <option value="graphql">GraphQL</option>
            </select>
            {#if draft.bodyMode === 'raw'}
              <button class="btn ghost sm" on:click={prettyBody}>Beautify JSON</button>
            {/if}
            <span class="muted">{['GET', 'HEAD'].includes(draft.method) ? 'body ignored for ' + draft.method : ''}</span>
          </div>
          {#if draft.bodyMode === 'raw'}
            <div class="body-editor">
              <CodeEditor bind:value={draft.body} language={bodyLang()} on:run={send} />
            </div>
          {:else if draft.bodyMode === 'urlencoded' || draft.bodyMode === 'multipart'}
            <div class="kv-grid">
              {#each draft.formFields as row}
                <div class="kv-row form-row" class:multipart={draft.bodyMode === 'multipart'}>
                  <input type="checkbox" bind:checked={row.on} />
                  <input class="input mono" placeholder="key" bind:value={row.k} on:input={onFormFieldInput} />
                  {#if draft.bodyMode === 'multipart'}
                    <select class="select sm" bind:value={row.kind}>
                      <option value="text">Text</option>
                      <option value="file">File</option>
                    </select>
                  {/if}
                  {#if draft.bodyMode === 'multipart' && row.kind === 'file'}
                    <span class="file-field">
                      <input type="file" on:change={(e) => onMultipartFilePick(e, row)} />
                      {#if row.filename}<span class="fname">{row.filename}</span>{/if}
                    </span>
                  {:else}
                    <input class="input mono" placeholder="value" bind:value={row.v} on:input={onFormFieldInput} />
                  {/if}
                </div>
              {/each}
            </div>
          {:else if draft.bodyMode === 'binary'}
            <div class="binary-body">
              <input type="file" on:change={onBinaryFilePick} />
              {#if draft.binaryFile}
                <span class="fname">{draft.binaryFile.filename} · {fmtSize(draft.binaryFile.size)}</span>
                <button class="btn ghost sm" on:click={() => (draft.binaryFile = null)}>Clear</button>
              {/if}
            </div>
          {:else if draft.bodyMode === 'graphql'}
            <div class="graphql-body">
              <div class="gql-pane">
                <div class="gql-label">Query</div>
                <CodeEditor bind:value={draft.graphqlQuery} language="text" on:run={send} />
              </div>
              <div class="gql-pane">
                <div class="gql-label">Variables (JSON)</div>
                <CodeEditor bind:value={draft.graphqlVariables} language="json" on:run={send} />
              </div>
            </div>
          {/if}
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
              <button class="btn ghost sm" on:click={inspectResponse}>{@html ICONS.toInspector.svg} Inspector</button>
              <button class="btn ghost sm" title="Export as CSV (needs an array of objects)" on:click={exportResponseCsv}
                >Export CSV</button
              >
            {/if}
            <button class="btn ghost sm" on:click={saveResponse}>{response.is_json ? 'Export JSON' : 'Save'}</button>
            <button class="btn ghost sm" title="Copy response body" on:click={copyResponse}>{@html ICONS.copy.svg} Body</button>
            <span class="code-menu-wrap">
              <button class="btn ghost sm" class:on={showCodeMenu} on:click={() => (showCodeMenu = !showCodeMenu)}>
                {@html ICONS.code.svg} Code
              </button>
              {#if showCodeMenu}
                <div class="backdrop" on:click={() => (showCodeMenu = false)} role="presentation" />
                <div class="code-menu">
                  <div class="code-menu-head">
                    {#each Object.entries(CODE_GENERATORS) as [key, g]}
                      <button class:active={codeLang === key} on:click={() => (codeLang = key)}>{g.label}</button>
                    {/each}
                  </div>
                  <pre class="code-snippet">{codeSnippet()}</pre>
                  <button class="btn ghost sm" on:click={copyCodeSnippet}>{@html ICONS.copy.svg} Copy</button>
                </div>
              {/if}
            </span>
            <div class="subtabs sm">
              <button class:active={respTab === 'body'} on:click={() => (respTab = 'body')}>Body</button>
              <button class:active={respTab === 'headers'} on:click={() => (respTab = 'headers')}>
                Headers <span class="n">{response.headers.length}</span>
              </button>
              {#if response.testResults}
                <button class:active={respTab === 'tests'} on:click={() => (respTab = 'tests')}>
                  Tests
                  <span class="n" class:bad={response.testResults.some((t) => !t.passed)}>
                    {response.testResults.filter((t) => t.passed).length}/{response.testResults.length}
                  </span>
                </button>
              {/if}
              {#if previewKind}
                <button class:active={respTab === 'preview'} on:click={() => (respTab = 'preview')}>Preview</button>
              {/if}
            </div>
          </div>
          {#if respTab === 'body'}
            <div class="resp-body">
              {#if response.is_binary}
                <div class="empty">
                  Binary response ({response.content_type || 'unknown type'}, {fmtSize(response.size_bytes)}).
                  {#if previewKind}See the Preview tab, or {/if}Use Save to write it to a file.
                </div>
              {:else}
                <CodeEditor
                  value={response.is_json ? tryPretty(response.body) : response.body}
                  language={response.is_json ? 'json' : 'text'}
                  readonly
                />
              {/if}
            </div>
          {:else if respTab === 'preview'}
            <div class="resp-preview">
              {#if previewKind === 'html'}
                <iframe title="Response preview" class="preview-frame" sandbox="" srcdoc={response.body}></iframe>
              {:else if previewKind === 'image'}
                <img class="preview-image" alt="Response preview" src={`data:${response.content_type};base64,${response.body}`} />
              {:else if previewKind === 'pdf'}
                <embed class="preview-frame" type="application/pdf" src={`data:application/pdf;base64,${response.body}`} />
              {/if}
            </div>
          {:else if respTab === 'tests'}
            <div class="resp-tests">
              {#each response.testResults || [] as t}
                <div class="test-row" class:fail={!t.passed}>
                  <span class="test-dot">{@html t.passed ? ICONS.check.svg : ICONS.cancel.svg}</span>
                  <span class="test-name">{t.name}</span>
                  {#if t.error}<span class="test-err">{t.error}</span>{/if}
                </div>
              {/each}
              {#if !response.testResults?.length}
                <div class="empty">No pm.test(...) assertions in the test script.</div>
              {/if}
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
    {/if}
  </section>
</div>

{#if envModal}
  <EnvModal on:close={() => (envModal = false)} />
{/if}
{#if cookieModal}
  <CookieManagerModal on:close={() => (cookieModal = false)} />
{/if}
{#if networkModal}
  <NetworkSettingsModal on:close={() => (networkModal = false)} />
{/if}
{#if mockModal}
  <MockServerModal on:close={() => (mockModal = false)} />
{/if}
{#if wsModal}
  <WebSocketPanel on:close={() => (wsModal = false)} />
{/if}
{#if grpcModal}
  <GrpcPanel on:close={() => (grpcModal = false)} />
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
    flex-shrink: 0;
    background: var(--surface-1);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .sidebar-resizer {
    width: 5px;
    flex-shrink: 0;
    cursor: col-resize;
    background: var(--border);
  }
  .sidebar-resizer:hover {
    background: var(--tool-requests-text);
  }
  .sec-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: nowrap;
    gap: 6px;
    padding: 8px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border);
  }
  .sec-head > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sec-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }
  /* import/export glyphs (⇩ ⇧) read as tiny arrows at the standard
     icon-btn size — size them up without growing the button box. */
  .sec-actions :global(.big-glyph) {
    font-size: 18px;
  }
  /* the new-folder icon is an inline SVG with its own fixed w/h — scale
     it up to match the bigger buttons here without touching the size
     used everywhere else this icon appears. */
  .sec-actions :global(.icon-btn svg) {
    width: 16px;
    height: 16px;
  }
  .scroll {
    flex: 1;
    overflow: auto;
    padding: 4px 0;
  }
  .col-head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 6px 8px;
    margin-top: 2px;
    background: none;
    border: none;
    cursor: pointer;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .col-head:hover {
    background: var(--surface-3);
  }
  .col-head.drop-target {
    outline: 1px dashed var(--tool-requests-text, var(--accent));
    outline-offset: -1px;
    background: var(--surface-3);
  }
  .col-head .chev {
    font-size: 9px;
    width: 10px;
    text-align: center;
    flex-shrink: 0;
  }
  .col-head .col-name {
    flex: 1;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .col-head .col-cnt {
    font-weight: 400;
    flex-shrink: 0;
  }
  .req-item {
    display: flex;
    align-items: center;
  }
  .req-item.active {
    background: color-mix(in srgb, var(--tool-requests-text) 14%, transparent);
  }
  .ri-check {
    flex-shrink: 0;
    margin-left: 8px;
    cursor: pointer;
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
  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }
  .empty-picker {
    width: 360px;
    max-height: 80%;
    display: flex;
    flex-direction: column;
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
  }
  .empty-hint {
    margin: 0;
    padding: 12px 14px 0;
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
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
  .n.bad {
    background: color-mix(in srgb, var(--danger) 25%, transparent);
    color: var(--danger);
  }
  .code-menu-wrap {
    position: relative;
  }
  .code-menu-wrap .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }
  .code-menu-wrap .btn.on {
    background: var(--tool-requests-tint);
    color: var(--tool-requests-text);
  }
  .code-menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 41;
    width: 420px;
    max-width: 80vw;
    display: flex;
    flex-direction: column;
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
    padding: 8px;
    gap: 6px;
  }
  .code-menu-head {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .code-menu-head button {
    all: unset;
    cursor: pointer;
    font-size: 10.5px;
    padding: 3px 8px;
    border-radius: 4px;
    color: var(--text-secondary);
  }
  .code-menu-head button.active {
    background: var(--tool-requests-tint);
    color: var(--tool-requests-text);
  }
  .code-snippet {
    max-height: 280px;
    overflow: auto;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 8px;
    font-family: var(--font-mono);
    font-size: 11px;
    white-space: pre-wrap;
    word-break: break-word;
    margin: 0;
  }
  .script-hint {
    padding: 8px 10px;
    margin: 0;
    font-size: 11px;
    line-height: 1.5;
    border-bottom: 1px solid var(--border);
  }
  .script-hint code {
    font-family: var(--font-mono);
    background: var(--surface-3);
    padding: 0 3px;
    border-radius: 3px;
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
  .select.sm {
    font-size: 11px;
    padding: 3px 6px;
  }
  .form-row {
    grid-template-columns: 18px 1fr auto 1.5fr;
  }
  .form-row:not(.multipart) {
    grid-template-columns: 18px 1fr 1.5fr;
  }
  .file-field {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    overflow: hidden;
  }
  .fname {
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .binary-body {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px;
  }
  .graphql-body {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .gql-pane {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    border-bottom: 1px solid var(--border);
  }
  .gql-pane:last-child {
    border-bottom: none;
  }
  .gql-label {
    font-size: 10px;
    color: var(--text-muted);
    padding: 4px 8px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .gql-pane :global(.editor) {
    flex: 1;
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
  .resp-preview {
    height: 100%;
    overflow: auto;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    background: var(--surface-2);
  }
  .preview-frame {
    width: 100%;
    height: 100%;
    border: none;
    background: #fff;
  }
  .preview-image {
    max-width: 100%;
    max-height: 100%;
    margin: 12px auto;
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
  .resp-tests {
    overflow: auto;
    padding: 8px;
    font-size: 12px;
  }
  .test-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 5px 4px;
    border-bottom: 1px solid var(--border);
  }
  .test-dot {
    color: var(--ok);
    font-weight: 700;
    width: 14px;
    flex-shrink: 0;
  }
  .test-row.fail .test-dot {
    color: var(--danger);
  }
  .test-name {
    flex-shrink: 0;
  }
  .test-err {
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: 11px;
    word-break: break-all;
  }
</style>
